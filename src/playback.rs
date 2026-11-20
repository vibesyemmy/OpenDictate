#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_only_timestamped_audio_in_chronological_order() {
        let dir = std::env::temp_dir().join(format!("open-dictate-playback-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["000005000.wav", "000001000.wav", "notes.txt", "bad.wav"] {
            std::fs::write(dir.join(name), []).unwrap();
        }
        let clips = clips_in(&dir);
        assert_eq!(clips.iter().map(|c| c.at_ms).collect::<Vec<_>>(), [1000, 5000]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn gap_preserves_meeting_timing_without_underflow() {
        assert_eq!(gap_ms(5000, 3000), 2000);
        assert_eq!(gap_ms(1000, 3000), 0);
    }

    #[test]
    fn playback_timeline_keeps_leading_offset_and_missing_gaps() {
        let ranges = vec![(5_000, 7_000), (10_000, 12_000)];
        assert_eq!(playback_to_meeting_ms(5_000, 0), 5_000);
        assert_eq!(playback_to_meeting_ms(5_000, 2_500), 7_500);
        assert_eq!(meeting_to_playback_ms(5_000, 4_000), 0);
        assert!(time_has_audio(&ranges, 6_000));
        assert!(!time_has_audio(&ranges, 8_000));
        assert!(time_has_audio(&ranges, 11_000));
    }

    #[test]
    fn transcript_highlight_requires_matching_saved_audio() {
        let segs = vec![crate::meeting::Segment { at_ms: 5_000, secs: 2.0, text: "Hello".into(), voice: "you".into() }, crate::meeting::Segment { at_ms: 8_000, secs: 2.0, text: "Next".into(), voice: "them".into() }];
        assert_eq!(active_segment(&segs, &[(5_000, 7_000)], 6_000), Some(0));
        assert_eq!(active_segment(&segs, &[(5_000, 7_000)], 8_500), None);
        assert_eq!(active_segment(&segs, &[(5_000, 7_000)], 7_500), None);
        assert_eq!(active_segment(&segs, &[(5_000, 10_000)], 8_500), None, "an overlapping clip must not imply this passage was saved");
        assert_eq!(active_segment(&segs, &[(5_000, 10_000), (8_000, 10_000)], 8_500), None, "earlier audio takes precedence when clips overlap");
    }

    #[test]
    fn saved_audio_ranges_preserve_missing_sections() {
        let dir = std::env::temp_dir().join(format!("open-dictate-ranges-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let clips: Vec<Clip> = [1_000, 4_000].into_iter().enumerate().map(|(i, at_ms)| {
            let path = dir.join(format!("{i}.wav"));
            let mut wav = wav_header(32_000).to_vec(); wav.resize(44 + 32_000, 0);
            std::fs::write(&path, wav).unwrap();
            Clip { at_ms, path }
        }).collect();
        assert_eq!(clip_ranges(&clips).unwrap(), [(1_000, 2_000), (4_000, 5_000)]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "uses macOS audio output"]
    fn player_seeks_pauses_and_reports_meeting_time() {
        let path = std::env::temp_dir().join(format!("open-dictate-player-state-{}.wav", std::process::id()));
        let mut wav = wav_header(64_000).to_vec(); wav.resize(44 + 64_000, 0);
        std::fs::write(&path, wav).unwrap();
        let mut player = Player::start_at(vec![Clip { at_ms: 5_000, path: path.clone() }], 5_500).unwrap();
        for _ in 0..100 {
            player.poll().unwrap();
            if player.ready() { break; }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(player.ready());
        assert!((5_500..6_000).contains(&player.position_ms()));
        let segments = [crate::meeting::Segment { at_ms: 5_000, secs: 2.0, text: "Example passage".into(), voice: "you".into() }];
        assert_eq!(active_segment(&segments, &player.ranges, player.position_ms()), Some(0));
        player.pause();
        assert!(!player.playing());
        player.seek(6_000);
        assert!((5_990..6_100).contains(&player.position_ms()));
        player.resume().unwrap();
        assert!(player.playing());
        drop(player);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn assembles_clips_with_silence_for_missing_time() {
        let dir = std::env::temp_dir().join(format!("open-dictate-assemble-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = dir.join("a.wav");
        let second = dir.join("b.wav");
        let output = dir.join("out.wav");
        let mut a = wav_header(4).to_vec(); a.extend_from_slice(&[1, 2, 3, 4]);
        let mut b = wav_header(2).to_vec(); b.extend_from_slice(&[5, 6]);
        std::fs::write(&first, a).unwrap();
        std::fs::write(&second, b).unwrap();
        let clips = [Clip { at_ms: 1000, path: first }, Clip { at_ms: 1001, path: second }];
        assemble(&clips, &output, &AtomicBool::new(false)).unwrap();
        let data = std::fs::read(&output).unwrap();
        assert_eq!(data.len(), 44 + 34);
        assert_eq!(&data[44..48], &[1, 2, 3, 4]);
        assert!(data[48..76].iter().all(|x| *x == 0));
        assert_eq!(&data[76..], &[5, 6]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    #[ignore = "requires OPENDICTATE_AUDIO_DIR containing saved meeting clips"]
    fn assembles_a_real_saved_meeting() {
        let clips = clips_in(Path::new(&std::env::var("OPENDICTATE_AUDIO_DIR").unwrap()));
        assert!(!clips.is_empty());
        let path = std::env::temp_dir().join(format!("open-dictate-real-playback-{}.wav", std::process::id()));
        let result = assemble(&clips, &path, &AtomicBool::new(false));
        let len = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let _ = std::fs::remove_file(path);
        assert!(result.is_ok(), "{result:?}");
        assert!(len > 44);
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "uses macOS audio output"]
    fn plays_a_silent_saved_clip() {
        let path = std::env::temp_dir().join(format!("open-dictate-playback-silent-{}.wav", std::process::id()));
        let frames = 32_000u32;
        let size = frames * 2;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + size).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&16_000u32.to_le_bytes());
        wav.extend_from_slice(&32_000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&size.to_le_bytes());
        wav.resize(44 + size as usize, 0);
        std::fs::write(&path, wav).unwrap();
        let result = prepare_clips(&[Clip { at_ms: 0, path: path.clone() }], &AtomicBool::new(false))
            .and_then(|p| NativePlayer::open(&p.path).and_then(|player| {
                player.seek(0.5);
                assert!((player.time() - 0.5).abs() < 0.1);
                player.play()?;
                std::thread::sleep(std::time::Duration::from_millis(150));
                player.pause();
                let time = player.time();
                assert!(time >= 0.5 && time < 2.0, "position was {time}");
                assert!(!player.playing());
                Ok(())
            }));
        let _ = std::fs::remove_file(path);
        assert!(result.is_ok(), "{result:?}");
    }
}
use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc};
use std::io::{Read, Seek, SeekFrom, Write};

#[derive(Clone, Debug)]
pub struct Clip {
    pub at_ms: u64,
    pub path: PathBuf,
}

pub fn playback_to_meeting_ms(first_clip_ms: u64, playback_ms: u64) -> u64 { first_clip_ms.saturating_add(playback_ms) }
pub fn meeting_to_playback_ms(first_clip_ms: u64, meeting_ms: u64) -> u64 { meeting_ms.saturating_sub(first_clip_ms) }
pub fn time_has_audio(ranges: &[(u64, u64)], at_ms: u64) -> bool {
    ranges.iter().any(|&(start, end)| start <= at_ms && at_ms < end)
}
pub fn active_segment(segments: &[crate::meeting::Segment], ranges: &[(u64, u64)], at_ms: u64) -> Option<usize> {
    let &(clip_start, _) = ranges.iter().find(|&&(start, end)| start <= at_ms && at_ms < end)?;
    segments.iter().position(|segment| {
        segment.at_ms <= at_ms && at_ms < segment.at_ms.saturating_add((segment.secs.max(0.0) * 1000.0) as u64)
            && segment.at_ms == clip_start
    })
}

pub fn clip_ranges(clips: &[Clip]) -> Result<Vec<(u64, u64)>, String> {
    let mut ranges: Vec<(u64, u64)> = Vec::new();
    for clip in clips {
        let mut file = std::fs::File::open(&clip.path).map_err(|e| format!("Could not read a saved clip: {e}"))?;
        let mut header = [0u8; 44];
        file.read_exact(&mut header).map_err(|e| format!("Invalid saved audio: {e}"))?;
        if header[0..4] != *b"RIFF" || header[8..40] != wav_header(0)[8..40] { return Err("A saved audio clip has an unsupported format.".into()); }
        let len = u32::from_le_bytes(header[40..44].try_into().unwrap()) as u64;
        if len % 2 != 0 || file.metadata().map_err(|e| e.to_string())?.len() < 44 + len { return Err("A saved audio clip is incomplete.".into()); }
        let end = clip.at_ms.saturating_add(len / 32);
        ranges.push((clip.at_ms, end));
    }
    Ok(ranges)
}

fn clips_in(dir: &Path) -> Vec<Clip> {
    let mut clips: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten().filter_map(|entry| {
        let path = entry.path();
        if !entry.file_type().ok()?.is_file() || path.extension()?.to_str()? != "wav" { return None; }
        let name = path.file_stem()?.to_str()?;
        if name.is_empty() || !name.bytes().all(|c| c.is_ascii_digit()) { return None; }
        Some(Clip { at_ms: name.parse().ok()?, path })
    }).collect();
    clips.sort_by_key(|c| c.at_ms);
    clips
}

pub fn clips(id: &str) -> Vec<Clip> {
    if id.is_empty() || id.contains('/') || id.contains("..") { return Vec::new(); }
    clips_in(&crate::meeting::root().join(id).join("audio"))
}

fn gap_ms(start: u64, elapsed: u64) -> u64 { start.saturating_sub(elapsed) }

fn wav_header(data_len: u32) -> [u8; 44] {
    let mut out = [0; 44];
    out[0..4].copy_from_slice(b"RIFF");
    out[4..8].copy_from_slice(&(36 + data_len).to_le_bytes());
    out[8..16].copy_from_slice(b"WAVEfmt ");
    out[16..20].copy_from_slice(&16u32.to_le_bytes());
    out[20..22].copy_from_slice(&1u16.to_le_bytes());
    out[22..24].copy_from_slice(&1u16.to_le_bytes());
    out[24..28].copy_from_slice(&16_000u32.to_le_bytes());
    out[28..32].copy_from_slice(&32_000u32.to_le_bytes());
    out[32..34].copy_from_slice(&2u16.to_le_bytes());
    out[34..36].copy_from_slice(&16u16.to_le_bytes());
    out[36..40].copy_from_slice(b"data");
    out[40..44].copy_from_slice(&data_len.to_le_bytes());
    out
}

fn assemble(clips: &[Clip], path: &Path, stopped: &AtomicBool) -> Result<(), String> {
    let first = clips.first().ok_or("No saved meeting audio is available.")?.at_ms;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    let mut output = options.open(path)
        .map_err(|e| format!("Could not prepare meeting audio: {e}"))?;
    output.write_all(&wav_header(0)).map_err(|e| e.to_string())?;
    let mut written = 0u64;
    for clip in clips {
        if stopped.load(Ordering::Relaxed) { return Ok(()); }
        let mut input = std::fs::File::open(&clip.path).map_err(|e| format!("Could not read a saved clip: {e}"))?;
        let mut header = [0u8; 44];
        input.read_exact(&mut header).map_err(|e| format!("Invalid saved audio: {e}"))?;
        if header[0..4] != *b"RIFF" || header[8..40] != wav_header(0)[8..40] {
            return Err("A saved audio clip has an unsupported format.".into());
        }
        let len = u32::from_le_bytes(header[40..44].try_into().unwrap()) as u64;
        if len % 2 != 0 { return Err("A saved audio clip is incomplete.".into()); }
        let start = clip.at_ms.saturating_sub(first).checked_mul(32).ok_or("The meeting is too long to play.")?;
        let skip = written.saturating_sub(start).min(len);
        let position = written + gap_ms(start, written);
        let end = position.checked_add(len - skip).ok_or("The meeting is too long to play.")?;
        if end > (u32::MAX - 36) as u64 { return Err("The meeting is too long to play.".into()); }
        output.seek(SeekFrom::Start(44 + position)).map_err(|e| e.to_string())?;
        input.seek(SeekFrom::Start(44 + skip)).map_err(|e| e.to_string())?;
        let mut remaining = len - skip;
        let mut buffer = [0u8; 64 * 1024];
        while remaining > 0 {
            if stopped.load(Ordering::Relaxed) { return Ok(()); }
            let n = remaining.min(buffer.len() as u64) as usize;
            input.read_exact(&mut buffer[..n]).map_err(|e| format!("Invalid saved audio: {e}"))?;
            output.write_all(&buffer[..n]).map_err(|e| format!("Could not prepare meeting audio: {e}"))?;
            remaining -= n as u64;
        }
        written = end;
    }
    output.set_len(44 + written).map_err(|e| e.to_string())?;
    output.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    output.write_all(&wav_header(written as u32)).map_err(|e| e.to_string())?;
    Ok(())
}

pub struct Player {
    cancel: Arc<AtomicBool>,
    result: mpsc::Receiver<Result<Prepared, String>>,
    worker: Option<std::thread::JoinHandle<()>>,
    pub ranges: Vec<(u64, u64)>,
    pub first_ms: u64,
    pending_seek_ms: u64,
    should_play: bool,
    prepared: Option<Prepared>,
    #[cfg(target_os = "macos")]
    native: Option<NativePlayer>,
}

struct Prepared { dir: PathBuf, path: PathBuf }
impl Drop for Prepared { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.dir); } }

#[cfg(target_os = "macos")]
#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {}

#[cfg(target_os = "macos")]
struct NativePlayer(objc2::rc::Retained<objc2::runtime::AnyObject>);

#[cfg(target_os = "macos")]
impl NativePlayer {
    fn open(path: &Path) -> Result<Self, String> {
        use objc2::{msg_send, rc::Retained, runtime::AnyObject, runtime::AnyClass};
        let url = objc2_foundation::NSURL::from_file_path(path).ok_or("Could not open the prepared audio.")?;
        let class = AnyClass::get(c"AVAudioPlayer").ok_or("Audio playback is unavailable on this Mac.")?;
        let raw: *mut AnyObject = unsafe {
            let allocated: *mut AnyObject = msg_send![class, alloc];
            msg_send![allocated, initWithContentsOfURL: &*url, error: std::ptr::null_mut::<*mut AnyObject>()]
        };
        let player = unsafe { Retained::from_raw(raw) }.ok_or("Could not open the prepared audio.")?;
        Ok(Self(player))
    }
    fn play(&self) -> Result<(), String> {
        let ok: objc2::runtime::Bool = unsafe { objc2::msg_send![&*self.0, play] };
        if ok.as_bool() { Ok(()) } else { Err("Could not start audio playback.".into()) }
    }
    fn pause(&self) { unsafe { let _: () = objc2::msg_send![&*self.0, pause]; } }
    fn seek(&self, seconds: f64) { unsafe { let _: () = objc2::msg_send![&*self.0, setCurrentTime: seconds]; } }
    fn time(&self) -> f64 { unsafe { objc2::msg_send![&*self.0, currentTime] } }
    fn duration(&self) -> f64 { unsafe { objc2::msg_send![&*self.0, duration] } }
    fn playing(&self) -> bool { let value: objc2::runtime::Bool = unsafe { objc2::msg_send![&*self.0, isPlaying] }; value.as_bool() }
}

impl Player {
    pub fn start(clips: Vec<Clip>) -> Result<Self, String> {
        let first = clips.first().map_or(0, |c| c.at_ms);
        Self::start_at(clips, first)
    }
    pub fn start_at(clips: Vec<Clip>, at_ms: u64) -> Result<Self, String> {
        if clips.is_empty() { return Err("No saved meeting audio is available.".into()); }
        #[cfg(not(target_os = "macos"))]
        return Err("Meeting audio playback is currently available on macOS.".into());
        #[cfg(target_os = "macos")]
        {
            let ranges = clip_ranges(&clips)?;
            let first_ms = clips[0].at_ms;
            let cancel = Arc::new(AtomicBool::new(false));
            let stopped = cancel.clone();
            let (tx, result) = mpsc::channel();
            let worker = std::thread::spawn(move || {
                let result = prepare_clips(&clips, &stopped);
                let _ = tx.send(result);
            });
            Ok(Self { cancel, result, worker: Some(worker), ranges, first_ms, pending_seek_ms: at_ms, should_play: true, prepared: None, native: None })
        }
    }

    pub fn poll(&mut self) -> Result<(), String> {
        if self.prepared.is_some() { return Ok(()); }
        match self.result.try_recv() {
            Ok(result) => {
                let prepared = result?;
                #[cfg(target_os = "macos")]
                {
                    let native = NativePlayer::open(&prepared.path)?;
                    native.seek(meeting_to_playback_ms(self.first_ms, self.pending_seek_ms) as f64 / 1000.0);
                    if self.should_play { native.play()?; }
                    self.native = Some(native);
                }
                self.prepared = Some(prepared);
                Ok(())
            }
            Err(mpsc::TryRecvError::Disconnected) => Err("Audio preparation stopped unexpectedly.".into()),
            Err(mpsc::TryRecvError::Empty) => Ok(()),
        }
    }
    pub fn ready(&self) -> bool { self.prepared.is_some() }
    pub fn playing(&self) -> bool {
        #[cfg(target_os = "macos")]
        if let Some(native) = &self.native { return native.playing(); }
        self.should_play && !self.ready()
    }
    pub fn position_ms(&self) -> u64 {
        #[cfg(target_os = "macos")]
        if let Some(native) = &self.native { return playback_to_meeting_ms(self.first_ms, (native.time().max(0.0) * 1000.0) as u64); }
        self.pending_seek_ms
    }
    pub fn end_ms(&self) -> u64 {
        #[cfg(target_os = "macos")]
        if let Some(native) = &self.native { return playback_to_meeting_ms(self.first_ms, (native.duration().max(0.0) * 1000.0) as u64); }
        self.ranges.iter().map(|r| r.1).max().unwrap_or(self.first_ms)
    }
    pub fn pause(&mut self) { self.should_play = false; #[cfg(target_os = "macos")] if let Some(native) = &self.native { native.pause(); } }
    pub fn resume(&mut self) -> Result<(), String> { self.should_play = true; #[cfg(target_os = "macos")] if let Some(native) = &self.native { if self.position_ms() >= self.end_ms() { native.seek(0.0); } native.play()?; } Ok(()) }
    pub fn seek(&mut self, at_ms: u64) { self.pending_seek_ms = at_ms.clamp(self.first_ms, self.end_ms()); #[cfg(target_os = "macos")] if let Some(native) = &self.native { native.seek(meeting_to_playback_ms(self.first_ms, self.pending_seek_ms) as f64 / 1000.0); } }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
        #[cfg(target_os = "macos")]
        { self.native = None; }
        self.prepared = None;
    }
}

#[cfg(target_os = "macos")]
fn prepare_clips(clips: &[Clip], stopped: &AtomicBool) -> Result<Prepared, String> {
    use std::os::unix::fs::DirBuilderExt;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = loop {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("opendictate-playback-{}-{n}", std::process::id()));
        match std::fs::DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => break path,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Could not prepare meeting audio: {e}")),
        }
    };
    let prepared = Prepared { path: dir.join("meeting.wav"), dir };
    assemble(clips, &prepared.path, stopped)?;
    if stopped.load(Ordering::Relaxed) { return Err("Audio preparation cancelled.".into()); }
    Ok(prepared)
}

