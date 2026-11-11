//! Meeting mode: a long recording of both sides of a call, transcribed as it
//! runs and kept on disk.
//!
//! A meeting is too long to hold in memory and too long to transcribe in one
//! go, so it is cut into stretches at pauses and turns that are written, transcribed and thrown away
//! while the next one fills. That keeps memory flat however long the meeting
//! runs, puts a transcript on screen before it ends, and costs at most one
//! stretch if the app dies.
//!
//! ```text
//! meetings/<started>/
//!   meta.json         title, started, ended, source
//!   transcript.jsonl  one line per stretch: { at_ms, secs, text }
//!   audio/000.wav     deleted as each one is transcribed
//! ```
//!
//! JSONL because a meeting in progress is appended to every few seconds: a
//! crash truncates one line instead of corrupting the file.

use crate::settings::{self, Settings};
use crate::tap::SystemCapture;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const OUT_RATE: u32 = 16_000;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Meta {
    pub id: String,
    /// What the meeting was about, written by the model once it has been
    /// summarised. Until then - and for a meeting nothing was transcribed
    /// from - it is the day and time the recording started.
    pub title: String,
    /// One line under the title, from the same model pass. Empty when the
    /// meeting has not been summarised, or when what came back was no use.
    #[serde(default)]
    pub subtitle: String,
    /// When the transcript was last corrected by hand, if it ever was.
    /// Recorded rather than read from the file's modified time: copying a
    /// meeting, restoring it from a backup or syncing it all move the mtime
    /// without a word having changed.
    #[serde(default)]
    pub edited_at: Option<u64>,
    pub started: u64,
    pub ended: Option<u64>,
    /// "You and the room" or "Everyone on the call", for the UI to show.
    pub source: String,
    /// Loudest sample in the whole meeting, 0 to 1. The only way to tell a
    /// quiet room from a recorder that heard nothing.
    #[serde(default)]
    pub peak: f32,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Segment {
    pub at_ms: u64,
    pub secs: f32,
    pub text: String,
    /// "you", "them" or "both", from which side of the call was louder.
    /// Older meetings have none, hence the default.
    #[serde(default)]
    pub voice: String,
}

/// What the UI reads while a meeting is running.
#[derive(Default)]
pub struct Live {
    pub elapsed: f32,
    pub segments: Vec<Segment>,
    /// Set when a chunk failed to transcribe; the recording carries on.
    pub warning: Option<String>,
    /// Loudest sample seen since the meeting started. Still zero once samples
    /// have arrived means nothing is reaching the recorder at all, which is a very
    /// different problem from a quiet room and has to be said differently.
    pub peak: f32,
    pub samples: usize,
    /// Whether the microphone ever contributed anything. The aggregate can
    /// report a microphone and then deliver nothing from it, and a meeting
    /// that says "you and everyone on the call" when your side was never
    /// recorded is worse than one that admits it.
    pub heard_you: bool,
}

pub fn root() -> PathBuf {
    settings::data_dir().join("meetings")
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Every meeting on disk, newest first.
pub fn list() -> Vec<Meta> {
    let mut out: Vec<Meta> = std::fs::read_dir(root())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| std::fs::read_to_string(e.path().join("meta.json")).ok())
        .filter_map(|s| serde_json::from_str::<Meta>(&s).ok())
        .collect();
    out.sort_by(|a, b| b.started.cmp(&a.started));
    out
}

pub fn transcript(id: &str) -> Vec<Segment> {
    std::fs::read_to_string(root().join(id).join("transcript.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<Segment>(l).ok())
        .collect()
}

/// Rewrites one line of a transcript.
///
/// The file is appended to while a meeting runs and only read afterwards, so
/// a correction is a whole-file rewrite. `at_ms` is the key: stretches are
/// cut in order and none of them start at the same millisecond.
///
/// Writes through a temporary file and renames over the original, so a
/// correction cannot leave half a transcript behind if something fails
/// mid-write. A meeting is not recoverable once its audio is gone.
pub fn edit_segment(id: &str, at_ms: u64, text: &str) -> Result<(), String> {
    let path = root().join(id).join("transcript.jsonl");
    let mut segments = transcript(id);
    let Some(seg) = segments.iter_mut().find(|s| s.at_ms == at_ms) else {
        return Err("That line is no longer in this meeting.".into());
    };
    if seg.text == text {
        return Ok(());
    }
    seg.text = text.to_string();
    let body: String = segments
        .iter()
        .filter_map(|s| serde_json::to_string(s).ok())
        .map(|l| l + "\n")
        .collect();
    let tmp = path.with_extension("jsonl.new");
    std::fs::write(&tmp, body).map_err(|e| format!("Could not save the correction: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("Could not save the correction: {e}"))?;

    let meta_path = root().join(id).join("meta.json");
    if let Some(mut m) = std::fs::read_to_string(&meta_path).ok().and_then(|s| serde_json::from_str::<Meta>(&s).ok()) {
        m.edited_at = Some(now());
        let _ = std::fs::write(&meta_path, serde_json::to_string_pretty(&m).unwrap_or_default());
    }
    Ok(())
}

pub fn rename(id: &str, title: &str) {
    let path = root().join(id).join("meta.json");
    if let Some(mut m) = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<Meta>(&s).ok()) {
        m.title = title.to_string();
        let _ = std::fs::write(&path, serde_json::to_string_pretty(&m).unwrap_or_default());
    }
}

/// Stores the name the model gave a meeting. A blank title is ignored, so a
/// model that returns nothing useful leaves the day and time in place rather
/// than wiping it.
pub fn set_name(id: &str, title: &str, subtitle: &str) {
    if title.trim().is_empty() {
        return;
    }
    let path = root().join(id).join("meta.json");
    if let Some(mut m) = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<Meta>(&s).ok()) {
        m.title = title.trim().to_string();
        m.subtitle = subtitle.trim().to_string();
        let _ = std::fs::write(&path, serde_json::to_string_pretty(&m).unwrap_or_default());
    }
}

pub fn delete(id: &str) {
    // Guard against a crafted id walking out of the meetings directory.
    if id.contains('/') || id.contains("..") {
        return;
    }
    let _ = std::fs::remove_dir_all(root().join(id));
}

/// Average every `ratio` samples. The tap runs at 48k and whisper wants 16k,
/// so this is a box filter of three - crude as a low-pass, but speech has
/// nothing up there worth protecting.
fn to_16k(input: &[f32], from: f64) -> Vec<f32> {
    if input.is_empty() {
        return Vec::new();
    }
    let ratio = (from / OUT_RATE as f64).max(1.0);
    let out_len = (input.len() as f64 / ratio) as usize;
    (0..out_len)
        .map(|i| {
            let start = (i as f64 * ratio) as usize;
            let end = (((i + 1) as f64 * ratio) as usize).min(input.len()).max(start + 1);
            let slice = &input[start..end.min(input.len())];
            slice.iter().sum::<f32>() / slice.len().max(1) as f32
        })
        .collect()
}

fn wav_16k_mono(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&OUT_RATE.to_le_bytes());
    out.extend_from_slice(&(OUT_RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    out
}

/// A line per chunk, beside the transcript. A meeting that comes back empty
/// has to be able to say why without being run again under a debugger.
pub(crate) fn log(dir: &Path, line: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("levels.log")) {
        let _ = writeln!(f, "{line}");
    }
}

fn append_segment(dir: &Path, seg: &Segment) {
    use std::io::Write;
    if let Ok(line) = serde_json::to_string(seg) {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("transcript.jsonl")) {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// A meeting being recorded. Dropping it stops the recording.
pub struct Session {
    pub id: String,
    pub dir: PathBuf,
    pub source: String,
    pub live: Arc<Mutex<Live>>,
    pub talking: Option<Arc<Mutex<crate::talking::Live>>>,
    talking_ended: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

fn should_start_lm_studio(s: &Settings, memory: u64, lms_found: bool) -> bool {
    lms_found && crate::lms::is_lm_studio(s) && crate::talking::availability(s, memory) == crate::talking::Availability::Ready
}

impl Session {
    pub fn start(s: &Settings) -> Result<Self, String> {
        let cap = SystemCapture::start(crate::tap::meeting_input(&s.microphone))?;
        let source = if cap.has_mic { "You and everyone on the call" } else { "Everyone on the call" };
        let started = now();
        let id = chrono::Local::now().format("%Y-%m-%d-%H%M%S").to_string();
        let dir = root().join(&id);
        std::fs::create_dir_all(dir.join("audio")).map_err(|e| format!("could not make room for the meeting: {e}"))?;
        let meta = Meta {
            id: id.clone(),
            title: chrono::Local::now().format("%A %H:%M").to_string(),
            subtitle: String::new(),
            edited_at: None,
            started,
            ended: None,
            source: source.to_string(),
            peak: 0.0,
        };
        let _ = std::fs::write(dir.join("meta.json"), serde_json::to_string_pretty(&meta).unwrap_or_default());

        let live = Arc::new(Mutex::new(Live::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker = std::thread::spawn({
            let (dir, live, stop) = (dir.clone(), live.clone(), stop.clone());
            let settings = s.clone();
            move || run(cap, dir, live, stop, settings)
        });
        if should_start_lm_studio(s, crate::talking::physical_memory(), crate::lms::find().is_some()) {
            let (dir, s) = (dir.clone(), s.clone());
            std::thread::spawn(move || {
                if let Err(e) = crate::lms::ensure(&s, &|l| log(&dir, &format!("lm studio: {l}"))) {
                    log(&dir, &format!("lm studio: {e}"));
                }
            });
        }
        let talking_ended = Arc::new(AtomicBool::new(false));
        let talking = s.talking_points.then(|| crate::talking::spawn(s.clone(), live.clone(), talking_ended.clone(), dir.clone()));
        Ok(Self { id, dir, source: source.to_string(), live, talking, talking_ended, stop, worker: Some(worker) })
    }

    /// Stops recording and waits for the last chunk to be transcribed.
    pub fn stop(mut self) -> String {
        self.finish();
        self.id.clone()
    }

    fn finish(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
        self.talking_ended.store(true, Ordering::Relaxed);
        let path = self.dir.join("meta.json");
        if let Some(mut m) = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<Meta>(&s).ok()) {
            m.ended = Some(now());
            if let Ok(l) = self.live.lock() {
                m.peak = l.peak;
                if !l.heard_you && !no_audio_reached(l.samples) {
                    m.source = "Everyone on the call · your microphone was not captured".into();
                }
            }
            let _ = std::fs::write(&path, serde_json::to_string_pretty(&m).unwrap_or_default());
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.worker.is_some() {
            self.finish();
        }
    }
}

fn run(cap: SystemCapture, dir: PathBuf, live: Arc<Mutex<Live>>, stop: Arc<AtomicBool>, s: Settings) {
    let rate = cap.rate;
    let mut cutter = crate::cut::Cutter::new(rate);
    let mut index = 0usize;
    let started = std::time::Instant::now();
    let server = std::cell::RefCell::new(start_server(&s, &dir, &stop));
    let transcribe = |wav: &[u8]| {
        let mut server = server.borrow_mut();
        let text = crate::whisper_server::transcribe_stretch(
            &mut server,
            &s,
            wav,
            &|w| crate::engines::transcribe(&s, w),
            &|why| log(&dir, &format!("    whisper-server: {why}")),
        );
        text
    };

    let mut process = |stretches: Vec<crate::cut::Stretch>| {
        for st in stretches {
            match handle_stretch(&dir, index, &st, rate, &transcribe) {
                Ok(Some(seg)) => {
                    if let Ok(mut l) = live.lock() {
                        l.segments.push(seg);
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    if let Ok(mut l) = live.lock() {
                        l.warning = Some(e);
                    }
                }
            }
            index += 1;
        }
    };

    loop {
        let ending = stop.load(Ordering::Relaxed);
        let mut got = cap.drain();
        if ending {
            cap.pause();
            let last = cap.drain();
            got.mine.extend(last.mine);
            got.theirs.extend(last.theirs);
        }
        if let Ok(mut l) = live.lock() {
            l.elapsed = started.elapsed().as_secs_f32();
            l.samples += got.mine.len().max(got.theirs.len());
            l.heard_you |= rms(&got.mine) > 0.001;
            l.peak = l.peak.max(peak(&got.mine)).max(peak(&got.theirs));
        }
        process(cutter.push(&got.mine, &got.theirs, cap.split_ok()));
        if ending {
            process(cutter.flush().into_iter().collect());
            if let Ok(mut l) = live.lock() {
                if no_audio_reached(l.samples) {
                    l.warning = Some("No audio reached the recorder.".into());
                    log(&dir, "no audio arrived at all");
                }
            }
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

fn start_server(s: &Settings, dir: &Path, stop: &AtomicBool) -> Option<crate::whisper_server::WhisperServer> {
    if s.speech_engine != crate::settings::SpeechEngine::WhisperLocal {
        return None;
    }
    crate::whisper_server::WhisperServer::start(s, stop, dir)
        .inspect(|_| log(dir, "whisper-server ready"))
        .inspect_err(|e| log(dir, &format!("whisper-server not used, falling back to whisper-cli: {e}")))
        .ok()
}

fn no_audio_reached(samples: usize) -> bool {
    samples == 0
}

fn peak(v: &[f32]) -> f32 {
    v.iter().fold(0f32, |m, x| m.max(x.abs()))
}

fn rms(v: &[f32]) -> f32 {
    (v.iter().map(|x| x * x).sum::<f32>() / v.len().max(1) as f32).sqrt()
}

fn handle_stretch(
    dir: &Path,
    index: usize,
    st: &crate::cut::Stretch,
    rate: f64,
    transcribe: &dyn Fn(&[u8]) -> Result<String, String>,
) -> Result<Option<Segment>, String> {
    let samples = to_16k(&st.samples, rate);
    let (loudest, level) = (peak(&samples), rms(&samples));
    log(dir, &format!("{index:03} {:5.1}s {:<4} peak {loudest:.3} rms {level:.4} voiced {:.1}s", st.secs, st.voice, st.voiced_secs));
    if !crate::cut::worth_transcribing(loudest, level, st.voiced_secs) {
        log(dir, "    skipped: too quiet to be speech");
        return Ok(None);
    }

    let path = dir.join("audio").join(format!("{index:03}.wav"));
    let wav = wav_16k_mono(&samples);
    let _ = std::fs::write(&path, &wav);
    let result = transcribe(&wav);
    let _ = std::fs::remove_file(&path);
    match result {
        Ok(text) if !text.trim().is_empty() => {
            log(dir, &format!("    {}: {}", st.voice, text.trim().chars().take(60).collect::<String>()));
            let seg = Segment { at_ms: (st.start_secs * 1000.0).round() as u64, secs: st.secs, text: text.trim().to_string(), voice: st.voice.to_string() };
            append_segment(dir, &seg);
            Ok(Some(seg))
        }
        Ok(text) => {
            log(dir, &format!("    transcribed to nothing ({} chars raw)", text.len()));
            Ok(None)
        }
        Err(e) => {
            log(dir, &format!("    failed: {e}"));
            Err(e)
        }
    }
}

// ---------------------------------------------------------------------------
// Summarising, at the end

/// What a meeting came to. Built once when the meeting stops, and kept
/// beside the transcript so it is never recomputed by accident.
#[derive(Clone, Default, Serialize, Deserialize, Debug)]
pub struct Summary {
    pub overview: String,
    pub decisions: Vec<String>,
    pub actions: Vec<String>,
    /// Questions nothing in the meeting answered.
    pub questions: Vec<String>,
    pub model: String,
    pub made_at: u64,
}

/// Small on purpose. At 450 words a 3B model stops listing and starts
/// summarising, and found one action in five; at 180 it found four. The
/// limit is attention, not context.
const BLOCK_WORDS: usize = 180;

const MAP: &str = "You list what happened in one part of a meeting transcript. You are not summarising it: \
you are going through it and writing down every item you find, including small ones.\n\n\
The transcript has no speaker labels; write \"someone\" where you cannot tell.\n\n\
One line per item, each line starting with exactly one of:\n\n\
DECISION: something the meeting agreed, settled or chose. Not a topic it touched on.\n\
QUESTION: something asked that this part does not answer.\n\
ACTION: something a person committed to doing after the meeting. Not something \
they did during it, not an opinion, not a suggestion, and not anything hypothetical.\n\n\
Rules:\n\
- Work through the part in order and catch every item. Missing one is the main way to do this badly.\n\
- Lines marked [you] were spoken by the person this summary is for; [the others] by everyone else. \
Say \"you\" for the first and \"someone\" for the second.\n\
- Only what is in the transcript. Never add, guess or round out. If the part is thin, write few lines.\n\
- One item per line, under twenty words. No headings, no numbering, no blank lines, no commentary.\n\
- Do not describe the conversation. An item is something still true tomorrow: a choice \
made, a question left hanging, a task taken on. A line reporting what was said is not one. \
Never copy wording from these rules into your answer.\n\
- A part this size rarely holds more than three or four of any one kind. If you are \
writing more than that, you are describing rather than listing.";

const OVERVIEW: &str = "You are writing the opening of a meeting summary for someone who missed it.\n\n\
Two or three sentences, plain and factual, saying what the meeting was about and what came of it. \
No bullet points, no preamble, no sign-off. Only what the notes say.";

const NAME: &str = "You are naming a meeting from its summary, the way someone would \
name it in a list of meetings.\n\n\
title: the subject, two to five words, built from the things the summary actually \
names. If it is mostly about one person, product or project, the title is that name - \
a summary about Marie Curie and radium is \"Marie Curie And Radium\", not \"Early \
Experiments\". No date, no time, no \
weekday, and never the word \"meeting\". Capitalise it like a heading. A title that \
would fit any meeting at all, such as \"Recent Developments\", \"Team Meeting\" or \
\"General Discussion\", is a failure: name what this one was about.\n\
subtitle: what came of it, under twelve words, written as a sentence in ordinary \
sentence case. The outcome, not the subject again. Leave it empty rather than padding it.\n\n\
Use only what the notes below say. Every word you write must come from them.";

const OVERVIEW_TALK: &str = "You are writing the opening of a summary for someone who \
missed this.\n\n\
You get part of what was said, in order. Two or three sentences, plain and factual, \
saying what was talked about. No bullet points, no preamble, no sign-off.\n\n\
Only what the transcript says. Nothing was decided and nobody took anything on, so do \
not write as though they had: say what the subject was and what was said about it. \
Never invent a project, a deadline, a team or a next meeting.";

const STILL_OPEN: &str = "You are checking which questions from a meeting were left unanswered.\n\n\
You get a list of questions and a list of what the meeting decided and assigned. Output only the \
questions that nothing in those lists answers, one per line, exactly as they were written. \
No numbering, no commentary. If all of them were answered, output nothing.";

fn summary_path(id: &str) -> PathBuf {
    root().join(id).join("summary.json")
}

/// Whether the transcript has been corrected since the summary was written.
///
/// Both times are recorded by this app, so the answer does not change
/// because a folder was copied or a backup restored. A summary written from
/// text that has since been fixed is quietly wrong, and the person who fixed
/// it is the one who would never know.
pub fn summary_is_stale(id: &str) -> bool {
    let Some(summary) = summary(id) else { return false };
    list().iter().find(|m| m.id == id).and_then(|m| m.edited_at).is_some_and(|at| at > summary.made_at)
}

pub fn summary(id: &str) -> Option<Summary> {
    std::fs::read_to_string(summary_path(id)).ok().and_then(|s| serde_json::from_str(&s).ok())
}

/// Collapses lines that say the same thing. Two blocks often catch one
/// decision in different words, and the merge is far safer done here than by
/// asking a small model to do it.
///
/// By shared words rather than by prefix: one real meeting produced "Legal's
/// concerns about annual contracts remain unaddressed" and "Nobody has heard
/// back from legal regarding annual contract concerns", which agree on
/// everything that matters and share no opening at all.
fn dedupe(items: Vec<String>) -> Vec<String> {
    // Words too common to count as agreement.
    const NOISE: [&str; 24] = [
        "the", "a", "an", "and", "or", "to", "of", "in", "on", "for", "is", "are", "was", "were",
        "be", "will", "that", "this", "it", "with", "by", "at", "someone", "about",
    ];
    fn words(s: &str) -> std::collections::HashSet<String> {
        s.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.len() > 2 && !NOISE.contains(w))
            .map(|w| w.trim_end_matches('s').to_string())
            .collect()
    }
    let mut kept: Vec<(String, std::collections::HashSet<String>)> = Vec::new();
    for item in items {
        let w = words(&item);
        if w.is_empty() {
            continue;
        }
        let dup = kept.iter().any(|(_, seen)| {
            let shared = w.intersection(seen).count();
            // Against the smaller of the two, so a long restatement of a
            // short point still counts as the same point.
            shared * 10 >= w.len().min(seen.len()) * 6
        });
        if !dup {
            kept.push((item, w));
        }
    }
    kept.into_iter().map(|(i, _)| i).collect()
}

/// Puts the right person in front of an extracted line.
///
/// Telling the model who was speaking is not enough: given "I will update the
/// roadmap" from someone else's mouth, both llama3.2:3b and qwen2.5:7b wrote
/// "You will update the roadmap". The transcript's own pronoun wins over any
/// instruction, so the pronoun is corrected here instead, where it is a fact
/// rather than a request.
fn attribute(item: &str, voice: &str) -> String {
    // Only the forms that stay grammatical once swapped. A bare "You " ->
    // "Someone " gave "Someone are a small startup" and "Someone want to
    // design a dog walking app": the verb does not agree, and a summary full
    // of broken English is worse than one that says "you" too often.
    let swap: &[(&str, &str)] = match voice {
        "them" => &[("You will ", "Someone will "), ("I will ", "Someone will "), ("I'll ", "Someone will ")],
        "you" => &[("Someone will ", "You will "), ("I will ", "You will "), ("I'll ", "You will ")],
        _ => return item.to_string(),
    };
    for (from, to) in swap {
        if let Some(rest) = item.strip_prefix(*from) {
            return format!("{to}{rest}");
        }
    }
    item.to_string()
}

fn strip_bullet(line: &str) -> String {
    line.trim().trim_start_matches(['-', '*', '•', ' ']).trim_start_matches(|c: char| c.is_ascii_digit()).trim_start_matches(['.', ')', ' ']).trim().to_string()
}

/// Reads a meeting's transcript and works out what it came to.
///
/// Map then reduce: every block is listed separately, then the lists are
/// merged here in code and the model is asked only for the two things code
/// cannot do - the opening paragraph, and which questions went unanswered.
fn name_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "title": { "type": "string" },
            "subtitle": { "type": "string" }
        },
        "required": ["title", "subtitle"]
    })
}

/// Cuts a model's answer down to something that fits a list row.
///
/// Small models decorate: they wrap the answer in quotes, open with "Title:",
/// and write "Meeting about the Q3 budget" when "Q3 Budget" was asked for.
/// Asking again in a firmer tone does not fix it; trimming it does. `words`
/// is a ceiling, not a target - a two word answer is a good answer.
fn tidy(line: &str, words: usize) -> String {
    let mut t = line.trim();
    for prefix in ["title:", "subtitle:", "name:"] {
        if t.to_lowercase().starts_with(prefix) {
            t = t[prefix.len()..].trim_start();
        }
    }
    let t = t.trim_matches(|c| c == '"' || c == '\'' || c == '*' || c == '#' || c == ' ');
    // One line: a model that ignores "one line" usually puts its reasoning
    // on the second.
    let t = t.lines().next().unwrap_or("").trim();
    let kept: Vec<&str> = t.split_whitespace().take(words).collect();
    kept.join(" ").trim_end_matches(['.', ',', ';', ':', '-']).to_string()
}

/// A title that would fit any meeting ever held tells you nothing the
/// timestamp beside it does not.
const EMPTY_TITLES: [&str; 10] = [
    "recent developments",
    "meeting summary",
    "meeting notes",
    "team meeting",
    "general discussion",
    "discussion",
    "various topics",
    "key points",
    "summary of discussion",
    "untitled",
];

/// The words of `line` that carry meaning, lowercased and crudely stemmed.
fn content_words(line: &str) -> Vec<String> {
    const NOISE: [&str; 28] = [
        "the", "a", "an", "and", "or", "to", "of", "in", "on", "for", "is", "are", "was", "were",
        "be", "will", "that", "this", "it", "with", "by", "at", "about", "from", "into", "their",
        "they", "our",
    ];
    line.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2 && !NOISE.contains(w))
        .map(|w| w.trim_end_matches('s').to_string())
        .collect()
}

/// Whether a name was drawn from the notes, or from somewhere else.
///
/// A small model handed a worked example in its instructions will hand the
/// example back: a real meeting about a stolen car and a trip to Greggs was
/// named "Conference spend halved to fund the search trial", word for word
/// the example the prompt gave. Taking the example out is half the fix. The
/// other half is not trusting the answer - most of what a name says has to
/// appear in the notes it was made from.
fn grounded(line: &str, notes: &str) -> bool {
    overlap(line, notes) >= 0.5
}

/// The share of a line's meaningful words that appear in its source.
fn overlap(line: &str, source: &str) -> f32 {
    let source = source.to_lowercase();
    let words = content_words(line);
    if words.is_empty() {
        return 0.0;
    }
    words.iter().filter(|w| source.contains(w.as_str())).count() as f32 / words.len() as f32
}

/// How much of an overview must come from the transcript.
///
/// Far below the bar a name has to clear, because an overview is a paraphrase
/// and is meant to use its own words. The number comes from measuring every
/// meeting on one Mac: the ones written from real notes scored 15% to 65%,
/// and the three written from nothing scored 0%, 3% and 7%. A fabricated
/// summary is not a worse summary, it is a false one, so the bar only has to
/// separate those two groups.
const OVERVIEW_FLOOR: f32 = 0.10;

/// Enough of a long transcript for a model to say what it was about.
///
/// The opening sets the subject and the end carries wherever it got to, so
/// those are the halves kept. The middle of an hour is the part an overview
/// can most afford to lose.
fn sample(text: &str, words: usize) -> String {
    let all: Vec<&str> = text.split_whitespace().collect();
    if all.len() <= words {
        return all.join(" ");
    }
    let half = words / 2;
    format!("{}\n…\n{}", all[..half].join(" "), all[all.len() - half..].join(" "))
}

/// Asks the model for a title and a subtitle, from the notes the summary
/// just produced rather than from the transcript.
///
/// The transcript is the wrong input: it is long, it costs another pass over
/// every block, and the opening minute of a call is small talk. The overview
/// alone was not enough either - it opens with its own filler, and a model
/// asked to name "The meeting discussed recent developments..." answers
/// "Recent Developments". The decisions and actions are where the nouns are.
fn name_from(s: &Settings, model: Option<&str>, overview: &str, decisions: &[String], actions: &[String]) -> Option<(String, String)> {
    let mut notes = overview.to_string();
    for line in decisions.iter().chain(actions.iter()).take(8) {
        notes.push('\n');
        notes.push_str(line);
    }
    if notes.split_whitespace().count() < 8 {
        return None;
    }
    let out = crate::engines::chat_json(s, model, NAME, &notes, "name", &name_schema(), &|| true).ok()?;
    let v: serde_json::Value = serde_json::from_str(&out).ok()?;
    let title = tidy(v["title"].as_str().unwrap_or(""), 6);
    let subtitle = tidy(v["subtitle"].as_str().unwrap_or(""), 14);
    // A one character answer is not a name, a name that could belong to any
    // meeting is no better than the date it replaces, and a name about
    // something the meeting never mentioned is worse than both.
    if title.len() < 3 || EMPTY_TITLES.contains(&title.to_lowercase().as_str()) || !grounded(&title, &notes) {
        return None;
    }
    let subtitle = if subtitle.eq_ignore_ascii_case(&title) || !grounded(&subtitle, &notes) {
        String::new()
    } else {
        subtitle
    };
    Some((title, subtitle))
}

/// The opening paragraph of a summary, and the check that it is true.
///
/// `barren` means the meeting decided nothing and assigned nothing, so the
/// notes are empty and the transcript is the only source there is.
fn write_overview(s: &Settings, model: Option<&str>, notes: &str, spoken: &str, barren: bool) -> Result<String, String> {
    let out = if barren {
        crate::engines::chat_as(s, model, OVERVIEW_TALK, &[], &sample(spoken, 350))?
    } else {
        crate::engines::chat_as(s, model, OVERVIEW, &[], notes)?
    };
    let out = out.trim().to_string();
    // A summary that is merely thin is worth keeping. One that describes a
    // meeting that did not happen is not, and it is worse than none at all
    // because it reads like the real thing.
    Ok(if overlap(&out, spoken) >= OVERVIEW_FLOOR { out } else { String::new() })
}

pub fn summarise(s: &Settings, id: &str, progress: &Mutex<String>) -> Result<Summary, String> {
    let segments = transcript(id);
    if segments.is_empty() {
        return Err("There is nothing in this meeting to summarise.".into());
    }
    let model = if s.meeting_model.trim().is_empty() { None } else { Some(s.meeting_model.trim()) };

    // Blocks break where the speaker changes as well as on length, so every
    // block has one speaker and the model is never asked to tell them apart
    // inside one. Both models ignored an inline [the others] marker and
    // followed the transcript's own "I will", putting other people's actions
    // on the user.
    let mut blocks: Vec<(String, String)> = Vec::new();
    let mut buf: Vec<String> = Vec::new();
    let mut words = 0usize;
    let mut voice = String::new();
    for seg in &segments {
        if !buf.is_empty() && (seg.voice != voice || words >= BLOCK_WORDS) {
            blocks.push((voice.clone(), buf.join(" ")));
            buf.clear();
            words = 0;
        }
        voice = seg.voice.clone();
        words += seg.text.split_whitespace().count();
        buf.push(seg.text.clone());
    }
    if !buf.is_empty() {
        blocks.push((voice, buf.join(" ")));
    }

    let (mut decisions, mut actions, mut questions) = (Vec::new(), Vec::new(), Vec::new());
    for (i, (voice, block)) in blocks.iter().enumerate() {
        if let Ok(mut p) = progress.lock() {
            *p = format!("Reading part {} of {}…", i + 1, blocks.len());
        }
        let who = match voice.as_str() {
            "you" => "Everything in this part was said by the person this summary is for.",
            "them" => "Everything in this part was said by other people on the call, not by the person this summary is for.",
            _ => "This part has several people talking.",
        };
        let user = format!("Part {} of {}. {who}\n\n{block}", i + 1, blocks.len());
        let out = crate::engines::chat_as(s, model, MAP, &[], &user)?;
        // Per block, not per meeting: a long meeting should still be able to
        // carry a long list, but no single block may run away with it.
        let (start_d, start_a, start_q) = (decisions.len(), actions.len(), questions.len());
        for line in out.lines() {
            let line = strip_bullet(line);
            let upper = line.to_uppercase();
            let (bucket, prefix, start) = if upper.starts_with("DECISION") {
                (&mut decisions, "DECISION", start_d)
            } else if upper.starts_with("ACTION") {
                (&mut actions, "ACTION", start_a)
            } else if upper.starts_with("QUESTION") {
                (&mut questions, "QUESTION", start_q)
            } else {
                continue;
            };
            let per_block_cap = start + 4;
            let item = line[prefix.len()..].trim_start_matches([':', ' ']).trim().to_string();
            // The caps and the length limit are enforced here as well as
            // asked for: one confused block of a 20 minute meeting
            // contributed its way to 53 "actions", most of them narration.
            let n = item.split_whitespace().count();
            let describing = ["the meeting", "someone explained", "someone said", "someone discussed", "it was noted"]
                .iter()
                .any(|p| item.to_lowercase().starts_with(p));
            if (2..=22).contains(&n) && !describing && bucket.len() < per_block_cap {
                bucket.push(attribute(&item, voice));
            }
        }
    }
    let decisions = dedupe(decisions);
    let actions = dedupe(actions);
    let questions = dedupe(questions);

    if let Ok(mut p) = progress.lock() {
        *p = "Writing it up…".into();
    }
    let notes = format!(
        "Decisions:\n{}\n\nActions:\n{}",
        decisions.join("\n"),
        actions.join("\n")
    );
    let spoken: String = segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");

    // A talk, a lecture or a call that settled nothing leaves both lists
    // empty, and the overview used to be written from those empty lists -
    // which is to say from nothing. A model handed nothing invents: a
    // recording about Thomas Edison was summarised as a project timeline
    // review with resource allocation and a follow-up in two weeks, none of
    // which was said. Nothing to list is not nothing to say, so when there
    // is nothing to list the transcript is what gets summarised.
    let barren = decisions.is_empty() && actions.is_empty();
    let overview = write_overview(s, model, &notes, &spoken, barren)?;
    if overview.is_empty() && barren {
        return Err("Nothing in this meeting could be summarised without making it up.".into());
    }

    // Naming it is the last thing that could fail, and the least important,
    // so a failure here costs the meeting its name and nothing else: the
    // summary is already worth keeping, and the day and time stay as the
    // title.
    if let Ok(mut p) = progress.lock() {
        *p = "Naming it…".into();
    }
    if let Some((title, subtitle)) = name_from(s, model, &overview, &decisions, &actions) {
        set_name(id, &title, &subtitle);
    }

    let open = if questions.is_empty() {
        Vec::new()
    } else {
        if let Ok(mut p) = progress.lock() {
            *p = "Checking what went unanswered…".into();
        }
        let asked = format!("Questions:\n{}\n\n{notes}", questions.join("\n"));
        let kept = crate::engines::chat_as(s, model, STILL_OPEN, &[], &asked)?;
        // Only questions that were actually asked: the filter is allowed to
        // drop, never to invent.
        let known: Vec<String> = questions.iter().map(|q| q.to_lowercase()).collect();
        kept.lines()
            .map(strip_bullet)
            .filter(|l| known.contains(&l.to_lowercase()))
            .collect()
    };

    let out = Summary {
        overview,
        decisions,
        actions,
        questions: open,
        model: model.unwrap_or(&s.ollama_model).to_string(),
        made_at: now(),
    };
    let _ = std::fs::write(summary_path(id), serde_json::to_string_pretty(&out).unwrap_or_default());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cut::Stretch;

    const RATE: f64 = 48_000.0;

    #[test]
    fn tidy_strips_what_small_models_decorate_with() {
        assert_eq!(tidy("\"Q3 Budget\"", 6), "Q3 Budget");
        assert_eq!(tidy("Title: Q3 Budget", 6), "Q3 Budget");
        assert_eq!(tidy("**Q3 Budget**", 6), "Q3 Budget");
        assert_eq!(tidy("Q3 Budget.", 6), "Q3 Budget");
    }

    #[test]
    fn tidy_caps_the_length_and_keeps_one_line() {
        assert_eq!(tidy("one two three four five six seven", 5), "one two three four five");
        assert_eq!(tidy("Hiring Plan\nHere is my reasoning", 6), "Hiring Plan");
        // A short answer is a good answer, not one to pad.
        assert_eq!(tidy("Budget", 6), "Budget");
    }

    #[test]
    fn tidy_survives_an_empty_answer() {
        assert_eq!(tidy("", 6), "");
        assert_eq!(tidy("   ", 6), "");
        assert_eq!(tidy("\"\"", 6), "");
    }

    #[test]
    fn overlap_separates_a_paraphrase_from_an_invention() {
        let spoken = "Thomas Edison and the electric light. The filament kept oxidising until \
            they found a way to stop it, and the real invention was the laboratory itself.";
        let real = "The talk went through Edison's work on the electric light and the trouble \
            with the filament oxidising, and argued his real invention was the laboratory.";
        let invented = "The meeting discussed the upcoming project timeline and deadlines. Key \
            decisions were made regarding resource allocation and task prioritization.";
        assert!(overlap(real, spoken) >= OVERVIEW_FLOOR, "{}", overlap(real, spoken));
        assert!(overlap(invented, spoken) < OVERVIEW_FLOOR, "{}", overlap(invented, spoken));
    }

    #[test]
    fn sample_keeps_both_ends_of_a_long_transcript() {
        let text: String = (0..1000).map(|i| format!("w{i} ")).collect();
        let got = sample(&text, 100);
        assert!(got.starts_with("w0 w1 "), "{}", &got[..20]);
        assert!(got.ends_with("w998 w999"), "{}", &got[got.len() - 20..]);
        assert!(got.contains('…'));
        assert!(sample("short one", 100) == "short one");
    }

    /// The exact failure a real meeting produced: a recording about a
    /// stolen car and a trip to Greggs came back named with the example
    /// sentence from the prompt, which mentioned neither.
    #[test]
    fn a_name_that_came_from_nowhere_is_rejected() {
        let notes = "The meeting discussed recent developments, including the theft of the 64 \
            sinus, with plans to transport the Bugatti and McLaren P1 to GR8 in the UK.\n\
            We'll do a Greggs run on the way back.";
        assert!(!grounded("Conference spend halved to fund the search trial", notes));
        assert!(grounded("Bugatti and McLaren going to the UK", notes));
    }

    #[test]
    fn grounded_wants_most_of_the_words_not_all_of_them() {
        let notes = "Priya takes the revised numbers to finance on Thursday.";
        // "revised numbers" and "finance" are there; "soon" is not.
        assert!(grounded("Revised numbers going to finance soon", notes));
        assert!(!grounded("Hiring freeze lifted for the design team", notes));
    }

    #[test]
    fn grounded_rejects_an_empty_line() {
        assert!(!grounded("", "anything at all"));
        assert!(!grounded("the and of", "anything at all"));
    }

    /// Run with:
    ///   cargo test --release names_a_real_meeting -- --ignored --nocapture
    /// The prompt is the risky part of naming, and only a real model shows
    /// whether it holds.
    #[test]
    #[ignore = "needs Ollama running llama3.2:3b on 127.0.0.1:11434"]
    fn names_a_real_meeting() {
        let s = Settings {
            text_engine: crate::settings::TextEngine::Ollama,
            ollama_url: "http://127.0.0.1:11434".into(),
            ollama_model: "llama3.2:3b".into(),
            ..Settings::default()
        };
        let overview = "The team went through the Q3 marketing budget and agreed to cut the \
            conference spend in half so the paid search trial can run for another quarter. \
            Priya is taking the revised numbers back to finance on Thursday, and nobody has \
            heard back from legal about the annual contracts yet.";
        let decisions = ["Conference spend halved for the quarter".to_string()];
        let actions = ["Priya takes the revised numbers to finance on Thursday".to_string()];
        let (title, subtitle) = name_from(&s, None, overview, &decisions, &actions).expect("the model should name this");
        println!("budget  title:    {title}");
        println!("budget  subtitle: {subtitle}");
        assert!((1..=6).contains(&title.split_whitespace().count()), "title was {title:?}");
        assert!(subtitle.split_whitespace().count() <= 14, "subtitle was {subtitle:?}");
        assert!(!title.to_lowercase().contains("meeting"), "the title should not say meeting: {title:?}");

        // The prompt carries an example subtitle, and a small model will
        // happily hand it back whatever it was given. A second meeting with
        // nothing to do with budgets is the only way to tell a model that
        // read the summary from one that copied the example.
        let hiring = "Two of the three candidates for the backend role were discussed and the \
            team agreed to make an offer to the second one at the top of the band. Dami is \
            writing it up tonight. The panel still cannot agree whether to keep the take-home \
            exercise for the next round.";
        let hiring_d = ["Offer goes to the second candidate at the top of the band".to_string()];
        let hiring_a = ["Dami writes up the offer tonight".to_string()];
        let (t2, s2) = name_from(&s, None, hiring, &hiring_d, &hiring_a).expect("the model should name this too");
        println!("hiring  title:    {t2}");
        println!("hiring  subtitle: {s2}");
        assert!(!s2.to_lowercase().contains("conference"), "the example leaked into the answer: {s2:?}");
        assert!(!s2.to_lowercase().contains("search trial"), "the example leaked into the answer: {s2:?}");
        assert!(!t2.eq_ignore_ascii_case(&title), "both meetings got the same name: {t2:?}");

        // The meeting that showed the bug: nothing to do with budgets or
        // hiring, and the old prompt named it after its own example.
        let car = "The meeting discussed recent developments, including the theft of the 64 \
            sinus, with plans to transport the Bugatti and McLaren P1 to GR8 in the UK. The team \
            also touched on logistical arrangements for their trip, including a planned stop at \
            Greggs on the way back.";
        let car_d = ["The Bugatti and McLaren P1 will be going to GR8 in the UK".to_string()];
        let car_a = ["We'll do a Greggs run on the way back".to_string()];
        let named = name_from(&s, None, car, &car_d, &car_a);
        println!("car     title:    {:?}", named.as_ref().map(|(t, _)| t));
        println!("car     subtitle: {:?}", named.as_ref().map(|(_, s)| s));
        if let Some((t3, s3)) = named {
            assert!(!t3.to_lowercase().contains("budget"), "named after another meeting: {t3:?}");
            assert!(!s3.to_lowercase().contains("conference"), "the old example came back: {s3:?}");
            assert!(!EMPTY_TITLES.contains(&t3.to_lowercase().as_str()), "a title that fits anything: {t3:?}");
        }
    }

    /// Run against a real stored meeting:
    ///   MEETING_ID=2026-11-11-130316 cargo test --release summarises_a_talk \
    ///     -- --ignored --nocapture
    /// Read-only: it writes nothing back.
    #[test]
    #[ignore = "needs Ollama and MEETING_ID naming a stored meeting"]
    fn summarises_a_talk_from_what_was_said() {
        let id = std::env::var("MEETING_ID").expect("set MEETING_ID");
        let spoken: String = transcript(&id).iter().map(|s| s.text.clone()).collect::<Vec<_>>().join(" ");
        assert!(!spoken.is_empty(), "no transcript for {id}");
        let s = Settings {
            text_engine: crate::settings::TextEngine::Ollama,
            ollama_url: "http://127.0.0.1:11434".into(),
            ollama_model: "llama3.2:3b".into(),
            ..Settings::default()
        };
        let overview = write_overview(&s, None, "", &spoken, true).unwrap();
        println!("overview: {overview}");
        println!("overlap:  {:.0}%", overlap(&overview, &spoken) * 100.0);
        assert!(!overview.is_empty(), "the overview was rejected as invented");
        let named = name_from(&s, None, &overview, &[], &[]);
        println!("named:    {named:?}");
    }

    #[test]
    fn editing_a_line_keeps_the_rest_of_the_transcript() {
        let dir = temp_meeting("edit-line");
        let id = dir.file_name().unwrap().to_str().unwrap().to_string();
        // root() is under the data dir, so write through the real path the
        // function will use rather than the temp one.
        let real = root().join(&id);
        std::fs::create_dir_all(&real).unwrap();
        let lines = [
            Segment { at_ms: 0, secs: 1.0, text: "Thomas Edision".into(), voice: "them".into() },
            Segment { at_ms: 1000, secs: 1.0, text: "and the light bulb".into(), voice: "you".into() },
        ];
        let body: String = lines.iter().map(|s| serde_json::to_string(s).unwrap() + "\n").collect();
        std::fs::write(real.join("transcript.jsonl"), body).unwrap();

        std::fs::write(
            real.join("meta.json"),
            serde_json::to_string(&Meta {
                id: id.clone(),
                title: "t".into(),
                subtitle: String::new(),
                edited_at: None,
                started: 0,
                ended: None,
                source: "s".into(),
                peak: 0.0,
            })
            .unwrap(),
        )
        .unwrap();

        edit_segment(&id, 0, "Thomas Edison").unwrap();
        let meta: Meta = serde_json::from_str(&std::fs::read_to_string(real.join("meta.json")).unwrap()).unwrap();
        assert!(meta.edited_at.is_some(), "the correction was not stamped");
        let got = transcript(&id);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].text, "Thomas Edison");
        assert_eq!(got[0].voice, "them", "the rest of the line survived the rewrite");
        assert_eq!(got[1].text, "and the light bulb");

        assert!(edit_segment(&id, 9999, "nowhere").is_err(), "an unknown line is refused");
        let _ = std::fs::remove_dir_all(&real);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn temp_meeting(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("opendictate-meeting-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("audio")).unwrap();
        dir
    }

    fn stretch(amplitude: f32, voiced_secs: f32) -> Stretch {
        let samples = (0..(2.0 * RATE) as usize)
            .map(|i| amplitude * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / RATE as f32).sin())
            .collect();
        Stretch { voice: "them", samples, secs: 2.0, voiced_secs, start_secs: 1.5 }
    }

    #[test]
    fn appends_a_segment_with_the_stretch_voice_to_the_transcript() {
        let dir = temp_meeting("appends");
        let seg = handle_stretch(&dir, 0, &stretch(0.3, 2.0), RATE, &|_| Ok("hello".into()))
            .unwrap()
            .unwrap();
        assert_eq!((seg.text.as_str(), seg.voice.as_str(), seg.at_ms), ("hello", "them", 1_500));
        let line = std::fs::read_to_string(dir.join("transcript.jsonl")).unwrap();
        assert_eq!(line.lines().count(), 1);
        let parsed: Segment = serde_json::from_str(line.lines().next().unwrap()).unwrap();
        assert_eq!((parsed.text.as_str(), parsed.voice.as_str()), ("hello", "them"));
        assert!(!dir.join("audio/000.wav").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn skips_a_stretch_too_quiet_to_be_speech_without_calling_whisper() {
        let dir = temp_meeting("quiet");
        let r = handle_stretch(&dir, 0, &stretch(0.01, 2.0), RATE, &|_| panic!("whisper was called"));
        assert!(matches!(r, Ok(None)));
        assert!(!dir.join("audio/000.wav").exists());
        assert!(!dir.join("transcript.jsonl").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_silent_meeting_is_not_reported_as_no_audio() {
        assert!(!no_audio_reached(48_000 * 60));
    }

    #[test]
    fn a_meeting_where_no_samples_arrive_is_reported_as_no_audio() {
        assert!(no_audio_reached(0));
    }

    #[test]
    fn reports_a_transcription_failure_and_still_deletes_the_audio() {
        let dir = temp_meeting("fails");
        let wav = dir.join("audio/000.wav");
        let r = handle_stretch(&dir, 0, &stretch(0.3, 2.0), RATE, &|_| {
            assert!(wav.exists(), "audio was not written before transcribing");
            Err("boom".into())
        });
        assert_eq!(r.unwrap_err(), "boom");
        assert!(!dir.join("audio/000.wav").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn lm_studio_settings() -> Settings {
        Settings {
            talking_points: true,
            text_engine: settings::TextEngine::OpenAiCompatible,
            text_url: "http://localhost:1234/v1".into(),
            ..Settings::default()
        }
    }

    #[test]
    fn lm_studio_is_not_started_on_a_mac_under_16_gb() {
        assert!(!should_start_lm_studio(&lm_studio_settings(), 8 * 1024 * 1024 * 1024, true));
    }

    #[test]
    fn lm_studio_is_started_when_talking_points_is_ready_and_lms_is_installed() {
        assert!(should_start_lm_studio(&lm_studio_settings(), 16 * 1024 * 1024 * 1024, true));
        assert!(!should_start_lm_studio(&lm_studio_settings(), 16 * 1024 * 1024 * 1024, false));
    }

    #[test]
    fn lm_studio_is_not_started_when_talking_points_is_off() {
        let s = Settings { talking_points: false, ..lm_studio_settings() };
        assert!(!should_start_lm_studio(&s, 16 * 1024 * 1024 * 1024, true));
    }
}
