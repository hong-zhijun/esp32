"""Render 16 kHz mono PCM from stdin into a named virtual audio cable.
No microphone capture, default-speaker fallback, cloud API, or transcription.
"""
import argparse
import collections
import json
import struct
import sys
import threading
import time


def emit(event, **kwargs):
    print(json.dumps(dict(event=event, **kwargs), ensure_ascii=False), flush=True)


class Resampler:
    """16 kHz -> 48 kHz linear interpolation, duplicated to stereo."""
    def __init__(self):
        self.previous = 0
        self.carry = b''

    def convert(self, data):
        data = self.carry + data
        self.carry = data[len(data) & ~1:]
        out = bytearray()
        for (sample,) in struct.iter_unpack('<h', data[:len(data) & ~1]):
            for n in (1, 2, 3):
                v = round(self.previous + (sample - self.previous) * n / 3)
                out.extend(struct.pack('<hh', v, v))
            self.previous = sample
        return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--list', action='store_true')
    parser.add_argument('--device', default='BlackHole 2ch' if sys.platform == 'darwin' else 'CABLE Input')
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    import sounddevice as sd
    devices = sd.query_devices()
    hosts = sd.query_hostapis()
    rows = [dict(index=i, name=d['name'], channels=d['max_output_channels'],
                 host=hosts[d['hostapi']]['name']) for i, d in enumerate(devices) if d['max_output_channels'] >= 2]
    if args.list:
        emit('devices', devices=rows)
        return
    candidates = [d for d in rows if args.device.casefold() in d['name'].casefold()]
    candidates.sort(key=lambda d: 'WASAPI' not in d['host'])
    if not candidates:
        raise RuntimeError('未找到虚拟音频输出设备：' + args.device + '；请先安装虚拟声卡')
    device = candidates[0]
    sd.check_output_settings(device=device['index'], channels=2, dtype='int16', samplerate=48000)
    if args.check:
        emit('ready', device=device)
        return
    queue = collections.deque()
    lock = threading.Lock()
    state = dict(bytes=0, underruns=0, frames=0, overflow=False, started=False)
    eof = threading.Event()

    def callback(outdata, frames, timing, status):
        size = frames * 4
        result = bytearray(size)
        with lock:
            if not state['started'] and (state['bytes'] >= 48000 * 4 * .08 or eof.is_set()):
                state['started'] = True
            if state['started']:
                cursor = 0
                while queue and cursor < size:
                    chunk = queue[0]
                    n = min(size - cursor, len(chunk))
                    result[cursor:cursor+n] = chunk[:n]
                    cursor += n
                    state['bytes'] -= n
                    if n == len(chunk):
                        queue.popleft()
                    else:
                        queue[0] = chunk[n:]
                state['frames'] += cursor // 4
                if cursor < size and not eof.is_set():
                    state['underruns'] += 1
        outdata[:] = result

    with sd.RawOutputStream(device=device['index'], samplerate=48000, channels=2,
                            dtype='int16', blocksize=480, latency='low', callback=callback):
        emit('ready', device=device)
        converter = Resampler()
        while True:
            pcm = sys.stdin.buffer.read1(4096)
            if not pcm:
                break
            audio = converter.convert(pcm)
            with lock:
                if state['bytes'] + len(audio) > 48000 * 4 * 2:
                    state['overflow'] = True
                    raise RuntimeError('虚拟麦克风积压超过 2 秒，已停止以避免越来越慢')
                queue.append(audio)
                state['bytes'] += len(audio)
        if converter.carry:
            raise RuntimeError('收到不完整的 PCM 采样')
        eof.set()
        deadline = time.monotonic() + 3
        while time.monotonic() < deadline:
            with lock:
                pending = state['bytes']
            if pending == 0:
                break
            time.sleep(.01)
        if pending:
            raise RuntimeError('音频设备未及时消费录音')
        time.sleep(.2)  # allow the last hardware buffer to reach the cable input
    emit('drained', audioSeconds=state['frames'] / 48000, underruns=state['underruns'])


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        emit('error', message=str(error))
        sys.exit(1)
