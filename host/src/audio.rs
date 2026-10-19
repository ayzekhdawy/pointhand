//! Loopback capture of what Windows is playing.
//!
//! The mix is converted to a 24 kHz stereo 16-bit WAV. Capture runs on its own
//! thread, so pointer and keyboard actions continue. It is not microphone
//! input and it does not wait for the pointer to be idle. Physical Escape
//! ends the capture; `stop` still returns the samples gathered so far.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{json, Value};
use windows::core::GUID;
use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
    WAVEFORMATEX, WAVEFORMATEXTENSIBLE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
    COINIT_MULTITHREADED,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
};

const SAMPLE_RATE: u32 = 24_000;
const DEFAULT_DURATION_MS: u64 = 60_000;
const MAX_DURATION_MS: u64 = 300_000;
const MAX_FRAMES: usize = (MAX_DURATION_MS as usize) * (SAMPLE_RATE as usize) / 1000;
const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
const PCM_SUBTYPE: GUID = GUID::from_u128(0x0000_0001_0000_0010_8000_00aa_0038_9b71);
const FLOAT_SUBTYPE: GUID = GUID::from_u128(0x0000_0003_0000_0010_8000_00aa_0038_9b71);

static BUSY: AtomicBool = AtomicBool::new(false);
static FILE_SEQ: AtomicU64 = AtomicU64::new(0);
static LIVE: Mutex<Option<Live>> = Mutex::new(None);

struct Live {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<Result<Vec<i16>, String>>,
}

struct MixSpec {
    channels: u16,
    rate: u32,
    bits: u16,
    align: u16,
    sample_bytes: usize,
    ieee_float: bool,
}

pub fn start(params: &Value) -> Result<Value, String> {
    if crate::STOPPED.load(Ordering::Relaxed) {
        return Err("stopped by Escape or emergency stop; restart PointHand to resume.".into());
    }
    let max_duration_ms = recording_limit(params.get("max_duration_ms").and_then(|v| v.as_f64()))?;
    if BUSY
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("audio recording is already active; call stop_audio_recording".into());
    }
    let stop = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = mpsc::channel();
    let stop_thread = Arc::clone(&stop);
    let thread = thread::spawn(move || capture(stop_thread, max_duration_ms, ready_tx));
    {
        let mut slot = lock();
        if slot.is_some() {
            stop.store(true, Ordering::Relaxed);
            drop(slot);
            let _ = join_pumping(thread);
            BUSY.store(false, Ordering::Release);
            return Err("audio recording is already active; call stop_audio_recording".into());
        }
        *slot = Some(Live {
            stop: Arc::clone(&stop),
            thread,
        });
    }
    // UI Automation owns an STA on this thread. WASAPI activation can marshal
    // back here, so the wait has to keep dispatching window messages.
    match wait_ready(&ready_rx) {
        Ok(()) => {
            if lock().is_some() {
                Ok(json!({
                    "recording": true,
                    "max_duration_ms": max_duration_ms,
                    "sample_rate": SAMPLE_RATE,
                    "channels": 2,
                }))
            } else {
                Err("audio recording ended before it could start".into())
            }
        }
        Err(error) => {
            stop.store(true, Ordering::Relaxed);
            release_failed_start();
            Err(error)
        }
    }
}

pub fn stop() -> Result<Value, String> {
    let Some(live) = take_live(true) else {
        return Err("no active audio recording".into());
    };
    live.stop.store(true, Ordering::Relaxed);
    let joined = join_pumping(live.thread);
    BUSY.store(false, Ordering::Release);
    match joined {
        Ok(Ok(pcm)) => audio_value(&pcm),
        Ok(Err(error)) => Err(error),
        Err(_) => Err("audio recording thread panicked".into()),
    }
}

/// Drop an in-flight recording when the host process is leaving.
pub fn discard() {
    let Some(live) = take_live(true) else {
        return;
    };
    live.stop.store(true, Ordering::Relaxed);
    let _ = join_pumping(live.thread);
    BUSY.store(false, Ordering::Release);
}

