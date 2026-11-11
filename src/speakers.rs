//! Telling the people on the far side of a call apart by their voices.
//!
//! The far side arrives as one mixed stream, so a transcript says "them" for
//! everyone who is not at this microphone. This describes each second or so
//! of that stream as a voice, groups the voices into as many people as
//! somebody says were there, and votes them into the transcript's lines, so
//! "them" can become Speaker 1 and Speaker 2 and, once somebody has named
//! them, a name.
//!
//! **What this is built on, and what it replaced.** The first attempt used
//! the engine's own diarization, which finds turns and clusters them. It
//! produced clean-looking alternation, and I called that separation. It was
//! not: the recording was two podcast hosts, one male and one female, and
//! pitch is a signal the models never see. Scored against a pitch split of
//! the same audio over 197 windows, that diarization agreed on 54.9% of them,
//! where always guessing the louder voice would have scored 70.6%.
//!
//! Embedding short windows and grouping them ourselves agreed with pitch on
//! 93.9%. Complete linkage and k-means landed within two points of that, so
//! it is not one lucky algorithm, and clustering a third of the windows and
//! assigning the rest cost nothing (92.4% to 93.4% from 33 to 99 windows),
//! which is what keeps a long meeting cheap. `experiments/speaker-pitch.py`
//! is the measurement.
//!
//! **What that does not show.** One recording, and the easiest one: two
//! voices an octave apart. Two speakers of the same sex and similar pitch are
//! the case pitch cannot referee, and there is no evidence either way about
//! them here. And the number of people is still something the person says:
//! left to count, the engine's own clustering found fourteen in a recording
//! of two.

use serde::{Deserialize, Serialize};
use sherpa_onnx::{SpeakerEmbeddingExtractor, SpeakerEmbeddingExtractorConfig};
use std::path::{Path, PathBuf};

use crate::meeting::Segment;

/// What the model wants, and what a meeting's stretches are written at.
pub const SAMPLE_RATE: u32 = 16_000;

/// One window of audio is described at a time. Long enough to carry a voice,
/// short enough that a turn is rarely shorter than one.
const WINDOW_SECS: f32 = 1.5;
/// Half a window, so a change of speaker lands mostly inside some window.
const HOP_SECS: f32 = 0.75;
/// A stretch shorter than a window gets one window if it is at least this
/// long. Under it a vector says more about the word than about the speaker.
const SHORTEST_WINDOW_SECS: f32 = 1.0;
/// Windows quieter than this are not described: room noise has a voice too,
/// and it is the same one every time.
const QUIET_RMS: f32 = 0.005;
/// Average linkage is cubic, so a long meeting is grouped from an even
/// sample of its windows and the rest are assigned to the nearest centre.
const GROUP_AT_MOST: usize = 400;
/// A speaker is credited with a line when they hold at least this much of it.
const MIN_SHARE: f32 = 0.25;
/// More than this and "how many people were there" has no sensible answer.
pub const MAX_SPEAKERS: usize = 6;

/// A model file, pinned.
///
/// Native code parses it, so it is not taken on trust: the size and SHA-256
/// of what was tested are written down here, and a download that does not
/// match is thrown away instead of loaded. If upstream ever replaces the
/// file, this fails loudly and safely rather than quietly running something
/// nobody has looked at.
struct Pinned {
    url: &'static str,
    file: &'static str,
    bytes: u64,
    sha256: &'static str,
}

/// Describes a voice as a vector, so two can be compared.
const MODEL: Pinned = Pinned {
    url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx",
    file: "campplus-voxceleb-en-16k.onnx",
    bytes: 29_596_978,
    sha256: "357a834f702b80161e5b981182c038e18553c1f2ca752ed6cec2052365d4129b",
};

