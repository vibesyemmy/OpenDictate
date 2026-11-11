//! System audio capture through a Core Audio process tap.
//!
//! The microphone alone is one side of a call. A tap records what the Mac is
//! playing, which is everyone else, and unlike a virtual audio driver it
//! needs nothing installed, and unlike ScreenCaptureKit it never asks to
//! record the screen.
//!
//! Needs macOS 14.4 and the System Audio Recording permission, which macOS
//! asks for at the first `AudioDeviceStart` rather than at launch. Every call
//! is checked, so on an older system this fails at the first one and the
//! caller falls back to the microphone.

use objc2::rc::Retained;
use objc2::AnyThread;
use objc2_core_audio::*;
use objc2_core_audio_types::{AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp};
use objc2_core_foundation::{CFDictionary, CFString};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Everything the IO proc needs. Handed over as the client pointer, and
/// reclaimed when the capture stops.
struct Sink {
    /// Mono per channel group, at the device's own rate. Keeping the
    /// microphone and the tap apart is what lets a meeting tell what you
    /// said from what everyone else said.
    mine: Mutex<Vec<f32>>,
    theirs: Mutex<Vec<f32>>,
    /// How many of the device's input channels belong to the microphone.
    /// The aggregate lists its sub-devices before its taps, so the mic's
    /// channels come first - checked by level, not taken on trust.
    mic_channels: usize,
    /// How many belong to the tap. Together these say what the device should
    /// be handing over; when it hands over something else, the split is off.
    tap_channels: usize,
    /// How many times the device has called us, and how many of those
    /// carried any frames. The difference between "the device is not
    /// running" and "the device is running and handing over nothing" is not
    /// visible from the captured audio, and it was guessed at once already.
    calls: std::sync::atomic::AtomicUsize,
    with_frames: std::sync::atomic::AtomicUsize,
    /// Cleared the moment the channel layout stops matching what was opened.
    ///
    /// A 20 minute meeting came back with the two sides bit-identical for 70
    /// of its 76 chunks: the microphone had dropped out of the aggregate and
    /// the tap's stereo pair was being cut in half and labelled "you" and
    /// "them". Splitting on a layout that no longer holds does not produce an
    /// error, it produces a confident lie, so it is checked every callback.
    split_ok: AtomicBool,
    /// Set while diagnosing: the loudest sample seen on each channel.
    probe: Mutex<Vec<f32>>,
}

pub struct SystemCapture {
    tap: AudioObjectID,
    device: AudioObjectID,
    proc_id: AudioDeviceIOProcID,
    sink: *mut Arc<Sink>,
    pub rate: f64,
    /// Whether the microphone made it into the same device, or only the tap.
    pub has_mic: bool,
}

/// One pass of audio, with the two sides kept apart.
pub struct Chunk {
    /// The microphone: you, and the room you are in.
    pub mine: Vec<f32>,
    /// The system's output: everyone else on the call.
    pub theirs: Vec<f32>,
}

// The Core Audio ids are plain integers and the sink is reference counted;
// nothing here is tied to the thread that opened it.
unsafe impl Send for SystemCapture {}

fn ns(s: &str) -> Retained<NSString> {
    NSString::from_str(s)
}

fn key(k: &std::ffi::CStr) -> Retained<NSString> {
    ns(k.to_str().unwrap_or_default())
}

unsafe fn get<T>(object: AudioObjectID, selector: AudioObjectPropertySelector, out: &mut T) -> i32 {
    let a = AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    let mut size = std::mem::size_of::<T>() as u32;
    AudioObjectGetPropertyData(
        object,
        NonNull::from(&a),
        0,
        std::ptr::null(),
        NonNull::from(&mut size),
        NonNull::new(out as *mut T as *mut c_void).unwrap(),
    )
}

