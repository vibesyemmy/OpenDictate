//! Microphone capture with cpal, plus WAV writing for the speech models.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

pub struct Recording {
    stream: cpal::Stream,
    samples: Arc<Mutex<Vec<f32>>>,
    level: Arc<Mutex<f32>>,
    rate: u32,
    started: std::time::Instant,
    /// Which device and format is being recorded, for diagnostics.
    info: String,
    /// Last error the audio system reported while recording.
    error: Arc<Mutex<Option<String>>>,
    device_name: String,
    gain: f32,
    prior: Vec<f32>,
    last_check: std::time::Instant,
}

fn needs_recovery(device: &str, available: &[String], stream_error: bool) -> bool {
    stream_error || (!device.is_empty() && !available.iter().any(|name| name == device))
}

fn append_section(out: &mut Vec<f32>, samples: &[f32], rate: u32) {
    out.extend(resample(samples, rate, 16_000));
}

fn device_name(d: &cpal::Device) -> Option<String> {
    d.description().ok().map(|desc| desc.name().to_string())
}

/// Names of every microphone the system can record from.
pub fn input_devices() -> Vec<String> {
    let mut names: Vec<String> = cpal::default_host()
        .input_devices()
        .map(|it| it.filter_map(|d| device_name(&d)).collect())
        .unwrap_or_default();
    names.dedup();
    names
}

/// Name of the microphone the system currently uses by default.
pub fn default_input_name() -> Option<String> {
    cpal::default_host().default_input_device().and_then(|d| device_name(&d))
}

impl Recording {
    /// `mic` is a microphone name ("" = system default); `gain` boosts quiet microphones.
    pub fn start(mic: &str, gain: f32) -> Result<Self, String> {
        let host = cpal::default_host();
        let chosen = if mic.trim().is_empty() {
            None
        } else {
            host.input_devices()
                .ok()
                .and_then(|mut it| it.find(|d| device_name(d).as_deref() == Some(mic)))
        };
        if let Some(device) = chosen {
            if let Ok(recording) = Self::open(device, gain) {
                return Ok(recording);
            }
        }
        let device = host.default_input_device().ok_or("No microphone found. Check System Settings › Sound › Input.")?;
        Self::open(device, gain)
    }