pub fn models_dir() -> PathBuf {
    let dir = crate::settings::models_dir().join("speakers");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// The model, if it is there and is the size it should be. By size only:
/// hashing 30 MB on every look would be the wrong place to spend it, and the
/// hash was checked when the file arrived.
pub fn model_path() -> Option<PathBuf> {
    ready_in(&models_dir())
}

fn ready_in(dir: &Path) -> Option<PathBuf> {
    let path = dir.join(MODEL.file);
    std::fs::metadata(&path).is_ok_and(|f| f.len() == MODEL.bytes).then_some(path)
}

fn sha256_of(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Fetches the model into `dir`, reporting (bytes so far, bytes in all) as it
/// goes, and lets it into place only once it matches the pin.
fn download_into(dir: &Path, mut report: impl FnMut(u64, u64)) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let dest = dir.join(MODEL.file);
    if ready_in(dir).is_some() {
        return Ok(());
    }
    let part = dest.with_extension("part");
    crate::engines::fetch_part(MODEL.url, &part, |done, _| report(done, MODEL.bytes))?;
    let got = std::fs::metadata(&part).map(|f| f.len()).unwrap_or(0);
    let hash = if got == MODEL.bytes { sha256_of(&part)? } else { String::new() };
    if got != MODEL.bytes || hash != MODEL.sha256 {
        let _ = std::fs::remove_file(&part);
        return Err(format!(
            "The voice model did not download correctly ({got} bytes, expected {}). Nothing was installed; try again.",
            MODEL.bytes
        ));
    }
    std::fs::rename(&part, &dest).map_err(|e| e.to_string())
}

/// Starts the download on a thread, in the shape the Models tab already uses.
pub fn download(progress: crate::engines::SharedProgress, ctx: eframe::egui::Context) {
    std::thread::spawn(move || {
        let result = download_into(&models_dir(), |done, total| {
            let mut p = progress.lock().unwrap();
            p.done = done;
            p.total = total;
            p.status = "voice model".into();
            ctx.request_repaint();
        });
        let mut p = progress.lock().unwrap();
        p.finished = true;
        p.error = result.err();
        ctx.request_repaint();
    });
}

/// Size of the download, so the bar has a total before the first byte.
pub const DOWNLOAD_BYTES: u64 = MODEL.bytes;

// ---------------------------------------------------------------------------
// Describing voices

/// Describes stretches of audio as unit vectors.
pub struct Embedder {
    extractor: SpeakerEmbeddingExtractor,
}

impl Embedder {
    pub fn new(model: &Path) -> Result<Self, String> {
        let config = SpeakerEmbeddingExtractorConfig {
            model: Some(model.to_str().ok_or("The voice model path is not valid text")?.to_owned()),
            num_threads: 2,
            ..Default::default()
        };
        SpeakerEmbeddingExtractor::create(&config)
            .map(|extractor| Self { extractor })
            .ok_or_else(|| "Could not load the voice model. It may be missing or damaged.".to_string())
    }

    /// The vector for `samples` (16 kHz mono), or None when there is too
    /// little audio to describe.
    pub fn embed(&self, samples: &[f32]) -> Option<Vec<f32>> {
        let stream = self.extractor.create_stream()?;
        stream.accept_waveform(SAMPLE_RATE as i32, samples);
        stream.input_finished();
        if !self.extractor.is_ready(&stream) {
            return None;
        }
        let v = self.extractor.compute(&stream)?;
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        (norm > 0.0).then(|| v.iter().map(|x| x / norm).collect())
    }

    pub fn dim(&self) -> usize {
        self.extractor.dim() as usize
    }
}

fn rms(v: &[f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt()
}

/// Where the windows over a stretch of `len` samples start, and how long each
/// is. Pure, so it can be tested without a model.
fn window_spans(len: usize) -> Vec<(usize, usize)> {
    let rate = SAMPLE_RATE as f32;
    let (win, hop) = ((WINDOW_SECS * rate) as usize, (HOP_SECS * rate) as usize);
    if len < win {
        return if len as f32 >= SHORTEST_WINDOW_SECS * rate { vec![(0, len)] } else { Vec::new() };
    }
    let mut spans: Vec<(usize, usize)> = (0..=len - win).step_by(hop).map(|s| (s, win)).collect();
    // The tail, so the end of a stretch is not the one part nobody listened to.
    if spans.last().is_some_and(|&(s, _)| s + win < len) {
        spans.push((len - win, win));
    }
    spans
}

/// One described window: when it began in the meeting, and its vector.
struct Described {
    at_ms: u64,
    embedding: Vec<f32>,
}

/// Describes the windows of one stretch that begins `stretch_ms` into the
/// meeting.
fn describe_stretch(embedder: &Embedder, samples: &[f32], stretch_ms: u64, out: &mut Vec<Described>) {
    for (start, len) in window_spans(samples.len()) {
        let window = &samples[start..start + len];
        if rms(window) < QUIET_RMS {
            continue;
        }
        if let Some(embedding) = embedder.embed(window) {
            out.push(Described { at_ms: stretch_ms + (start as u64 * 1000 / SAMPLE_RATE as u64), embedding });
        }
    }
}

// ---------------------------------------------------------------------------
// Grouping them

/// Groups `voices` (unit vectors laid end to end, `dim` numbers each, in time
/// order) into `k` people, and says which person each belongs to. People are
/// numbered by first appearance, so Speaker 1 is whoever spoke first and the
/// numbering does not move if the same meeting is grouped again.
///
/// The grouping is average linkage on cosine similarity over an even sample
/// of at most [`GROUP_AT_MOST`] voices; every voice, sampled or not, is then
/// assigned to the nearest of the resulting centres.
pub fn group(voices: &[f32], dim: usize, k: usize) -> Vec<usize> {
    if dim == 0 || voices.len() < dim {
        return Vec::new();
    }
    let n = voices.len() / dim;
    let at = |i: usize| &voices[i * dim..(i + 1) * dim];
    let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();

    let step = n.div_ceil(GROUP_AT_MOST).max(1);
    let sample: Vec<usize> = (0..n).step_by(step).collect();
    let m = sample.len();
    let k = k.clamp(1, m);

    let mut sim = vec![vec![0f32; m]; m];
    for a in 0..m {
        for b in a + 1..m {
            let d = dot(at(sample[a]), at(sample[b]));
            sim[a][b] = d;
            sim[b][a] = d;
        }
    }
    let mut members: Vec<Vec<usize>> = (0..m).map(|i| vec![i]).collect();
    let mut alive = vec![true; m];
    let mut count = m;
    while count > k {
        let (mut best, mut pair) = (f32::MIN, (0, 0));
        for a in (0..m).filter(|&a| alive[a]) {
            for b in (a + 1..m).filter(|&b| alive[b]) {
                if sim[a][b] > best {
                    best = sim[a][b];
                    pair = (a, b);
                }
            }
        }
        let (a, b) = pair;
        let (na, nb) = (members[a].len() as f32, members[b].len() as f32);
        for c in (0..m).filter(|&c| alive[c] && c != a && c != b) {
            let merged = (na * sim[a][c] + nb * sim[b][c]) / (na + nb);
            sim[a][c] = merged;
            sim[c][a] = merged;
        }
        let moved = std::mem::take(&mut members[b]);
        members[a].extend(moved);
        alive[b] = false;
        count -= 1;
    }

    let centres: Vec<Vec<f32>> = members
        .iter()
        .enumerate()
        .filter(|(i, _)| alive[*i])
        .map(|(_, group)| {
            let mut c = vec![0f32; dim];
            for &g in group {
                for (slot, x) in c.iter_mut().zip(at(sample[g])) {
                    *slot += x;
                }
            }
            let norm = c.iter().map(|x| x * x).sum::<f32>().sqrt().max(f32::MIN_POSITIVE);
            c.iter().map(|x| x / norm).collect()
        })
        .collect();

    let raw: Vec<usize> = (0..n)
        .map(|i| {
            (0..centres.len())
                .max_by(|&a, &b| dot(at(i), &centres[a]).total_cmp(&dot(at(i), &centres[b])))
                .unwrap_or(0)
        })
        .collect();

    let mut order: Vec<usize> = Vec::new();
    raw.iter()
        .map(|r| match order.iter().position(|o| o == r) {
            Some(p) => p,
            None => {
                order.push(*r);
                order.len() - 1
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// What a meeting keeps

/// Who spoke when, on the far side of one meeting, and what they are called.
///
/// What is kept is the voices and not the audio: a vector per window instead
/// of 1.9 MB a minute. That is what lets a corrected head count be grouped
/// again long after the recording was deleted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Speakers {
    /// How many people were grouped.
    pub count: usize,
    /// What each is called, by number; empty means "Speaker N".
    #[serde(default)]
    pub names: Vec<String>,
    pub dim: usize,
    /// Meeting time each window began, ascending.
    pub at_ms: Vec<u64>,
    /// Which person each window belongs to.
    pub labels: Vec<u8>,
    /// The vectors, laid end to end. Kept beside this in `voices.f32`, not in
    /// the JSON, which would be several times the size and slow to read.
    #[serde(skip)]
    voices: Vec<f32>,
}

impl Speakers {
    pub fn name(&self, speaker: usize) -> String {
        match self.names.get(speaker).map(|n| n.trim()).filter(|n| !n.is_empty()) {
            Some(n) => n.to_string(),
            None => format!("Speaker {}", speaker + 1),
        }
    }

    /// Whether this person has been given a name, as opposed to a number.
    pub fn is_named(&self, speaker: usize) -> bool {
        self.names.get(speaker).is_some_and(|n| !n.trim().is_empty())
    }

    pub fn rename(&mut self, speaker: usize, name: &str) {
        if speaker >= self.count {
            return;
        }
        self.names.resize(self.count, String::new());
        self.names[speaker] = name.trim().to_string();
    }

    /// How much of the far side's speech each person has, as a share.
    pub fn shares(&self) -> Vec<f32> {
        let mut counts = vec![0usize; self.count];
        for &l in &self.labels {
            if let Some(c) = counts.get_mut(l as usize) {
                *c += 1;
            }
        }
        let total = counts.iter().sum::<usize>().max(1) as f32;
        counts.into_iter().map(|c| c as f32 / total).collect()
    }

    /// Groups the stored voices again into `k` people. Names go with the
    /// person who kept them: a name survives only if the voices it was given
    /// to still make up most of one new person, and that person is most of
    /// them. Anything murkier loses its name, because a wrong name on a
    /// summary is worse than none.
    pub fn regroup(&mut self, k: usize) {
        let k = k.clamp(1, MAX_SPEAKERS);
        let new: Vec<usize> = group(&self.voices, self.dim, k);
        if new.len() != self.labels.len() {
            return;
        }
        let new_count = new.iter().copied().max().map_or(0, |m| m + 1);
        let old_count = self.count.max(1);
        let mut overlap = vec![vec![0usize; new_count]; old_count];
        let (mut old_total, mut new_total) = (vec![0usize; old_count], vec![0usize; new_count]);
        for (o, n) in self.labels.iter().zip(&new) {
            let o = (*o as usize).min(old_count - 1);
            overlap[o][*n] += 1;
            old_total[o] += 1;
            new_total[*n] += 1;
        }
        let mut names = vec![String::new(); new_count];
        for (o, row) in overlap.iter().enumerate() {
            let Some((n, &shared)) = row.iter().enumerate().max_by_key(|(_, c)| **c) else { continue };
            let given = self.names.get(o).map(|s| s.trim()).unwrap_or("");
            let mostly_new = shared * 10 >= new_total[n] * 6;
            let mostly_old = shared * 10 >= old_total[o] * 6;
            if !given.is_empty() && mostly_new && mostly_old && names[n].is_empty() {
                names[n] = given.to_string();
            }
        }
        self.count = new_count;
        self.labels = new.into_iter().map(|n| n as u8).collect();
        self.names = names;
    }

    /// Who spoke each of `segments`, in order of how much of it they spoke.
    /// Only far-side lines are labelled: "you" is already you. A line with no
    /// windows in it - too short, too quiet, or audio that was never kept - is
    /// left alone.
    pub fn lines(&self, segments: &[Segment]) -> Vec<Vec<usize>> {
        segments
            .iter()
            .map(|seg| {
                if seg.voice != "them" {
                    return Vec::new();
                }
                let (from, to) = (seg.at_ms, seg.at_ms + (seg.secs * 1000.0) as u64);
                let a = self.at_ms.partition_point(|&t| t < from);
                let b = self.at_ms.partition_point(|&t| t < to);
                if a >= b || b > self.labels.len() {
                    return Vec::new();
                }
                let mut counts = vec![0usize; self.count];
                for &l in &self.labels[a..b] {
                    if let Some(c) = counts.get_mut(l as usize) {
                        *c += 1;
                    }
                }
                let total = (b - a) as f32;
                let mut who: Vec<(usize, usize)> = counts
                    .iter()
                    .enumerate()
                    .map(|(i, &c)| (i, c))
                    .filter(|&(_, c)| c as f32 / total >= MIN_SHARE)
                    .collect();
                who.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0)));
                who.into_iter().map(|(i, _)| i).collect()
            })
            .collect()
    }

    fn files(dir: &Path) -> (PathBuf, PathBuf) {
        (dir.join("speakers.json"), dir.join("voices.f32"))
    }

    /// Writes both files, each through a temporary and renamed into place so a
    /// failure cannot leave half a meeting's speakers behind.
    fn save_to(&self, dir: &Path) -> Result<(), String> {
        let (json, bin) = Self::files(dir);
        let mut raw = Vec::with_capacity(self.voices.len() * 4);
        for v in &self.voices {
            raw.extend_from_slice(&v.to_le_bytes());
        }
        let tmp_bin = bin.with_extension("f32.new");
        std::fs::write(&tmp_bin, raw).map_err(|e| format!("Could not save the voices: {e}"))?;
        std::fs::rename(&tmp_bin, &bin).map_err(|e| format!("Could not save the voices: {e}"))?;
        let tmp_json = json.with_extension("json.new");
        std::fs::write(&tmp_json, serde_json::to_string(self).map_err(|e| e.to_string())?)
            .map_err(|e| format!("Could not save the speakers: {e}"))?;
        std::fs::rename(&tmp_json, &json).map_err(|e| format!("Could not save the speakers: {e}"))
    }

    fn load_from(dir: &Path) -> Option<Self> {
        let (json, bin) = Self::files(dir);
        let mut s: Self = serde_json::from_str(&std::fs::read_to_string(json).ok()?).ok()?;
        let raw = std::fs::read(bin).ok()?;
        s.voices = raw.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
        // Refuse a meeting whose files disagree rather than label from them.
        (s.dim > 0 && s.voices.len() == s.at_ms.len() * s.dim && s.labels.len() == s.at_ms.len()).then_some(s)
    }

    pub fn load(id: &str) -> Option<Self> {
        Self::load_from(&crate::meeting::root().join(id))
    }

    pub fn save(&self, id: &str) -> Result<(), String> {
        self.save_to(&crate::meeting::root().join(id))
    }
}

/// Describes every kept far-side stretch of a meeting, groups the voices into
/// `count` people and stores the result beside the transcript.
///
/// Needs the stretches' audio, which a meeting only keeps while it is being
/// labelled; see `handle_stretch`. `report` is told how far along it is.
pub fn label_meeting(id: &str, count: usize, model: &Path, report: &dyn Fn(&str)) -> Result<Speakers, String> {
    let embedder = Embedder::new(model)?;
    let audio = crate::meeting::root().join(id).join("audio");
    let far: Vec<Segment> = crate::meeting::transcript(id).into_iter().filter(|s| s.voice == "them").collect();
    let files: Vec<(u64, PathBuf)> = far
        .iter()
        .map(|s| (s.at_ms, audio.join(crate::meeting::stretch_file(s.at_ms))))
        .filter(|(_, p)| p.is_file())
        .collect();
    if files.is_empty() {
        return Err("There is no far-side audio to tell apart. It is only kept while a meeting is recorded with speaker labels on.".into());
    }
    let described = describe_files(&embedder, &files, report)?;
    finish(described, embedder.dim(), count, |s| s.save(id))
}

/// Tells a finished meeting's voices apart and then lets go of its audio.
///
/// The audio goes whether or not the labelling worked. It was only ever held
/// for this, an hour of it is more than a hundred megabytes, and a failure is
/// not a reason to keep a recording of other people that nobody asked to
/// keep. Unless they did: `keep` is the "keep meeting audio" setting.
pub fn label_and_release(id: &str, count: usize, model: &Path, keep: bool, report: &dyn Fn(&str)) -> Result<Speakers, String> {
    let out = label_meeting(id, count, model, report);
    if !keep {
        crate::meeting::clear_audio(id);
    }
    out
}

fn describe_files(embedder: &Embedder, files: &[(u64, PathBuf)], report: &dyn Fn(&str)) -> Result<Vec<Described>, String> {
    let mut out = Vec::new();
    for (i, (at_ms, path)) in files.iter().enumerate() {
        report(&format!("Listening to the voices… {} of {}", i + 1, files.len()));
        describe_stretch(embedder, &read_wav(path)?, *at_ms, &mut out);
    }
    out.sort_by_key(|d| d.at_ms);
    Ok(out)
}

fn finish(described: Vec<Described>, dim: usize, count: usize, save: impl Fn(&Speakers) -> Result<(), String>) -> Result<Speakers, String> {
    if described.is_empty() {
        return Err("No one spoke clearly enough on the far side to tell voices apart.".into());
    }
    let voices: Vec<f32> = described.iter().flat_map(|d| d.embedding.iter().copied()).collect();
    let labels = group(&voices, dim, count.clamp(1, MAX_SPEAKERS));
    let people = labels.iter().copied().max().map_or(0, |m| m + 1);
    let speakers = Speakers {
        count: people,
        names: vec![String::new(); people],
        dim,
        at_ms: described.iter().map(|d| d.at_ms).collect(),
        labels: labels.into_iter().map(|l| l as u8).collect(),
        voices,
    };
    save(&speakers)?;
    Ok(speakers)
}

/// Reads a 16-bit mono WAV, which is what a meeting writes its stretches as.
/// Anything else is refused rather than guessed at: a model fed the wrong
/// rate does not fail, it returns confident nonsense.
pub fn read_wav(path: &Path) -> Result<Vec<f32>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(format!("{}: not a WAV file", path.display()));
    }
    let (mut at, mut format) = (12usize, None);
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let len = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]]) as usize;
        let body = at + 8;
        if id == b"fmt " && body + 16 <= bytes.len() {
            let channels = u16::from_le_bytes([bytes[body + 2], bytes[body + 3]]);
            let rate = u32::from_le_bytes([bytes[body + 4], bytes[body + 5], bytes[body + 6], bytes[body + 7]]);
            let bits = u16::from_le_bytes([bytes[body + 14], bytes[body + 15]]);
            format = Some((channels, rate, bits));
        } else if id == b"data" {
            let (channels, rate, bits) = format.ok_or_else(|| format!("{}: data before format", path.display()))?;
            if channels != 1 || bits != 16 || rate != SAMPLE_RATE {
                return Err(format!(
                    "{}: expected 16-bit mono at {SAMPLE_RATE} Hz, got {bits}-bit {channels}ch at {rate} Hz",
                    path.display()
                ));
            }
            let end = (body + len).min(bytes.len());
            return Ok(bytes[body..end]
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
                .collect());
        }
        at = body + len + (len & 1);
    }
    Err(format!("{}: no audio in it", path.display()))
}

