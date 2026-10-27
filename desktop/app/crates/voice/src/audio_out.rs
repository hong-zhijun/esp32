//! Plays the board's 16 kHz PCM into a virtual cable (VB-CABLE / BlackHole) through cpal.
//!
//! The stream lives on its own thread because cpal streams are not `Send` on every
//! platform. The sink feeds a shared queue; the device callback drains it.
//! The default speaker is never used as a fallback.

use crate::ports::{AudioSink, AudioSinkFactory, Canceller, DrainStats};
use crate::resample::Resampler;
use anyhow::{anyhow, bail, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    default_host, Device, Host, OutputCallbackInfo, SampleFormat, SizedSample, Stream, StreamConfig,
};
use parking_lot::{Condvar, Mutex};
use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const INPUT_RATE: u32 = 16_000;
const TARGET_RATE: u32 = 48_000;
const PREBUFFER_MS: usize = 80;
const MAX_BACKLOG_SECONDS: usize = 2;
const OPEN_TIMEOUT: Duration = Duration::from_secs(8);
const DRAIN_TIMEOUT: Duration = Duration::from_secs(3);
/// Lets the last hardware buffer reach the cable input after the queue empties.
const TAIL: Duration = Duration::from_millis(200);
const CANCELLED: &str = "音频输出已取消";
const BACKLOG_MESSAGE: &str = "虚拟麦克风积压超过 2 秒，已停止以避免越来越慢";

/// Opens a [`CpalSink`] per recording.
pub(crate) struct CpalFactory;

impl AudioSinkFactory for CpalFactory {
    fn open(&self, device: &str) -> Result<Box<dyn AudioSink>> {
        Ok(Box::new(CpalSink::open(device)?))
    }
}