unsafe extern "C-unwind" fn io_proc(
    _device: AudioObjectID,
    _now: NonNull<AudioTimeStamp>,
    input: NonNull<AudioBufferList>,
    _input_time: NonNull<AudioTimeStamp>,
    _output: NonNull<AudioBufferList>,
    _output_time: NonNull<AudioTimeStamp>,
    client: *mut c_void,
) -> i32 {
    if client.is_null() {
        return 0;
    }
    let sink = &*(client as *const Arc<Sink>);
    sink.calls.fetch_add(1, Ordering::Relaxed);
    let list = input.as_ref();
    let buffers = std::slice::from_raw_parts(list.mBuffers.as_ptr(), list.mNumberBuffers as usize);

    // Frames are the same across buffers; channels are split across them when
    // the device is non-interleaved and packed inside them when it is not.
    let mut frames = 0usize;
    let mut channels = 0usize;
    for b in buffers {
        if b.mData.is_null() || b.mNumberChannels == 0 {
            continue;
        }
        let n = b.mDataByteSize as usize / 4 / b.mNumberChannels as usize;
        frames = frames.max(n);
        channels += b.mNumberChannels as usize;
    }
    if frames == 0 || channels == 0 {
        return 0;
    }
    sink.with_frames.fetch_add(1, Ordering::Relaxed);

    // Only split when the device is handing over exactly what it was opened
    // with. Anything else and we do not know whose channel is whose, so
    // everything goes down one side and the chunk is marked as mixed.
    let split = sink.mic_channels > 0 && channels == sink.mic_channels + sink.tap_channels;
    if !split {
        sink.split_ok.store(false, Ordering::Relaxed);
    }

    let mut mine = vec![0f32; frames];
    let mut theirs = vec![0f32; frames];
    let (mut n_mine, mut n_theirs) = (0usize, 0usize);
    let mut peaks = vec![0f32; channels];
    let mut index = 0usize;
    for b in buffers {
        if b.mData.is_null() || b.mNumberChannels == 0 {
            continue;
        }
        let ch = b.mNumberChannels as usize;
        let n = (b.mDataByteSize as usize / 4 / ch).min(frames);
        let data = std::slice::from_raw_parts(b.mData as *const f32, n * ch);
        for c in 0..ch {
            let global = index + c;
            let into_mine = split && global < sink.mic_channels;
            if into_mine {
                n_mine += 1;
            } else {
                n_theirs += 1;
            }
            let target = if into_mine { &mut mine } else { &mut theirs };
            let mut peak = 0f32;
            for f in 0..n {
                let v = data[f * ch + c];
                target[f] += v;
                let a = v.abs();
                if a > peak {
                    peak = a;
                }
            }
            if global < peaks.len() {
                peaks[global] = peak;
            }
        }
        index += ch;
    }
    if let Ok(mut out) = sink.mine.lock() {
        let scale = 1.0 / n_mine.max(1) as f32;
        out.extend(mine.iter().map(|s| s * scale));
    }
    if let Ok(mut out) = sink.theirs.lock() {
        let scale = 1.0 / n_theirs.max(1) as f32;
        out.extend(theirs.iter().map(|s| s * scale));
    }
    if let Ok(mut p) = sink.probe.lock() {
        if p.len() < peaks.len() {
            p.resize(peaks.len(), 0.0);
        }
        for (slot, v) in p.iter_mut().zip(peaks) {
            *slot = slot.max(v);
        }
    }
    0
}

/// An OSStatus as the four-character code Core Audio means by it.
fn status(st: i32) -> String {
    let b = (st as u32).to_be_bytes();
    if b.iter().all(|c| c.is_ascii_graphic()) {
        format!("{st} '{}'", String::from_utf8_lossy(&b))
    } else {
        st.to_string()
    }
}

/// A device's input channel count, read from its stream configuration.
fn input_channels(device: AudioObjectID) -> usize {
    unsafe {
        let a = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyStreamConfiguration,
            mScope: kAudioObjectPropertyScopeInput,
            mElement: kAudioObjectPropertyElementMain,
        };
        let mut size = 0u32;
        if AudioObjectGetPropertyDataSize(device, NonNull::from(&a), 0, std::ptr::null(), NonNull::from(&mut size)) != 0 {
            return 0;
        }
        let mut buf = vec![0u8; size as usize];
        if AudioObjectGetPropertyData(
            device,
            NonNull::from(&a),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::new(buf.as_mut_ptr() as *mut c_void).unwrap(),
        ) != 0
        {
            return 0;
        }
        let list = &*(buf.as_ptr() as *const AudioBufferList);
        let buffers = std::slice::from_raw_parts(list.mBuffers.as_ptr(), list.mNumberBuffers as usize);
        buffers.iter().map(|b| b.mNumberChannels as usize).sum()
    }
}