/// `--speakers-check <dir> --embedding <onnx> --speakers N`: groups whatever
/// stretch audio is in `<dir>/audio`, in file-name order, and says what came
/// out. A diagnostic, in the manner of `--tap-check`.
pub fn check(args: &[String]) -> String {
    let flag = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let (Some(dir), Some(model)) = (flag("--speakers-check"), flag("--embedding")) else {
        return "usage: --speakers-check <meeting dir> --embedding <onnx> [--speakers N]\n".into();
    };
    let k = flag("--speakers").and_then(|n| n.parse::<usize>().ok()).unwrap_or(2);
    let audio = Path::new(&dir).join("audio");
    let mut wavs: Vec<PathBuf> = std::fs::read_dir(&audio)
        .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "wav")).collect())
        .unwrap_or_default();
    wavs.sort();
    if wavs.is_empty() {
        return format!("No stretch audio in {}. Record with Keep meeting audio switched on.\n", audio.display());
    }
    let embedder = match Embedder::new(Path::new(&model)) {
        Ok(e) => e,
        Err(e) => return format!("{e}\n"),
    };
    // Stretches are named by when they began; older ones by number, in which
    // case their order is all there is to go on.
    let mut clock = 0u64;
    let mut files = Vec::new();
    for w in &wavs {
        let stem = w.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let at = stem.parse::<u64>().ok().filter(|_| stem.len() == 9).unwrap_or(clock);
        let secs = read_wav(w).map(|s| s.len() as u64 * 1000 / SAMPLE_RATE as u64).unwrap_or(0);
        files.push((at, w.clone()));
        clock = at + secs;
    }
    let started = std::time::Instant::now();
    let described = match describe_files(&embedder, &files, &|_| {}) {
        Ok(d) => d,
        Err(e) => return format!("{e}\n"),
    };
    let n = described.len();
    let mut out = format!("{} stretches, {n} windows described in {:.1}s\n", files.len(), started.elapsed().as_secs_f32());
    let voices: Vec<f32> = described.iter().flat_map(|d| d.embedding.iter().copied()).collect();
    let labels = group(&voices, embedder.dim(), k);
    let mut counts = vec![0usize; labels.iter().copied().max().map_or(0, |m| m + 1)];
    for l in &labels {
        counts[*l] += 1;
    }
    out.push_str(&format!("grouped into {} people\n", counts.len()));
    for (i, c) in counts.iter().enumerate() {
        out.push_str(&format!("  speaker {}: {:3.0}% of windows\n", i + 1, *c as f32 / n.max(1) as f32 * 100.0));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("opendictate-speakers-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn temp_wav(name: &str, rate: u32, channels: u16, samples: &[i16]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("opendictate-speakers-{name}-{}.wav", std::process::id()));
        let data_len = (samples.len() * 2) as u32;
        let mut w = Vec::new();
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + data_len).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&channels.to_le_bytes());
        w.extend_from_slice(&rate.to_le_bytes());
        w.extend_from_slice(&(rate * 2 * channels as u32).to_le_bytes());
        w.extend_from_slice(&(2 * channels).to_le_bytes());
        w.extend_from_slice(&16u16.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&data_len.to_le_bytes());
        for s in samples {
            w.extend_from_slice(&s.to_le_bytes());
        }
        std::fs::write(&path, w).unwrap();
        path
    }

    /// `n` unit vectors of `dim` numbers: even ones lean one way, odd ones the
    /// other, each knocked about a little by a deterministic wobble.
    fn two_voices(n: usize, dim: usize) -> Vec<f32> {
        let mut out = Vec::new();
        for i in 0..n {
            let mut v = vec![0f32; dim];
            v[if i % 2 == 0 { 0 } else { 1 }] = 1.0;
            for (j, x) in v.iter_mut().enumerate().skip(2) {
                *x = (((i * 31 + j * 17) % 13) as f32 - 6.0) * 0.01;
            }
            let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            out.extend(v.iter().map(|x| x / norm));
        }
        out
    }

    // -- audio ------------------------------------------------------------

    #[test]
    fn reads_what_a_meeting_writes() {
        let p = temp_wav("ok", 16_000, 1, &[0, 16384, -16384, 32767]);
        let got = read_wav(&p).unwrap();
        assert_eq!(got.len(), 4);
        assert!((got[1] - 0.5).abs() < 1e-4 && (got[2] + 0.5).abs() < 1e-4);
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn refuses_audio_at_the_wrong_rate_instead_of_guessing() {
        let p = temp_wav("rate", 48_000, 1, &[0; 8]);
        assert!(read_wav(&p).unwrap_err().contains("48000"), "a model fed the wrong rate returns nonsense, not an error");
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn refuses_stereo() {
        let p = temp_wav("stereo", 16_000, 2, &[0; 8]);
        assert!(read_wav(&p).is_err());
        let _ = std::fs::remove_file(p);
    }

    // -- windows ----------------------------------------------------------

    #[test]
    fn windows_cover_a_stretch_and_its_tail() {
        let rate = SAMPLE_RATE as usize;
        let spans = window_spans(5 * rate);
        assert_eq!(spans[0], (0, (WINDOW_SECS * rate as f32) as usize));
        let (last_start, last_len) = *spans.last().unwrap();
        assert_eq!(last_start + last_len, 5 * rate, "the end of a stretch is listened to as well");
        assert!(spans.windows(2).all(|w| w[0].0 < w[1].0), "in order, none repeated");
    }

    #[test]
    fn a_short_stretch_gets_one_window_and_a_tiny_one_none() {
        let rate = SAMPLE_RATE as usize;
        assert_eq!(window_spans(rate + rate / 4), vec![(0, rate + rate / 4)], "1.25 s is one window");
        assert!(window_spans(rate / 2).is_empty(), "half a second says more about the word than the speaker");
    }

    // -- grouping ---------------------------------------------------------

    #[test]
    fn two_voices_come_apart() {
        let v = two_voices(40, 8);
        let labels = group(&v, 8, 2);
        assert_eq!(labels.len(), 40);
        assert!((0..40).step_by(2).all(|i| labels[i] == labels[0]), "one voice stays one person");
        assert!((1..40).step_by(2).all(|i| labels[i] == labels[1]));
        assert_ne!(labels[0], labels[1]);
    }

    #[test]
    fn people_are_numbered_by_first_appearance() {
        // B speaks first, then A, then B again: B is Speaker 1 whatever the
        // clustering happened to meet first.
        let mut v = Vec::new();
        for who in [1usize, 0, 1, 0, 1, 1, 0] {
            let mut x = vec![0f32; 4];
            x[who] = 1.0;
            v.extend(x);
        }
        assert_eq!(group(&v, 4, 2), vec![0, 1, 0, 1, 0, 0, 1]);
    }

    #[test]
    fn a_long_meeting_is_grouped_from_a_sample_and_everything_is_still_assigned() {
        let n = GROUP_AT_MOST * 4 + 7;
        let v = two_voices(n, 8);
        let labels = group(&v, 8, 2);
        assert_eq!(labels.len(), n, "every window gets someone, sampled or not");
        let right = (0..n).filter(|&i| labels[i] == labels[i % 2]).count();
        assert_eq!(right, n, "and the sample finds the same two people the whole would");
    }

    #[test]
    fn asking_for_more_people_than_there_are_windows_does_not_panic() {
        let v = two_voices(3, 8);
        assert_eq!(group(&v, 8, 6).len(), 3);
        assert!(group(&[], 8, 2).is_empty());
        assert!(group(&v, 0, 2).is_empty());
    }

    // -- a meeting's speakers ----------------------------------------------

    fn meeting(count: usize) -> Speakers {
        // Windows every 750 ms for 30 s; the first 10 s one person, then
        // the other, then the first again.
        let at_ms: Vec<u64> = (0..40).map(|i| i * 750).collect();
        let labels: Vec<u8> = at_ms.iter().map(|&t| if (10_000..20_000).contains(&t) { 1 } else { 0 }).collect();
        Speakers { count, names: vec![String::new(); count], dim: 2, at_ms, labels, voices: Vec::new() }
    }

    fn line(at_ms: u64, secs: f32, voice: &str) -> Segment {
        Segment { at_ms, secs, text: "x".into(), voice: voice.into() }
    }

    #[test]
    fn a_line_goes_to_whoever_spoke_it() {
        let s = meeting(2);
        let got = s.lines(&[line(0, 5.0, "them"), line(11_000, 5.0, "them")]);
        assert_eq!(got[0], vec![0]);
        assert_eq!(got[1], vec![1]);
    }

    #[test]
    fn a_line_with_two_voices_in_it_names_both_the_louder_first() {
        let s = meeting(2);
        // 7 s to 14 s: 3 s of the first person, 4 s of the second.
        let got = s.lines(&[line(7_000, 7.0, "them")]);
        assert_eq!(got[0], vec![1, 0]);
    }

    #[test]
    fn lines_that_are_yours_or_have_no_windows_are_left_unlabelled() {
        let s = meeting(2);
        let got = s.lines(&[line(0, 5.0, "you"), line(0, 5.0, "both"), line(500_000, 3.0, "them")]);
        assert!(got.iter().all(|g| g.is_empty()));
    }

    #[test]
    fn a_speaker_with_a_sliver_of_a_line_is_not_credited_with_it() {
        let s = meeting(2);
        // 9.75 s to 15 s holds seven windows: one belongs to the first person,
        // six to the second. One in seven is under a quarter, so it is a
        // sliver of a word at the edge of the line and not a speaker in it.
        let got = s.lines(&[line(9_750, 5.25, "them")]);
        assert_eq!(got[0], vec![1], "a sliver is not a speaker");

        // Whereas three in seven is a real share, and both are named.
        let got = s.lines(&[line(8_250, 5.25, "them")]);
        assert_eq!(got[0], vec![1, 0], "three windows of seven is a speaker");
    }

    #[test]
    fn a_name_is_what_you_called_them_and_otherwise_a_number() {
        let mut s = meeting(2);
        assert_eq!(s.name(0), "Speaker 1");
        s.rename(1, "  Priya ");
        assert_eq!(s.name(1), "Priya");
        assert!(s.is_named(1) && !s.is_named(0));
        s.rename(1, "   ");
        assert_eq!(s.name(1), "Speaker 2", "clearing a name puts the number back");
        s.rename(9, "nobody");
        assert_eq!(s.count, 2, "renaming someone who is not there changes nothing");
    }

    #[test]
    fn shares_add_up() {
        let s = meeting(2);
        let shares = s.shares();
        assert!((shares.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        assert!(shares[0] > shares[1]);
    }

    #[test]
    fn regrouping_keeps_a_name_only_with_the_voice_it_was_given_to() {
        let dim = 4;
        let mut voices = Vec::new();
        let mut at_ms = Vec::new();
        for i in 0..60u64 {
            let mut x = vec![0f32; dim];
            x[if i % 3 == 0 { 0 } else { 1 }] = 1.0;
            voices.extend(x);
            at_ms.push(i * 750);
        }
        let labels: Vec<u8> = group(&voices, dim, 2).into_iter().map(|l| l as u8).collect();
        let mut s = Speakers { count: 2, names: vec!["Priya".into(), String::new()], dim, labels, at_ms, voices };
        // Speaker 1 is whoever spoke first, and holds the name.
        s.regroup(2);
        assert_eq!(s.count, 2);
        assert_eq!(s.name(0), "Priya", "the same two people, so the same name");
        s.regroup(1);
        assert_eq!(s.count, 1);
        assert_eq!(s.name(0), "Speaker 1", "two people folded into one is nobody's name");
    }

    #[test]
    fn it_survives_being_written_and_read_back() {
        let dir = temp_dir("roundtrip");
        let mut s = meeting(2);
        s.voices = (0..s.at_ms.len() * 2).map(|i| i as f32 * 0.5).collect();
        s.names = vec!["Priya".into(), String::new()];
        s.save_to(&dir).unwrap();
        let back = Speakers::load_from(&dir).unwrap();
        assert_eq!(back, s);
        assert!(!dir.join("voices.f32.new").exists() && !dir.join("speakers.json.new").exists(), "no half-written files left");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_meeting_whose_files_disagree_is_refused_not_labelled_from() {
        let dir = temp_dir("disagree");
        let mut s = meeting(2);
        s.voices = vec![0.0; s.at_ms.len() * 2];
        s.save_to(&dir).unwrap();
        // Truncate the vectors, as a copy interrupted halfway would.
        let (_, bin) = Speakers::files(&dir);
        let raw = std::fs::read(&bin).unwrap();
        std::fs::write(&bin, &raw[..raw.len() / 2]).unwrap();
        assert!(Speakers::load_from(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -- letting go of the audio ---------------------------------------------

    /// A meeting directory of its own under the data dir, because that is
    /// where `label_and_release` looks. It touches no other meeting, and is
    /// removed afterwards whatever happens.
    fn meeting_with_audio(tag: &str) -> (String, PathBuf) {
        let id = format!("test-speakers-{tag}-{}", std::process::id());
        let dir = crate::meeting::root().join(&id);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("audio")).unwrap();
        std::fs::write(dir.join("audio").join(crate::meeting::stretch_file(1500)), b"RIFF....").unwrap();
        std::fs::write(dir.join("audio").join(crate::meeting::stretch_file(9000)), b"RIFF....").unwrap();
        std::fs::write(dir.join("audio").join("not-audio.txt"), b"not audio").unwrap();
        (id, dir)
    }

    fn wavs_in(dir: &Path) -> usize {
        std::fs::read_dir(dir.join("audio"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "wav"))
            .count()
    }

    #[test]
    fn the_audio_is_let_go_even_when_the_labelling_failed() {
        let (id, dir) = meeting_with_audio("failed");
        let out = label_and_release(&id, 2, Path::new("/nowhere/model.onnx"), false, &|_| {});
        assert!(out.is_err(), "there is no model, so this cannot have worked");
        assert_eq!(wavs_in(&dir), 0, "a failure is not a reason to hold on to a recording of other people");
        assert!(dir.join("audio").join("not-audio.txt").exists(), "and only the audio goes");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_audio_stays_when_it_was_asked_to() {
        let (id, dir) = meeting_with_audio("kept");
        let _ = label_and_release(&id, 2, Path::new("/nowhere/model.onnx"), true, &|_| {});
        assert_eq!(wavs_in(&dir), 2, "Keep meeting audio means keep it, labelled or not");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // -- the model ----------------------------------------------------------

    /// The whole pipeline over a real kept-audio meeting, printing who each
    /// line went to so it can be scored against something independent. Writes
    /// speakers.json and voices.f32 into the meeting, so point HOME at a copy:
    ///
    ///   HOME=/tmp/copy MEETING_ID=<id> VOICE_MODEL=<onnx> \
    ///     cargo test --release labels_a_real_meeting -- --ignored --nocapture
    #[test]
    #[ignore = "needs MEETING_ID, VOICE_MODEL and a meeting whose audio was kept, under HOME"]
    fn labels_a_real_meeting() {
        let id = std::env::var("MEETING_ID").expect("set MEETING_ID");
        let model = std::env::var("VOICE_MODEL").expect("set VOICE_MODEL");
        let count: usize = std::env::var("PEOPLE").ok().and_then(|n| n.parse().ok()).unwrap_or(2);
        let started = std::time::Instant::now();
        let sp = label_meeting(&id, count, Path::new(&model), &|_| {}).unwrap();
        println!("TIME {:.1}", started.elapsed().as_secs_f32());
        println!("PEOPLE {} WINDOWS {}", sp.count, sp.at_ms.len());
        println!("SHARES {:?}", sp.shares());
        let segs = crate::meeting::transcript(&id);
        for (seg, who) in segs.iter().zip(sp.lines(&segs)) {
            println!("LINE {} {:.1} {} {:?}", seg.at_ms, seg.secs, seg.voice, who);
        }
        let again = Speakers::load(&id).expect("what was saved should load");
        assert_eq!(again, sp, "and what loads is what was saved");
    }

    #[test]
    fn sha256_matches_the_published_test_vector() {
        let p = std::env::temp_dir().join(format!("opendictate-sha-{}", std::process::id()));
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(sha256_of(&p).unwrap(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn the_model_is_not_ready_until_it_is_exactly_the_pinned_size() {
        let dir = temp_dir("model-ready");
        assert!(ready_in(&dir).is_none(), "absent");
        std::fs::write(dir.join(MODEL.file), vec![0u8; MODEL.bytes as usize - 1]).unwrap();
        assert!(ready_in(&dir).is_none(), "a truncated download is not ready");
        std::fs::write(dir.join(MODEL.file), vec![0u8; MODEL.bytes as usize]).unwrap();
        assert!(ready_in(&dir).is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The real download, once, into a temp dir: the pinned hash has only ever
    /// been compared with a file fetched by a script, not by this code.
    ///   cargo test --release downloads_and_verifies -- --ignored --nocapture
    #[test]
    #[ignore = "downloads about 30 MB"]
    fn downloads_and_verifies_the_real_model() {
        let dir = temp_dir("download");
        download_into(&dir, |_, _| {}).unwrap();
        assert!(ready_in(&dir).is_some(), "the file should be in place and the right size");
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "part"))
            .collect();
        assert!(leftovers.is_empty(), "a finished download left partial files behind");
        assert_eq!(sha256_of(&dir.join(MODEL.file)).unwrap(), MODEL.sha256);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
