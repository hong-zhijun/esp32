//! Streaming linear resampler from the board's 16 kHz mono PCM to the device rate.
//! Each output frame is duplicated to every device channel.

const INPUT_RATE: u64 = 16_000;

pub(crate) struct Resampler {
    output_rate: u64,
    channels: usize,
    /// Last input sample; interpolation runs between it and the next one.
    previous: Option<i16>,
    /// Position of the next output frame, in 1/output_rate units of one input sample.
    phase: u64,
    /// Odd trailing byte waiting for its partner in the next chunk.
    carry: Option<u8>,
}

impl Resampler {
    /// The device rate must not be below the input rate: the phase arithmetic
    /// only handles upsampling, and the device picker guarantees that.
    pub(crate) fn new(output_rate: u32, channels: usize) -> Self {
        let output_rate = u64::from(output_rate);
        assert!(
            output_rate >= INPUT_RATE,
            "device rate must be at least 16 kHz"
        );
        Self {
            output_rate,
            channels,
            previous: None,
            phase: 0,
            carry: None,
        }
    }

    /// True when the stream ended in the middle of a 16-bit sample.
    pub(crate) fn has_partial_sample(&self) -> bool {
        self.carry.is_some()
    }

    /// Appends the interleaved output for `pcm` to `out`. State carries over between calls.
    pub(crate) fn push(&mut self, pcm: &[u8], out: &mut Vec<i16>) {
        let mut rest = pcm;
        if let Some(low) = self.carry.take() {
            match rest.split_first() {
                Some((&high, tail)) => {
                    self.feed(i16::from_le_bytes([low, high]), out);
                    rest = tail;
                }
                None => {
                    self.carry = Some(low);
                    return;
                }
            }
        }
        let mut pairs = rest.chunks_exact(2);
        for pair in &mut pairs {
            self.feed(i16::from_le_bytes([pair[0], pair[1]]), out);
        }
        self.carry = pairs.remainder().first().copied();
    }

    fn feed(&mut self, sample: i16, out: &mut Vec<i16>) {
        let Some(previous) = self.previous.replace(sample) else {
            return;
        };
        let (from, to) = (f64::from(previous), f64::from(sample));
        while self.phase < self.output_rate {
            let fraction = self.phase as f64 / self.output_rate as f64;
            let value = (from + (to - from) * fraction).round() as i16;
            out.extend(std::iter::repeat_n(value, self.channels));
            self.phase += INPUT_RATE;
        }
        self.phase -= self.output_rate;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    #[test]
    fn upsamples_by_three_with_linear_interpolation_and_duplicates_channels() {
        let mut r = Resampler::new(48_000, 2);
        let mut out = Vec::new();
        r.push(&bytes(&[0, 300, 600]), &mut out);
        // The first sample only seeds the history; each later one yields three frames.
        assert_eq!(
            out,
            vec![0, 0, 100, 100, 200, 200, 300, 300, 400, 400, 500, 500]
        );
    }

    #[test]
    fn output_does_not_depend_on_chunk_boundaries_including_odd_bytes() {
        let samples: Vec<i16> = (0..200).map(|i| (i * 97 % 2000 - 1000) as i16).collect();
        let pcm = bytes(&samples);

        let mut whole = Resampler::new(44_100, 1);
        let mut expected = Vec::new();
        whole.push(&pcm, &mut expected);

        let mut split = Resampler::new(44_100, 1);
        let mut actual = Vec::new();
        for chunk in pcm.chunks(7) {
            split.push(chunk, &mut actual);
        }
        assert_eq!(actual, expected);
        assert!(!split.has_partial_sample());
    }

    #[test]
    fn output_rate_tracks_device_rate_without_drift() {
        let mut r = Resampler::new(44_100, 1);
        let mut out = Vec::new();
        r.push(&bytes(&vec![0; 16_000]), &mut out);
        // 15999 interpolated intervals at 44.1 kHz, within one frame of exact.
        let expected = 15_999 * 44_100 / 16_000;
        assert!(
            (out.len() as i64 - expected as i64).abs() <= 1,
            "got {}",
            out.len()
        );
    }

    #[test]
    fn dangling_byte_is_reported() {
        let mut r = Resampler::new(48_000, 2);
        let mut out = Vec::new();
        r.push(&[1, 0, 2], &mut out);
        assert!(r.has_partial_sample());
    }
}
