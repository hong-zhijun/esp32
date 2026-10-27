#!/usr/bin/env python3
"""Build and run the real firmware UI with host clang/cc on macOS or Linux."""
import argparse
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
import os
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--arduino-json', type=Path,
                        default=Path.home() / 'Documents/Arduino/libraries/ArduinoJson/src')
    args = parser.parse_args()
    if not (args.arduino_json / 'ArduinoJson.h').is_file():
        parser.error('--arduino-json must contain ArduinoJson.h (ArduinoJson 7)')
    repo = Path(__file__).resolve().parents[3]
    build = repo / 'build/lvgl-row-host'
    objects = build / 'obj'
    objects.mkdir(parents=True, exist_ok=True)
    config = (repo / 'firmware/libraries/lvgl8/lv_conf.h').read_text()
    for before, after in [('LV_COLOR_DEPTH 16', 'LV_COLOR_DEPTH 32'),
                          ('LV_COLOR_16_SWAP 1', 'LV_COLOR_16_SWAP 0'),
                          ('LV_COLOR_SCREEN_TRANSP 1', 'LV_COLOR_SCREEN_TRANSP 0')]:
        config = config.replace(before, after)
    config_file = build / 'lv_conf.h'
    if not config_file.exists() or config_file.read_text() != config:
        config_file.write_text(config)
    lvgl = repo / 'firmware/libraries/lvgl8/lvgl'
    sketch = repo / 'firmware/ai-status-screen'
    includes = ['-I' + str(p) for p in [build, lvgl, sketch, args.arduino_json]]
    flags = ['-O1', '-w', '-DLV_CONF_INCLUDE_SIMPLE'] + includes
    sources = sorted((lvgl / 'src').rglob('*.c'))
    sources += [sketch / ('panel_font_' + str(size) + '.c') for size in [16, 20, 24, 28, 30, 'meta_16']]
    sources += [Path(__file__).with_name('main.c'), Path(__file__).with_name('panel_host.cpp')]

    def compile_source(source):
        obj = objects / ('_'.join(source.relative_to(repo).parts) + '.o')
        newest = max(source.stat().st_mtime, config_file.stat().st_mtime)
        if source.suffix == '.cpp':
            newest = max(newest, *(p.stat().st_mtime for p in sketch.glob('*.h')),
                         (sketch / 'panel_ui.cpp').stat().st_mtime)
        if not obj.exists() or obj.stat().st_mtime < newest:
            compiler = os.environ.get('CXX', 'c++') if source.suffix == '.cpp' else os.environ.get('CC', 'cc')
            standard = '-std=c++17' if source.suffix == '.cpp' else '-std=c11'
            subprocess.run([compiler, standard] + flags + ['-c', str(source), '-o', str(obj)], check=True)
        return obj

    with ThreadPoolExecutor(max_workers=min(os.cpu_count() or 2, 8)) as workers:
        compiled = list(workers.map(compile_source, sources))
    binary = build / 'lvgl-row-test'
    subprocess.run([os.environ.get('CXX', 'c++'), '-o', str(binary)] + [str(p) for p in compiled], check=True)
    subprocess.run([str(binary)], cwd=build, check=True)


if __name__ == '__main__':
    main()
