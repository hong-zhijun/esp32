//! 16 kHz mono int16 LE PCM helpers shared by the bridge and its diagnostics.

/// Board audio is 16 kHz, mono, 16-bit: 2 bytes per sample.
const BYTES_PER_SECOND: f64 = 16_000.0 * 2.0;

/// RMS at or above this counts as speech in the diagnostics log.
const SIGNAL_RMS: u32 = 120;

/// Peak and RMS of a PCM chunk, plus whether it carries speech.
pub(crate) struct Level {
    pub peak: u32,
    pub rms: u32,
}

impl Level {
    pub(crate) fn has_signal(&self) -> bool {
        self.rms >= SIGNAL_RMS
    }
}

pub(crate) fn seconds(bytes: u64) -> f64 {
    bytes as f64 / BYTES_PER_SECOND
}

/// Trailing odd byte is ignored, as the Node version did.
pub(crate) fn level(pcm: &[u8]) -> Level {
    let mut peak = 0i64;
    let mut sum = 0i64;
    let mut count = 0i64;
    for pair in pcm.chunks_exact(2) {
        let sample = i64::from(i16::from_le_bytes([pair[0], pair[1]]));
        peak = peak.max(sample.abs());
        sum += sample * sample;
        count += 1;
    }
    let rms = (sum as f64 / count.max(1) as f64).sqrt().round() as u32;
    Level {
        peak: peak as u32,
        rms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_reports_peak_and_rms() {
        let pcm: Vec<u8> = [100i16, -200, 300]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let l = level(&pcm);
        assert_eq!(l.peak, 300);
        assert_eq!(l.rms, 216);
        assert!(l.has_signal());
    }

    #[test]
    fn empty_and_odd_length_chunks_are_silent() {
        assert_eq!(level(&[]).rms, 0);
        let l = level(&[0xff]);
        assert_eq!((l.peak, l.rms), (0, 0));
        assert!(!l.has_signal());
    }

    #[test]
    fn seconds_uses_16k_mono_16bit() {
        assert_eq!(seconds(32_000), 1.0);
    }
}
