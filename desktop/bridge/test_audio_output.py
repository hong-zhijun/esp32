import struct
import unittest
from importlib.machinery import SourceFileLoader
Resampler = SourceFileLoader('audio_output', __file__.replace('test_audio_output.py', 'audio-output.py')).load_module().Resampler

class AudioTests(unittest.TestCase):
    def test_resampling_preserves_duration_channels_and_packet_boundaries(self):
        pcm = struct.pack('<5h', 0, 3000, -3000, 32767, -32768)
        complete = Resampler().convert(pcm)
        r = Resampler()
        chunks = r.convert(pcm[:3]) + r.convert(pcm[3:7]) + r.convert(pcm[7:])
        self.assertEqual(chunks, complete)
        self.assertEqual(len(complete), 5 * 3 * 2 * 2)
        self.assertEqual(r.carry, b'')
        stereo = list(struct.iter_unpack('<hh', complete))
        self.assertTrue(all(left == right for left, right in stereo))
        self.assertEqual(stereo[-1], (-32768, -32768))

    def test_silence_is_silence(self):
        self.assertEqual(Resampler().convert(bytes(640)), bytes(3840))

if __name__ == '__main__': unittest.main()
