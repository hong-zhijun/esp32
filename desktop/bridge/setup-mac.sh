#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
python3 -m venv .venv
.venv/bin/python -m pip install -r bridge/requirements.txt
brew install --cask blackhole-2ch
mkdir -p ../build
xcrun clang bridge/wetype-key.c -framework ApplicationServices -o ../build/wetype-key
xcrun clang bridge/wetype-status.c -framework CoreAudio -framework CoreFoundation -o ../build/wetype-status
.venv/bin/python bridge/audio-output.py --check