    fn open(device: cpal::Device, gain: f32) -> Result<Self, String> {
        let gain = gain.clamp(0.1, 8.0);
        let supported = device
            .default_input_config()
            .map_err(|e| format!("Microphone unavailable: {e}"))?;
        let format = supported.sample_format();
        let config = supported.config();
        let channels = config.channels.max(1) as usize;
        let rate = config.sample_rate;
        let info = format!(
            "{} · {format:?} · {rate} Hz · {channels} ch",
            device_name(&device).unwrap_or_else(|| "unknown".into())
        );

        let samples = Arc::new(Mutex::new(Vec::<f32>::with_capacity(rate as usize * 30)));
        let level = Arc::new(Mutex::new(0.0f32));

        fn push(frames: impl Iterator<Item = f32>, gain: f32, channels: usize, out: &Mutex<Vec<f32>>, level: &Mutex<f32>) {
            let mut acc = 0.0;
            let mut n = 0;
            let mut peak = 0.0f32;
            let mut buf = out.lock().unwrap();
            for s in frames {
                acc += s;
                n += 1;
                if n == channels {
                    let v = (acc / channels as f32 * gain).clamp(-1.0, 1.0);
                    peak = peak.max(v.abs());
                    buf.push(v);
                    acc = 0.0;
                    n = 0;
                }
            }
            let mut l = level.lock().unwrap();
            *l = (*l * 0.6).max(peak);
        }

        let error = Arc::new(Mutex::new(None::<String>));
        let er = error.clone();
        let err = move |e: cpal::Error| {
            eprintln!("audio stream error: {e}");
            *er.lock().unwrap() = Some(e.to_string());
        };
        let (s, l) = (samples.clone(), level.clone());
        let stream = match format {
            cpal::SampleFormat::F32 => device.build_input_stream::<f32, _, _>(
                config,
                move |data, _| push(data.iter().copied(), gain, channels, &s, &l),
                err,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_input_stream::<i16, _, _>(
                config,
                move |data, _| push(data.iter().map(|&v| v as f32 / 32768.0), gain, channels, &s, &l),
                err,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream::<u16, _, _>(
                config,
                move |data, _| {
                    push(data.iter().map(|&v| (v as f32 - 32768.0) / 32768.0), gain, channels, &s, &l)
                },
                err,
                None,
            ),
            cpal::SampleFormat::I32 => device.build_input_stream::<i32, _, _>(
                config,
                move |data, _| {
                    push(data.iter().map(|&v| v as f32 / 2147483648.0), gain, channels, &s, &l)
                },
                err,
                None,
            ),
            other => return Err(format!("Unsupported microphone format {other:?}")),
        }
        .map_err(|e| format!("Could not open the microphone: {e}"))?;
        stream.play().map_err(|e| format!("Could not start the microphone: {e}"))?;

        Ok(Self { stream, samples, level, rate, started: std::time::Instant::now(), info, error,
            device_name: device_name(&device).unwrap_or_default(), gain, prior: Vec::new(), last_check: std::time::Instant::now() })
    }

    /// Preserve the old section and continue on the current system input.
    pub fn recover_if_needed(&mut self) -> Option<Result<String, String>> {
        if self.last_check.elapsed() < std::time::Duration::from_secs(1) { return None; }
        self.last_check = std::time::Instant::now();
        if !needs_recovery(&self.device_name, &input_devices(), self.error().is_some()) { return None; }
        let host = cpal::default_host();
        let device = match host.default_input_device() {
            Some(d) => d,
            None => return Some(Err("Microphone disconnected; no system microphone is available.".into())),
        };
        let mut next = match Self::open(device, self.gain) {
            Ok(r) => r,
            Err(e) => return Some(Err(format!("Microphone disconnected; could not switch to the system input: {e}"))),
        };
        let old = std::mem::replace(self, next);
        let started = old.started;
        let mut prior = old.prior;
        drop(old.stream);
        append_section(&mut prior, &old.samples.lock().unwrap(), old.rate);
        self.prior = prior;
        self.started = started;
        Some(Ok(self.device_name.clone()))
    }

    pub fn info(&self) -> String {
        self.info.clone()
    }

    pub fn error(&self) -> Option<String> {
        self.error.lock().unwrap().clone()
    }

    /// How many samples have arrived so far (0 means the system sends nothing).
    pub fn sample_count(&self) -> usize {
        self.samples.lock().unwrap().len()
    }

    /// Loudest sample so far.
    pub fn peak(&self) -> f32 {
        self.samples.lock().unwrap().iter().fold(0.0f32, |m, v| m.max(v.abs()))
    }

    pub fn level(&self) -> f32 {
        *self.level.lock().unwrap()
    }

    pub fn elapsed(&self) -> f32 {
        self.started.elapsed().as_secs_f32()
    }

    /// Stops the microphone and returns 16 kHz mono samples.
    pub fn finish(self) -> Clip {
        drop(self.stream);
        let raw = std::mem::take(&mut *self.samples.lock().unwrap());
        let mut samples = self.prior;
        append_section(&mut samples, &raw, self.rate);
        Clip { samples }
    }
}

pub struct Clip {
    pub samples: Vec<f32>,
}

impl Clip {
    pub fn seconds(&self) -> f32 {
        self.samples.len() as f32 / 16_000.0
    }

    /// True when the clip is too short or too quiet to hold speech.
    pub fn is_silent(&self) -> bool {
        if self.seconds() < 0.3 {
            return true;
        }
        let peak = self.samples.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        if peak < 0.01 { return true; }
        // A single click can exceed the peak threshold even in an otherwise
        // quiet recording. Require a short run of sustained sound as well.
        let mut active = 0;
        for frame in self.samples.chunks_exact(320) {
            let rms = (frame.iter().map(|v| v * v).sum::<f32>() / frame.len() as f32).sqrt();
            active = if rms >= 0.003 { active + 1 } else { 0 };
            if active >= 3 {
                // Reuse the meeting cutter's adaptive noise floor so steady
                // fan noise is not mistaken for a voice after a click.
                let mut detector = crate::cut::Cutter::new(16_000.0);
                for chunk in self.samples.chunks(16_000) {
                    if !detector.push(chunk, &[], false).is_empty() { return false; }
                }
                return detector.flush().is_none();
            }
        }
        true
    }

    /// 16-bit PCM mono WAV at 16 kHz, the format whisper.cpp expects.
    pub fn to_wav(&self) -> Vec<u8> {
        let data_len = (self.samples.len() * 2) as u32;
        let mut out = Vec::with_capacity(44 + data_len as usize);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // mono
        out.extend_from_slice(&16_000u32.to_le_bytes());
        out.extend_from_slice(&32_000u32.to_le_bytes()); // byte rate
        out.extend_from_slice(&2u16.to_le_bytes()); // block align
        out.extend_from_slice(&16u16.to_le_bytes()); // bits
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for s in &self.samples {
            let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }
}

fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let len = (input.len() as f64 / ratio) as usize;
    (0..len)
        .map(|i| {
            // Average the source samples that fall into this output slot (cheap low-pass).
            let start = i as f64 * ratio;
            let end = ((i + 1) as f64 * ratio).min(input.len() as f64);
            let (a, b) = (start as usize, (end as usize).max(start as usize + 1).min(input.len()));
            let slice = &input[a..b];
            slice.iter().sum::<f32>() / slice.len() as f32
        })
        .collect()
}

#[cfg(test)]
mod recovery_tests {
    use super::*;

    #[test]
    fn joins_recorded_sections_with_different_sample_rates_in_order() {
        let mut samples = Vec::new();
        append_section(&mut samples, &[0.5; 480], 48_000);
        append_section(&mut samples, &[-0.5; 160], 16_000);
        assert_eq!(samples.len(), 320);
        assert!(samples[..160].iter().all(|x| *x == 0.5));
        assert!(samples[160..].iter().all(|x| *x == -0.5));
    }

    #[test]
    fn recovery_only_triggers_for_missing_device_or_stream_error() {
        assert!(!needs_recovery("External", &["External".into()], false));
        assert!(needs_recovery("External", &["Built-in".into()], false));
        assert!(needs_recovery("External", &["External".into()], true));
        assert!(!needs_recovery("", &[], false));
    }
}