unsafe fn device_uid(device: AudioObjectID) -> String {
    let mut uid: *const CFString = std::ptr::null();
    get(device, kAudioDevicePropertyDeviceUID, &mut uid);
    if uid.is_null() { String::new() } else { (*uid).to_string() }
}

unsafe fn device_name(device: AudioObjectID) -> String {
    let mut name: *const CFString = std::ptr::null();
    get(device, kAudioObjectPropertyName, &mut name);
    if name.is_null() { String::new() } else { (*name).to_string() }
}

/// Every Core Audio device that can record, with the name it shows under.
fn input_devices() -> Vec<(AudioObjectID, String)> {
    unsafe {
        let a = AudioObjectPropertyAddress {
            mSelector: kAudioHardwarePropertyDevices,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };
        let mut size = 0u32;
        if AudioObjectGetPropertyDataSize(kAudioObjectSystemObject as u32, NonNull::from(&a), 0, std::ptr::null(), NonNull::from(&mut size)) != 0 {
            return Vec::new();
        }
        let mut ids = vec![0 as AudioObjectID; size as usize / std::mem::size_of::<AudioObjectID>()];
        if ids.is_empty()
            || AudioObjectGetPropertyData(
                kAudioObjectSystemObject as u32,
                NonNull::from(&a),
                0,
                std::ptr::null(),
                NonNull::from(&mut size),
                NonNull::new(ids.as_mut_ptr() as *mut c_void).unwrap(),
            ) != 0
        {
            return Vec::new();
        }
        ids.into_iter().filter(|d| input_channels(*d) > 0).map(|d| (d, device_name(d))).collect()
    }
}

/// The Core Audio id of the microphone named in Audio input.
///
/// cpal takes its device names straight from Core Audio, so the name stored
/// in settings is the name this matches on. Nothing clever if it misses: a
/// microphone that has been unplugged since it was chosen is simply not
/// there, and the caller falls back.
pub fn input_named(name: &str) -> Option<AudioObjectID> {
    let want = name.trim();
    if want.is_empty() {
        return None;
    }
    input_devices().into_iter().find(|(_, n)| n == want).map(|(d, _)| d)
}

/// The microphone a meeting should record.
///
/// Audio input first, the system default second. Until now a meeting always
/// took the system default, which is how a Mac set to record from a chosen
/// microphone produced a meeting with nobody in it: the default input here
/// was a device that hands over silence, while the chosen one worked.
pub fn meeting_input(configured: &str) -> Option<AudioObjectID> {
    input_named(configured).or_else(default_input)
}