/// Names of the output devices with at least two channels, in host order, without duplicates.
pub(crate) fn list_output_devices() -> Vec<String> {
    let Ok(devices) = default_host().output_devices() else {
        return Vec::new();
    };
    let mut names: Vec<String> = Vec::new();
    for device in devices.filter(has_stereo_output) {
        if let Ok(name) = device_name(&device) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

/// Finds the device and checks that a stream can be built for it, without playing anything.
/// Returns the resolved device name.
pub(crate) fn probe(wanted: &str) -> Result<String> {
    let (opened, _keep_open) = spawn_stream(wanted, false)?;
    Ok(opened.name)
}

/// Audio output to the virtual cable. Dropping it closes the device.
pub(crate) struct CpalSink {
    name: String,
    queue: Arc<Queue>,
    resampler: Resampler,
    scratch: Vec<i16>,
    rate: u32,
    /// Dropping this sender ends the stream thread, which releases the device.
    _keep_open: Sender<()>,
}

impl CpalSink {
    fn open(wanted: &str) -> Result<Self> {
        let (opened, keep_open) = spawn_stream(wanted, true)?;
        Ok(Self {
            name: opened.name,
            resampler: Resampler::new(opened.rate, opened.channels),
            scratch: Vec::new(),
            rate: opened.rate,
            queue: opened.queue,
            _keep_open: keep_open,
        })
    }
}

impl AudioSink for CpalSink {
    fn device_name(&self) -> &str {
        &self.name
    }

    fn canceller(&self) -> Canceller {
        let queue = self.queue.clone();
        Box::new(move || queue.fail(CANCELLED.into()))
    }

    fn write(&mut self, pcm16k: &[u8]) -> Result<()> {
        self.scratch.clear();
        self.resampler.push(pcm16k, &mut self.scratch);
        self.queue.ring.lock().push(&self.scratch)
    }

    fn finish(self: Box<Self>) -> Result<DrainStats> {
        if self.resampler.has_partial_sample() {
            bail!("收到不完整的 PCM 采样");
        }
        let queue = self.queue.clone();
        let mut ring = queue.ring.lock();
        ring.eof = true;
        let deadline = Instant::now() + DRAIN_TIMEOUT;
        while !ring.samples.is_empty() && ring.error.is_none() {
            if queue.changed.wait_until(&mut ring, deadline).timed_out() {
                break;
            }
        }
        if let Some(error) = &ring.error {
            bail!("虚拟音频设备出错：{error}");
        }
        if !ring.samples.is_empty() {
            bail!("音频设备未及时消费录音");
        }
        let (played, underruns) = (ring.played_frames, ring.underruns);
        drop(ring);
        thread::sleep(TAIL);
        Ok(DrainStats {
            audio_seconds: played as f64 / f64::from(self.rate),
            underruns,
        })
    }
}

/// What the stream thread reports once the device is open.
struct Opened {
    name: String,
    queue: Arc<Queue>,
    rate: u32,
    channels: usize,
}

/// Builds the stream on a new thread and waits for the result. The returned sender
/// keeps the stream alive until it is dropped.
fn spawn_stream(wanted: &str, play: bool) -> Result<(Opened, Sender<()>)> {
    let (ready_tx, ready_rx) = mpsc::channel();
    let (keep_tx, keep_rx) = mpsc::channel();
    let wanted = wanted.to_owned();
    thread::Builder::new()
        .name("voice-output".into())
        .spawn(move || run_stream(&wanted, play, ready_tx, keep_rx))?;
    match ready_rx.recv_timeout(OPEN_TIMEOUT) {
        Ok(opened) => Ok((opened?, keep_tx)),
        Err(RecvTimeoutError::Timeout) => bail!("虚拟声卡启动超时"),
        Err(RecvTimeoutError::Disconnected) => bail!("虚拟声卡启动失败"),
    }
}

fn run_stream(wanted: &str, play: bool, ready: Sender<Result<Opened>>, keep_open: Receiver<()>) {
    match build(wanted, play) {
        Err(error) => {
            let _ = ready.send(Err(error));
        }
        Ok((stream, opened)) => {
            // `stream` stays alive while the owner holds `keep_open`.
            if ready.send(Ok(opened)).is_ok() {
                let _ = keep_open.recv();
            }
            drop(stream);
        }
    }
}

fn build(wanted: &str, play: bool) -> Result<(Stream, Opened)> {
    let device = find_device(&default_host(), wanted)?;
    let name = device_name(&device)?;
    let format = choose_format(&device)?;
    let queue = Arc::new(Queue::new(
        format.config.sample_rate,
        usize::from(format.config.channels),
    ));
    let stream = match format.sample_format {
        SampleFormat::I16 => build_stream::<i16>(&device, &format.config, &queue)?,
        _ => build_stream::<f32>(&device, &format.config, &queue)?,
    };
    if play {
        stream.play()?;
    }
    let opened = Opened {
        name,
        queue,
        rate: format.config.sample_rate,
        channels: usize::from(format.config.channels),
    };
    Ok((stream, opened))
}

fn build_stream<T: DevicePcm>(
    device: &Device,
    config: &StreamConfig,
    queue: &Arc<Queue>,
) -> Result<Stream> {
    let (play_queue, error_queue) = (queue.clone(), queue.clone());
    let stream = device.build_output_stream::<T, _, _>(
        *config,
        move |out: &mut [T], _: &OutputCallbackInfo| play_queue.fill(out),
        move |error| error_queue.fail(error.to_string()),
        Some(OPEN_TIMEOUT),
    )?;
    Ok(stream)
}

fn has_stereo_output(device: &Device) -> bool {
    device
        .supported_output_configs()
        .is_ok_and(|mut configs| configs.any(|c| c.channels() >= 2))
}

fn device_name(device: &Device) -> Result<String> {
    Ok(device.description()?.name().to_string())
}

/// Case-insensitive substring match among stereo-capable outputs. Never falls back to another device.
fn find_device(host: &Host, wanted: &str) -> Result<Device> {
    let needle = wanted.trim().to_lowercase();
    if needle.is_empty() {
        bail!("未设置虚拟音频输出设备；请先安装虚拟声卡");
    }
    for device in host.output_devices()? {
        if !has_stereo_output(&device) {
            continue;
        }
        if device_name(&device).is_ok_and(|name| name.to_lowercase().contains(&needle)) {
            return Ok(device);
        }
    }
    bail!(
        "未找到虚拟音频输出设备：{}；请先安装虚拟声卡",
        wanted.trim()
    )
}

struct Format {
    config: StreamConfig,
    sample_format: SampleFormat,
}

/// Prefers 2 channels, then 16-bit samples, then the rate closest to 48 kHz
/// (clamped into the device's range and never below the 16 kHz input rate).
fn choose_format(device: &Device) -> Result<Format> {
    let mut best: Option<((bool, bool, u32), Format)> = None;
    for range in device.supported_output_configs()? {
        let (channels, sample_format) = (range.channels(), range.sample_format());
        if channels < 2 || !matches!(sample_format, SampleFormat::I16 | SampleFormat::F32) {
            continue;
        }
        let low = range.min_sample_rate().max(INPUT_RATE);
        let high = range.max_sample_rate();
        if low > high {
            continue;
        }
        let rate = TARGET_RATE.clamp(low, high);
        let Some(supported) = range.try_with_sample_rate(rate) else {
            continue;
        };
        let key = (
            channels != 2,
            sample_format != SampleFormat::I16,
            rate.abs_diff(TARGET_RATE),
        );
        if best.as_ref().is_none_or(|(best_key, _)| key < *best_key) {
            best = Some((
                key,
                Format {
                    config: supported.config(),
                    sample_format,
                },
            ));
        }
    }
    best.map(|(_, format)| format)
        .ok_or_else(|| anyhow!("虚拟声卡没有可用的 16 kHz 录音输出格式（需要至少 2 声道）"))
}

/// Device sample types the stream can be built with.
trait DevicePcm: SizedSample + Send + 'static {
    fn from_pcm(sample: i16) -> Self;
}

impl DevicePcm for i16 {
    fn from_pcm(sample: i16) -> Self {
        sample
    }
}

impl DevicePcm for f32 {
    fn from_pcm(sample: i16) -> Self {
        f32::from(sample) / 32_768.0
    }
}

/// Samples shared between the writer and the device callback.
struct Queue {
    ring: Mutex<Ring>,
    /// Signalled on every callback and on errors, so `finish` can re-check.
    changed: Condvar,
}

impl Queue {
    fn new(rate: u32, channels: usize) -> Self {
        Self {
            ring: Mutex::new(Ring::new(rate, channels)),
            changed: Condvar::new(),
        }
    }

    fn fill<T: DevicePcm>(&self, out: &mut [T]) {
        self.ring.lock().fill(out);
        self.changed.notify_all();
    }

    fn fail(&self, message: String) {
        self.ring.lock().error = Some(message);
        self.changed.notify_all();
    }
}

/// Interleaved device-rate samples. Playback starts after a prebuffer, or at once when finishing.
struct Ring {
    samples: VecDeque<i16>,
    channels: usize,
    prebuffer: usize,
    capacity: usize,
    playing: bool,
    eof: bool,
    played_frames: u64,
    underruns: u64,
    error: Option<String>,
}

impl Ring {
    fn new(rate: u32, channels: usize) -> Self {
        let per_second = rate as usize * channels;
        Self {
            samples: VecDeque::new(),
            channels,
            prebuffer: per_second * PREBUFFER_MS / 1000,
            capacity: per_second * MAX_BACKLOG_SECONDS,
            playing: false,
            eof: false,
            played_frames: 0,
            underruns: 0,
            error: None,
        }
    }

    fn push(&mut self, samples: &[i16]) -> Result<()> {
        if let Some(error) = &self.error {
            bail!("虚拟音频设备出错：{error}");
        }
        if self.samples.len() + samples.len() > self.capacity {
            bail!(BACKLOG_MESSAGE);
        }
        self.samples.extend(samples);
        Ok(())
    }

    /// Fills one callback buffer. Silence is written for whatever is not available.
    fn fill<T: DevicePcm>(&mut self, out: &mut [T]) {
        self.playing |= self.samples.len() >= self.prebuffer || self.eof;
        let available = if self.playing {
            out.len().min(self.samples.len())
        } else {
            0
        };
        for (slot, sample) in out[..available]
            .iter_mut()
            .zip(self.samples.drain(..available))
        {
            *slot = T::from_pcm(sample);
        }
        out[available..].fill(T::from_pcm(0));
        self.played_frames += (available / self.channels) as u64;
        if self.playing && available < out.len() && !self.eof {
            self.underruns += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples(ring: &mut Ring, count: usize) -> Vec<i16> {
        let mut out = vec![0i16; count];
        ring.fill(&mut out);
        out
    }

    #[test]
    fn holds_silence_until_the_prebuffer_is_full() {
        let mut ring = Ring::new(48_000, 2);
        let prebuffer = ring.prebuffer;
        ring.push(&vec![7; prebuffer - 2]).unwrap();
        assert_eq!(samples(&mut ring, 4), vec![0; 4]);
        assert_eq!(ring.underruns, 0);

        ring.push(&[7, 7]).unwrap();
        assert_eq!(samples(&mut ring, 4), vec![7; 4]);
    }

    #[test]
    fn counts_underruns_only_while_playing_and_not_finishing() {
        let mut ring = Ring::new(48_000, 2);
        let prebuffer = ring.prebuffer;
        ring.push(&vec![1; prebuffer]).unwrap();
        samples(&mut ring, 2 * prebuffer);
        assert_eq!(ring.underruns, 1);
        assert_eq!(ring.played_frames, prebuffer as u64 / 2);
    }

    #[test]
    fn finishing_plays_a_short_tail_without_underruns() {
        let mut ring = Ring::new(48_000, 2);
        ring.push(&[3, 3, 4, 4]).unwrap();
        ring.eof = true;
        assert_eq!(samples(&mut ring, 6), vec![3, 3, 4, 4, 0, 0]);
        assert_eq!(ring.underruns, 0);
    }

    #[test]
    fn backlog_beyond_two_seconds_is_refused_without_changing_the_queue() {
        let mut ring = Ring::new(48_000, 2);
        let capacity = ring.capacity;
        ring.push(&vec![0; capacity]).unwrap();
        let error = ring.push(&[0, 0]).unwrap_err();
        assert_eq!(error.to_string(), BACKLOG_MESSAGE);
        assert_eq!(ring.samples.len(), capacity);
    }

    #[test]
    fn float_devices_get_scaled_samples() {
        assert_eq!(f32::from_pcm(16_384), 0.5);
        assert_eq!(i16::from_pcm(-5), -5);
    }
}