fn release_failed_start() {
    if let Some(live) = lock().take() {
        live.stop.store(true, Ordering::Relaxed);
        let _ = join_pumping(live.thread);
    }
    BUSY.store(false, Ordering::Release);
}

fn wait_ready(ready: &mpsc::Receiver<Result<(), String>>) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match ready.try_recv() {
            Ok(result) => return result,
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err("audio recording stopped before it started".into());
            }
            Err(mpsc::TryRecvError::Empty) if Instant::now() >= deadline => {
                return Err("audio output did not start within 5 seconds".into());
            }
            Err(mpsc::TryRecvError::Empty) => {
                pump_messages();
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

fn join_pumping(
    thread: JoinHandle<Result<Vec<i16>, String>>,
) -> thread::Result<Result<Vec<i16>, String>> {
    while !thread.is_finished() {
        pump_messages();
        thread::sleep(Duration::from_millis(10));
    }
    thread.join()
}

fn pump_messages() {
    unsafe {
        let mut message = MSG::default();
        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

fn take_live(wait: bool) -> Option<Live> {
    if !BUSY.load(Ordering::Acquire) {
        return None;
    }
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        if let Some(live) = lock().take() {
            return Some(live);
        }
        if !wait || !BUSY.load(Ordering::Acquire) || Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn lock() -> std::sync::MutexGuard<'static, Option<Live>> {
    LIVE.lock().unwrap_or_else(|poison| poison.into_inner())
}

fn cover_elapsed(pcm: &mut Vec<i16>, elapsed_ms: u64, max_duration_ms: u64) {
    let ms = elapsed_ms.min(max_duration_ms);
    let want = (ms as usize).saturating_mul(SAMPLE_RATE as usize) / 1000;
    let want = want.min(MAX_FRAMES);
    if pcm.len() / 2 < want {
        pcm.resize(want * 2, 0);
    }
}

fn recording_limit(value: Option<f64>) -> Result<u64, String> {
    let Some(ms) = value else {
        return Ok(DEFAULT_DURATION_MS);
    };
    if !ms.is_finite() || ms <= 0.0 {
        return Err("max_duration_ms must be a positive number".into());
    }
    Ok((ms as u64).min(MAX_DURATION_MS))
}

fn capture(
    stop: Arc<AtomicBool>,
    max_duration_ms: u64,
    ready: mpsc::Sender<Result<(), String>>,
) -> Result<Vec<i16>, String> {
    let mut com_owned = false;
    let outcome = unsafe { record(&stop, max_duration_ms, &ready, &mut com_owned) };
    if com_owned {
        unsafe { CoUninitialize() };
    }
    outcome
}

unsafe fn record(
    stop: &AtomicBool,
    max_duration_ms: u64,
    ready: &mpsc::Sender<Result<(), String>>,
    com_owned: &mut bool,
) -> Result<Vec<i16>, String> {
    let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
    if hr.is_err() && hr != RPC_E_CHANGED_MODE {
        let error = format!("audio COM init failed: {hr}");
        let _ = ready.send(Err(error.clone()));
        return Err(error);
    }
    *com_owned = hr.is_ok();
    let opened = open_loopback();
    if let Err(error) = opened {
        let _ = ready.send(Err(error.clone()));
        return Err(error);
    }
    let (client, capture_client, spec) = opened.unwrap();
    let _ = ready.send(Ok(()));
    let mut resampler = Resampler::new(spec.rate, SAMPLE_RATE);
    let mut pcm = Vec::new();
    let started = Instant::now();
    let deadline = Instant::now() + Duration::from_millis(max_duration_ms);
    let mut device_error = None;
    while !stop.load(Ordering::Relaxed)
        && !crate::STOPPED.load(Ordering::Relaxed)
        && Instant::now() < deadline
        && pcm.len() / 2 < MAX_FRAMES
    {
        let packet = match capture_client.GetNextPacketSize() {
            Ok(packet) => packet,
            Err(error) => {
                device_error = Some(format!("audio packet size: {error}"));
                break;
            }
        };
        if packet == 0 {
            thread::sleep(Duration::from_millis(10));
            continue;
        }
        match take_packet(&capture_client, &spec) {
            Ok(frames) => pcm.extend(resampler.push(&frames)),
            Err(error) => {
                device_error = Some(error);
                break;
            }
        }
    }
    let _ = client.Stop();
    pcm.extend(resampler.finish());
    // A quiet engine delivers no packets. Keep the WAV as long as the capture
    // actually ran, filled with silence where nothing was playing.
    if device_error.is_none() {
        cover_elapsed(
            &mut pcm,
            started.elapsed().as_millis() as u64,
            max_duration_ms,
        );
    }
    if pcm.len() / 2 > MAX_FRAMES {
        pcm.truncate(MAX_FRAMES * 2);
    }
    match device_error {
        Some(error) if pcm.is_empty() => Err(error),
        _ => Ok(pcm),
    }
}

unsafe fn open_loopback() -> Result<(IAudioClient, IAudioCaptureClient, MixSpec), String> {
    let enumerator: IMMDeviceEnumerator =
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
            .map_err(|error| format!("audio device enumerator: {error}"))?;
    let device = enumerator
        .GetDefaultAudioEndpoint(eRender, eConsole)
        .map_err(|error| format!("no default audio output: {error}"))?;
    let client: IAudioClient = device
        .Activate(CLSCTX_ALL, None)
        .map_err(|error| format!("audio client: {error}"))?;
    let mix = client
        .GetMixFormat()
        .map_err(|error| format!("audio mix format: {error}"))?;
    let opened: Result<MixSpec, String> = (|| {
        let spec = spec_from_format(mix)?;
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK,
                10_000_000,
                0,
                mix,
                None,
            )
            .map_err(|error| format!("audio loopback init: {error}"))?;
        Ok(spec)
    })();
    CoTaskMemFree(Some(mix.cast()));
    let spec = opened?;
    let capture_client: IAudioCaptureClient = client
        .GetService()
        .map_err(|error| format!("audio capture client: {error}"))?;
    client
        .Start()
        .map_err(|error| format!("audio capture start: {error}"))?;
    Ok((client, capture_client, spec))
}

unsafe fn take_packet(capture: &IAudioCaptureClient, spec: &MixSpec) -> Result<Vec<f32>, String> {
    let mut data = std::ptr::null_mut();
    let mut frames = 0u32;
    let mut flags = 0u32;
    capture
        .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
        .map_err(|error| format!("audio buffer: {error}"))?;
    let decoded = if frames == 0 {
        Ok(Vec::new())
    } else if flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 || data.is_null() {
        Ok(vec![0.0; frames as usize * 2])
    } else {
        let byte_len = frames as usize * spec.align as usize;
        let bytes = std::slice::from_raw_parts(data, byte_len);
        decode_packet(bytes, spec, frames as usize)
    };
    let released = capture
        .ReleaseBuffer(frames)
        .map_err(|error| format!("audio buffer release: {error}"));
    let frames = decoded?;
    released?;
    Ok(frames)
}

fn spec_from_format(format: *const WAVEFORMATEX) -> Result<MixSpec, String> {
    if format.is_null() {
        return Err("audio device returned no mix format".into());
    }
    let header = unsafe { std::ptr::read_unaligned(format) };
    if header.nChannels == 0 || header.nSamplesPerSec == 0 || header.nBlockAlign == 0 {
        return Err("audio device mix format is empty".into());
    }
    let (pcm, ieee_float) = match header.wFormatTag {
        WAVE_FORMAT_PCM => (true, false),
        WAVE_FORMAT_IEEE_FLOAT => (false, true),
        WAVE_FORMAT_EXTENSIBLE => {
            if header.cbSize < 22 {
                return Err("extensible audio format is incomplete".into());
            }
            let subformat = unsafe {
                std::ptr::read_unaligned(std::ptr::addr_of!(
                    (*format.cast::<WAVEFORMATEXTENSIBLE>()).SubFormat
                ))
            };
            (subformat == PCM_SUBTYPE, subformat == FLOAT_SUBTYPE)
        }
        other => {
            return Err(format!("unsupported audio mix format tag {other}"));
        }
    };
    if !pcm && !ieee_float {
        return Err("unsupported extensible audio subtype".into());
    }
    let sample_bytes = if ieee_float {
        match header.wBitsPerSample {
            32 => 4,
            64 => 8,
            other => return Err(format!("unsupported float width {other}")),
        }
    } else {
        match header.wBitsPerSample {
            8 | 16 | 24 | 32 => header.wBitsPerSample as usize / 8,
            other => return Err(format!("unsupported PCM width {other}")),
        }
    };
    if header.nBlockAlign as usize > 65_536
        || (header.nBlockAlign as usize) < header.nChannels as usize * sample_bytes
    {
        return Err("audio mix format block size does not match its channels".into());
    }
    Ok(MixSpec {
        channels: header.nChannels,
        rate: header.nSamplesPerSec,
        bits: header.wBitsPerSample,
        align: header.nBlockAlign,
        sample_bytes,
        ieee_float,
    })
}

fn decode_packet(bytes: &[u8], spec: &MixSpec, frames: usize) -> Result<Vec<f32>, String> {
    let need = frames * spec.align as usize;
    if bytes.len() < need {
        return Err("short audio packet".into());
    }
    let mut stereo = Vec::with_capacity(frames * 2);
    for frame in 0..frames {
        let base = frame * spec.align as usize;
        let mut channels = Vec::with_capacity(spec.channels as usize);
        for channel in 0..spec.channels as usize {
            let at = base + channel * spec.sample_bytes;
            channels.push(read_sample(&bytes[at..], spec)?);
        }
        let [left, right] = downmix(&channels);
        stereo.push(left);
        stereo.push(right);
    }
    Ok(stereo)
}

fn read_sample(bytes: &[u8], spec: &MixSpec) -> Result<f32, String> {
    if bytes.len() < spec.sample_bytes {
        return Err("short audio sample".into());
    }
    if spec.ieee_float {
        return Ok(match spec.sample_bytes {
            4 => f32::from_le_bytes(bytes[..4].try_into().unwrap()),
            8 => f64::from_le_bytes(bytes[..8].try_into().unwrap()) as f32,
            _ => return Err("unsupported float width".into()),
        });
    }
    let sample = match spec.bits {
        8 => (bytes[0] as f32 - 128.0) / 128.0,
        16 => i16::from_le_bytes(bytes[..2].try_into().unwrap()) as f32 / 32768.0,
        24 => read_i24(bytes) as f32 / 8_388_608.0,
        32 => i32::from_le_bytes(bytes[..4].try_into().unwrap()) as f32 / 2_147_483_648.0,
        _ => return Err("unsupported PCM width".into()),
    };
    Ok(sample)
}

fn read_i24(bytes: &[u8]) -> i32 {
    let value = bytes[0] as i32 | ((bytes[1] as i32) << 8) | ((bytes[2] as i32) << 16);
    if value & 0x80_0000 != 0 {
        value | !0xFF_FFFF
    } else {
        value
    }
}

fn downmix(channels: &[f32]) -> [f32; 2] {
    match channels {
        [] => [0.0, 0.0],
        [only] => [*only, *only],
        [left, right, ..] => [*left, *right],
    }
}

struct Resampler {
    step: f64,
    pos: f64,
    pending: Vec<f32>,
}

impl Resampler {
    fn new(src_rate: u32, dst_rate: u32) -> Self {
        let step = if dst_rate == 0 {
            0.0
        } else {
            src_rate as f64 / dst_rate as f64
        };
        Self {
            step,
            pos: 0.0,
            pending: Vec::new(),
        }
    }

    fn push(&mut self, stereo: &[f32]) -> Vec<i16> {
        self.pending.extend_from_slice(stereo);
        self.emit(false)
    }

    fn finish(mut self) -> Vec<i16> {
        self.emit(true)
    }

    fn emit(&mut self, flush: bool) -> Vec<i16> {
        if !(self.step.is_finite() && self.step > 0.0) {
            return Vec::new();
        }
        let mut out = Vec::new();
        loop {
            let frames = self.pending.len() / 2;
            if frames == 0 {
                break;
            }
            let index = self.pos.floor() as usize;
            if index >= frames {
                break;
            }
            let next = index + 1;
            if next >= frames && !flush {
                break;
            }
            let frac = (self.pos - index as f64) as f32;
            for channel in 0..2 {
                let current = self.pending[index * 2 + channel];
                let following = if next < frames {
                    self.pending[next * 2 + channel]
                } else {
                    current
                };
                out.push(to_i16(current + (following - current) * frac));
            }
            self.pos += self.step;
            let drop = (self.pos.floor() as usize).min(frames);
            if drop > 0 {
                self.pending.drain(..drop * 2);
                self.pos -= drop as f64;
            }
        }
        out
    }
}

fn to_i16(sample: f32) -> i16 {
    if !sample.is_finite() {
        return 0;
    }
    (sample.clamp(-1.0, 1.0) * 32_767.0).round() as i16
}

fn wav_bytes(pcm: &[i16]) -> Vec<u8> {
    let data_bytes = pcm.len() * 2;
    let mut out = Vec::with_capacity(44 + data_bytes);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_bytes as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&WAVE_FORMAT_PCM.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 4).to_le_bytes());
    out.extend_from_slice(&4u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data_bytes as u32).to_le_bytes());
    for sample in pcm {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

fn audio_value(pcm: &[i16]) -> Result<Value, String> {
    let wav = wav_bytes(pcm);
    let seq = FILE_SEQ.fetch_add(1, Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("pointhand-audio-{}-{seq}.wav", std::process::id()));
    std::fs::write(&path, &wav)
        .map_err(|error| format!("could not write the recording: {error}"))?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&wav);
    let frames = pcm.len() / 2;
    Ok(json!({
        "filepath": path.display().to_string(),
        "bytes": encoded,
        "data_url": format!("data:audio/wav;base64,{encoded}"),
        "duration_ms": frames as u64 * 1000 / u64::from(SAMPLE_RATE),
        "sample_rate": SAMPLE_RATE,
        "channels": 2,
        "samples": frames,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm16(channels: u16, rate: u32) -> MixSpec {
        MixSpec {
            channels,
            rate,
            bits: 16,
            align: channels * 2,
            sample_bytes: 2,
            ieee_float: false,
        }
    }

    #[test]
    fn duration_uses_sixty_seconds_and_caps_at_five_minutes() {
        assert_eq!(recording_limit(None).unwrap(), 60_000);
        assert_eq!(recording_limit(Some(10.0)).unwrap(), 10);
        assert_eq!(recording_limit(Some(500_000.0)).unwrap(), 300_000);
        assert!(recording_limit(Some(0.0)).is_err());
        assert!(recording_limit(Some(-1.0)).is_err());
        assert!(recording_limit(Some(f64::NAN)).is_err());
    }

    #[test]
    fn quiet_capture_keeps_the_elapsed_duration() {
        let mut quiet = Vec::new();
        cover_elapsed(&mut quiet, 1_000, 60_000);
        assert_eq!(quiet.len(), 24_000 * 2);
        assert!(quiet.iter().all(|sample| *sample == 0));

        let mut already = vec![2_i16; 24_000 * 2 + 10];
        cover_elapsed(&mut already, 1_000, 60_000);
        assert_eq!(already.len(), 24_000 * 2 + 10);

        let mut capped = vec![1_i16; 4];
        cover_elapsed(&mut capped, 500_000, 1_000);
        assert_eq!(capped.len() / 2, 24_000);
        assert_eq!(&capped[..4], &[1, 1, 1, 1]);
    }

    #[test]
    fn pcm_float_and_mono_become_stereo_frames() {
        let stereo = decode_packet(&[0, 0, 0x00, 0x80], &pcm16(2, 24_000), 1).unwrap();
        assert_eq!(stereo, vec![0.0, -1.0]);
        let mono = decode_packet(&[0, 0x40], &pcm16(1, 24_000), 1).unwrap();
        assert_eq!(mono, vec![0.5, 0.5]);
        let mut spec = pcm16(1, 48_000);
        spec.bits = 32;
        spec.align = 4;
        spec.sample_bytes = 4;
        spec.ieee_float = true;
        let float = decode_packet(&1.5f32.to_le_bytes(), &spec, 1).unwrap();
        assert_eq!(float, vec![1.5, 1.5]);
        let mut wide = pcm16(2, 48_000);
        wide.bits = 24;
        wide.align = 6;
        wide.sample_bytes = 3;
        let packed = decode_packet(&[0, 0, 0, 0xFF, 0xFF, 0x7F], &wide, 1).unwrap();
        assert!(packed[0].abs() < 0.0001);
        assert!((packed[1] - 1.0).abs() < 0.0001);
    }

    #[test]
    fn extensible_float_subtype_is_recognized() {
        let mut format = WAVEFORMATEXTENSIBLE::default();
        format.Format.wFormatTag = WAVE_FORMAT_EXTENSIBLE;
        format.Format.nChannels = 2;
        format.Format.nSamplesPerSec = 48_000;
        format.Format.wBitsPerSample = 32;
        format.Format.nBlockAlign = 8;
        format.Format.cbSize = 22;
        format.SubFormat = FLOAT_SUBTYPE;
        let spec = spec_from_format(&format as *const WAVEFORMATEXTENSIBLE as *const WAVEFORMATEX)
            .unwrap();
        assert!(spec.ieee_float);
        assert_eq!(spec.rate, 48_000);
        format.SubFormat = GUID::from_u128(0);
        assert!(
            spec_from_format(&format as *const WAVEFORMATEXTENSIBLE as *const WAVEFORMATEX)
                .is_err()
        );
    }

    #[test]
    fn resample_keeps_a_constant_signal_and_matching_rates() {
        let mut half = Resampler::new(48_000, 24_000);
        let source = vec![0.5f32, -0.25, 0.5, -0.25, 0.5, -0.25, 0.5, -0.25];
        let mut pcm = half.push(&source);
        pcm.extend(half.finish());
        assert_eq!(pcm.len() % 2, 0);
        assert!(pcm.len() >= 2);
        assert!(pcm
            .iter()
            .step_by(2)
            .all(|sample| (sample - 16_384).abs() <= 1));
        assert!(pcm
            .iter()
            .skip(1)
            .step_by(2)
            .all(|sample| (sample + 8_192).abs() <= 1));

        let mut same = Resampler::new(24_000, 24_000);
        let mut identity = same.push(&[0.0, 0.0, 1.0, -1.0]);
        identity.extend(same.finish());
        assert_eq!(identity, vec![0, 0, 32_767, -32_767]);
    }

    #[test]
    fn wav_header_is_24khz_stereo_pcm() {
        let wav = wav_bytes(&[0, 32_767, -32_767, 1]);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes([wav[20], wav[21]]), 1);
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 2);
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 24_000);
        assert_eq!(u16::from_le_bytes([wav[34], wav[35]]), 16);
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(wav.len(), 44 + 8);
        assert_eq!(i16::from_le_bytes([wav[44], wav[45]]), 0);
        assert_eq!(i16::from_le_bytes([wav[46], wav[47]]), 32_767);
    }
}