/// The Core Audio id of the system's default input.
/// Makes the audio chain run.
///
/// An aggregate built from a process tap and a microphone does not run at
/// all until some process plays something: measured as zero IO callbacks and
/// zero samples through a silent window, then 657,920 samples the moment a
/// sound played. The microphone sits in that device, so it was deaf until
/// the call started - which is exactly what "my audio doesn't come in until
/// theirs does" means.
///
/// It is only the cold start. Once anything has played the device keeps
/// clocking through the quiet: 563 more callbacks and the microphone still
/// at 0.147 after the sound stopped. So a third of a second of silence,
/// played by us at the moment recording begins, is enough to wake it, and
/// nobody hears it.
fn wake() {
    let path = std::env::temp_dir().join("open-dictate-wake.wav");
    if !path.is_file() {
        // 0.3 s of 48 kHz mono silence, as a minimal RIFF file.
        let frames = 14_400u32;
        let bytes = frames * 2;
        let mut w = Vec::with_capacity(44 + bytes as usize);
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + bytes).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes()); // PCM
        w.extend_from_slice(&1u16.to_le_bytes()); // mono
        w.extend_from_slice(&48_000u32.to_le_bytes());
        w.extend_from_slice(&96_000u32.to_le_bytes()); // byte rate
        w.extend_from_slice(&2u16.to_le_bytes()); // block align
        w.extend_from_slice(&16u16.to_le_bytes()); // bits
        w.extend_from_slice(b"data");
        w.extend_from_slice(&bytes.to_le_bytes());
        w.resize(44 + bytes as usize, 0);
        if std::fs::write(&path, w).is_err() {
            return;
        }
    }
    let _ = std::process::Command::new("afplay")
        .arg(&path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

pub fn default_input() -> Option<AudioObjectID> {
    unsafe {
        let mut dev: AudioObjectID = 0;
        let st = get(kAudioObjectSystemObject as u32, kAudioHardwarePropertyDefaultInputDevice, &mut dev);
        (st == 0 && dev != 0).then_some(dev)
    }
}

impl SystemCapture {
    /// Opens one device carrying the system's output and, when `mic` names a
    /// Core Audio device, that microphone with it. Both arrive mixed, which
    /// is what a meeting transcript wants.
    pub fn start(mic: Option<AudioObjectID>) -> Result<Self, String> {
        unsafe {
            let desc = CATapDescription::initStereoGlobalTapButExcludeProcesses(
                CATapDescription::alloc(),
                &NSArray::new(),
            );
            desc.setName(&ns("Open Dictate"));
            desc.setPrivate(true);
            // Unmuted: a meeting you cannot hear is not a meeting.
            desc.setMuteBehavior(CATapMuteBehavior::Unmuted);

            let mut tap: AudioObjectID = 0;
            let st = AudioHardwareCreateProcessTap(Some(&desc), &mut tap);
            if st != 0 || tap == 0 {
                return Err(format!("system audio is unavailable (tap {})", status(st)));
            }
            let uuid = desc.UUID().UUIDString().to_string();

            let mut out_dev: AudioObjectID = 0;
            get(kAudioObjectSystemObject as u32, kAudioHardwarePropertyDefaultOutputDevice, &mut out_dev);
            let out_uid = device_uid(out_dev);

            let mic_uid = mic.map(|d| device_uid(d)).filter(|u| !u.is_empty());
            let has_mic = mic_uid.is_some();
            let subs: Vec<Retained<NSDictionary<NSString, objc2::runtime::AnyObject>>> = mic_uid
                .iter()
                .map(|uid| {
                    NSDictionary::from_slices::<NSString>(
                        &[&key(kAudioSubDeviceUIDKey), &key(kAudioSubDeviceDriftCompensationKey)],
                        &[&*ns(uid) as &objc2::runtime::AnyObject, &*NSNumber::new_u32(1)],
                    )
                })
                .collect();
            let sub_refs: Vec<&objc2::runtime::AnyObject> =
                subs.iter().map(|d| &**d as &objc2::runtime::AnyObject).collect();

            let sub_tap = NSDictionary::from_slices::<NSString>(
                &[&key(kAudioSubTapUIDKey), &key(kAudioSubTapDriftCompensationKey)],
                &[&*ns(&uuid) as &objc2::runtime::AnyObject, &*NSNumber::new_u32(1)],
            );
            let agg = NSDictionary::from_slices::<NSString>(
                &[
                    &key(kAudioAggregateDeviceNameKey),
                    &key(kAudioAggregateDeviceUIDKey),
                    &key(kAudioAggregateDeviceMainSubDeviceKey),
                    &key(kAudioAggregateDeviceIsPrivateKey),
                    &key(kAudioAggregateDeviceTapAutoStartKey),
                    &key(kAudioAggregateDeviceSubDeviceListKey),
                    &key(kAudioAggregateDeviceTapListKey),
                ],
                &[
                    &*ns("Open Dictate meeting") as &objc2::runtime::AnyObject,
                    &*ns("app.opendictate.meeting.aggregate"),
                    &*ns(&out_uid),
                    &*NSNumber::new_u32(1),
                    &*NSNumber::new_u32(1),
                    &*NSArray::from_slice(&sub_refs),
                    &*NSArray::from_slice(&[&*sub_tap]),
                ],
            );
            let mut device: AudioObjectID = 0;
            let st = AudioHardwareCreateAggregateDevice(
                &*(Retained::as_ptr(&agg) as *const CFDictionary),
                NonNull::from(&mut device),
            );
            if st != 0 || device == 0 {
                AudioHardwareDestroyProcessTap(tap);
                return Err(format!("system audio is unavailable (device {})", status(st)));
            }

            let mut fmt: AudioStreamBasicDescription = std::mem::zeroed();
            get(tap, kAudioTapPropertyFormat, &mut fmt);
            // The tap's own format is not what the IO proc delivers: an
            // aggregate device built around a microphone adopts a rate of
            // its own, and reading the tap's instead had a 42 second meeting
            // arriving as 21 seconds of audio. Its channel count is still
            // the tap's own, and that is what the split is measured against.
            let mut rate: f64 = 0.0;
            get(device, kAudioDevicePropertyNominalSampleRate, &mut rate);
            if rate <= 0.0 {
                rate = if fmt.mSampleRate > 0.0 { fmt.mSampleRate } else { 48_000.0 };
            }

            // The mic's own channel count, so the IO proc knows where its
            // channels stop and the tap's begin.
            let mic_channels = mic.filter(|_| has_mic).map(|d| input_channels(d)).unwrap_or(0);
            let tap_channels = fmt.mChannelsPerFrame as usize;
            let sink = Box::into_raw(Box::new(Arc::new(Sink {
                mine: Mutex::new(Vec::new()),
                theirs: Mutex::new(Vec::new()),
                calls: std::sync::atomic::AtomicUsize::new(0),
                with_frames: std::sync::atomic::AtomicUsize::new(0),
                mic_channels,
                tap_channels,
                split_ok: AtomicBool::new(mic_channels > 0 && tap_channels > 0),
                probe: Mutex::new(Vec::new()),
            })));
            let mut proc_id: AudioDeviceIOProcID = None;
            let st = AudioDeviceCreateIOProcID(device, Some(io_proc), sink as *mut c_void, NonNull::from(&mut proc_id));
            if st != 0 {
                drop(Box::from_raw(sink));
                AudioHardwareDestroyAggregateDevice(device);
                AudioHardwareDestroyProcessTap(tap);
                return Err(format!("system audio is unavailable (io proc {})", status(st)));
            }
            // macOS asks for System Audio Recording here, not at launch, so
            // this is the call that can sit behind a dialog.
            let st = AudioDeviceStart(device, proc_id);
            if st != 0 {
                if let Some(p) = proc_id {
                    AudioDeviceDestroyIOProcID(device, Some(p));
                }
                drop(Box::from_raw(sink));
                AudioHardwareDestroyAggregateDevice(device);
                AudioHardwareDestroyProcessTap(tap);
                return Err(format!("system audio was refused (start {})", status(st)));
            }
            wake();
            Ok(Self { tap, device, proc_id, sink, rate, has_mic })
        }
    }

    /// Stops the IO proc without tearing the device down, so the last of
    /// the audio can be collected without more arriving behind it.
    pub fn pause(&self) {
        unsafe {
            AudioDeviceStop(self.device, self.proc_id);
        }
    }

    /// Everything captured since the last call, the two sides apart, each
    /// mono at `rate`.
    pub fn drain(&self) -> Chunk {
        unsafe {
            let sink = &*self.sink;
            Chunk {
                mine: sink.mine.lock().map(|mut s| std::mem::take(&mut *s)).unwrap_or_default(),
                theirs: sink.theirs.lock().map(|mut s| std::mem::take(&mut *s)).unwrap_or_default(),
            }
        }
    }

    /// Loudest sample seen on each of the device's input channels since the
    /// capture opened. Only used to check that the microphone really does
    /// come before the tap.
    pub fn channel_peaks(&self) -> Vec<f32> {
        unsafe {
            let sink = &*self.sink;
            sink.probe.lock().map(|p| p.clone()).unwrap_or_default()
        }
    }

    /// Whether the two sides can still be told apart. False once the device
    /// has handed over a layout other than the one it was opened with.
    pub fn split_ok(&self) -> bool {
        unsafe {
            let sink = &*self.sink;
            sink.split_ok.load(Ordering::Relaxed)
        }
    }

    /// (callbacks, callbacks that carried frames) since the capture opened.
    pub fn io_calls(&self) -> (usize, usize) {
        unsafe {
            let sink = &*self.sink;
            (sink.calls.load(Ordering::Relaxed), sink.with_frames.load(Ordering::Relaxed))
        }
    }

    pub fn mic_channels(&self) -> usize {
        unsafe {
            let sink = &*self.sink;
            sink.mic_channels
        }
    }
}

impl Drop for SystemCapture {
    fn drop(&mut self) {
        unsafe {
            AudioDeviceStop(self.device, self.proc_id);
            if let Some(p) = self.proc_id {
                AudioDeviceDestroyIOProcID(self.device, Some(p));
            }
            AudioHardwareDestroyAggregateDevice(self.device);
            AudioHardwareDestroyProcessTap(self.tap);
            drop(Box::from_raw(self.sink));
        }
    }
}

/// `--tap-check`: proves system audio works on this Mac, the way
/// `--mic-check` proves the microphone does.
///
/// Plays a system sound at itself, so a tap that runs but captures nothing
/// can be told from one that works. Only a run from the installed bundle
/// means anything about the permission: a bare binary inherits the TCC
/// identity of whatever launched it.
pub fn check(secs: u64, microphone: &str) -> String {
    let mut r = String::new();
    r.push_str(&format!(
        "inputs: {:?}\nasked for: {microphone:?}\n",
        input_devices().iter().map(|(_, n)| n.as_str()).collect::<Vec<_>>()
    ));
    // The same choice a meeting makes, or the check proves nothing about
    // meetings.
    match SystemCapture::start(meeting_input(microphone)) {
        Err(e) => r.push_str(&format!("{e}\nVERDICT: no system audio.\n")),
        Ok(cap) => {
            r.push_str(&format!("opened at {} Hz\n", cap.rate));
            let pk = |v: &[f32]| v.iter().fold(0f32, |m, x| m.max(x.abs()));

            // Before anything plays. A process tap carries nothing until a
            // process makes a sound, and the microphone sits in the same
            // aggregate device: if the device only clocks once the tap has
            // something to say, the microphone is deaf until the call
            // starts. This window is what tells those apart, so speak into
            // the microphone while it runs.
            r.push_str("say something now - listening with nothing playing\n");
            std::thread::sleep(std::time::Duration::from_secs(secs));
            let quiet = cap.drain();
            r.push_str(&format!(
                "  with nothing playing: you {:.4}  them {:.4}  ({} samples)\n",
                pk(&quiet.mine),
                pk(&quiet.theirs),
                quiet.mine.len()
            ));
            let (calls, with_frames) = cap.io_calls();
            r.push_str(&format!("  the device called us {calls} times, {with_frames} with any frames\n"));

            let player = std::thread::spawn(move || {
                for _ in 0..secs.max(1) {
                    let _ = std::process::Command::new("afplay")
                        .arg("/System/Library/Sounds/Submarine.aiff")
                        .status();
                }
            });
            std::thread::sleep(std::time::Duration::from_secs(secs));
            let _ = player.join();
            let c = cap.drain();
            r.push_str(&format!(
                "  with sound playing:   you {:.4}  them {:.4}  ({} samples)\n",
                pk(&c.mine),
                pk(&c.theirs),
                c.mine.len()
            ));
            let (after_play, _) = cap.io_calls();

            // And once it has stopped again. If the device keeps clocking
            // after the first sound, the microphone is only deaf until
            // something plays once, which is a very different problem from
            // being deaf whenever the room is quiet.
            std::thread::sleep(std::time::Duration::from_secs(secs));
            let quiet2 = cap.drain();
            let (calls2, _) = cap.io_calls();
            r.push_str(&format!(
                "  after it stopped:     you {:.4}  them {:.4}  ({} samples, {} more callbacks)\n",
                pk(&quiet2.mine),
                pk(&quiet2.theirs),
                quiet2.mine.len(),
                calls2 - after_play
            ));
            r.push_str(&format!(
                "mic channels {}, per-channel peaks {:?}\n",
                cap.mic_channels(),
                cap.channel_peaks().iter().map(|p| (p * 1000.0).round() / 1000.0).collect::<Vec<_>>()
            ));
            r.push_str(&format!("you {:.4}  them {:.4}\n", pk(&c.mine), pk(&c.theirs)));
            let s = c.theirs;
            let peak = pk(&s);
            r.push_str(&format!("captured {} samples, peak {peak:.4}\n", s.len()));
            r.push_str(if s.is_empty() {
                "VERDICT: nothing arrived - the IO proc never ran.\n"
            } else if peak < 0.001 {
                "VERDICT: silence - running, but capturing nothing. Permission.\n"
            } else {
                "VERDICT: system audio captured.\n"
            });
        }
    }
    r
}
