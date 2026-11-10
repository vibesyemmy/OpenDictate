#![allow(dead_code)]

const FRAME_SECS: f64 = 0.02;
const VOICED_RMS: f32 = 0.01;
const PAUSE_FRAMES: usize = 25;
const FORCED_CUT_FRAMES: usize = 750;
const TURN_FRAMES: usize = 15;
const SIDE_RATIO: f32 = 2.5;

pub struct Stretch {
    pub voice: &'static str,
    pub samples: Vec<f32>,
    pub secs: f32,
    pub voiced_secs: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum Side {
    You,
    Them,
    Both,
}

pub struct Cutter {
    rate: f64,
    frame: usize,
    pending_mine: Vec<f32>,
    pending_theirs: Vec<f32>,
    mine: Vec<f32>,
    theirs: Vec<f32>,
    frames: usize,
    voiced: usize,
    quiet_run: usize,
    side: Option<Side>,
    turn_run: usize,
    split_ok: bool,
}

fn rms(v: &[f32]) -> f32 {
    (v.iter().map(|x| x * x).sum::<f32>() / v.len().max(1) as f32).sqrt()
}

fn side_of(you_rms: f32, them_rms: f32) -> Side {
    if you_rms > them_rms * SIDE_RATIO {
        Side::You
    } else if them_rms > you_rms * SIDE_RATIO {
        Side::Them
    } else {
        Side::Both
    }
}

impl Cutter {
    pub fn new(rate: f64) -> Self {
        Self {
            rate,
            frame: ((rate * FRAME_SECS) as usize).max(1),
            pending_mine: Vec::new(),
            pending_theirs: Vec::new(),
            mine: Vec::new(),
            theirs: Vec::new(),
            frames: 0,
            voiced: 0,
            quiet_run: 0,
            side: None,
            turn_run: 0,
            split_ok: true,
        }
    }

    pub fn push(&mut self, mine: &[f32], theirs: &[f32], split_ok: bool) -> Vec<Stretch> {
        self.split_ok = split_ok;
        self.pending_mine.extend_from_slice(mine);
        self.pending_theirs.extend_from_slice(theirs);
        if theirs.is_empty() {
            self.pending_theirs.resize(self.pending_mine.len().max(self.pending_theirs.len()), 0.0);
        }
        if mine.is_empty() {
            self.pending_mine.resize(self.pending_theirs.len().max(self.pending_mine.len()), 0.0);
        }

        let mut out = Vec::new();
        let whole = self.pending_mine.len().min(self.pending_theirs.len()) / self.frame * self.frame;
        let a: Vec<f32> = self.pending_mine.drain(..whole).collect();
        let b: Vec<f32> = self.pending_theirs.drain(..whole).collect();
        for (fa, fb) in a.chunks(self.frame).zip(b.chunks(self.frame)) {
            self.frame_in(fa, fb, &mut out);
        }
        out
    }

    pub fn flush(&mut self) -> Option<Stretch> {
        let n = self.pending_mine.len().max(self.pending_theirs.len());
        self.pending_mine.resize(n, 0.0);
        self.pending_theirs.resize(n, 0.0);
        self.mine.append(&mut self.pending_mine);
        self.theirs.append(&mut self.pending_theirs);
        self.take_stretch(self.mine.len().div_ceil(self.frame))
    }

    fn frame_in(&mut self, fa: &[f32], fb: &[f32], out: &mut Vec<Stretch>) {
        let (you_rms, them_rms) = (rms(fa), rms(fb));
        self.mine.extend_from_slice(fa);
        self.theirs.extend_from_slice(fb);
        self.frames += 1;

        if you_rms.max(them_rms) < VOICED_RMS {
            self.quiet_run += 1;
            self.turn_run = 0;
            if self.quiet_run >= PAUSE_FRAMES {
                out.extend(self.take_stretch(self.frames));
            }
            return;
        }
        self.quiet_run = 0;
        self.voiced += 1;

        let side = side_of(you_rms, them_rms);
        if let Some(at) = self.turn_cut_at(side) {
            self.voiced -= TURN_FRAMES;
            out.extend(self.take_stretch(at));
            self.side = Some(side);
            self.voiced = TURN_FRAMES;
            return;
        }
        if self.frames >= FORCED_CUT_FRAMES {
            out.extend(self.take_stretch(self.frames));
        }
    }

    fn turn_cut_at(&mut self, frame_side: Side) -> Option<usize> {
        match self.side {
            None if frame_side != Side::Both => self.side = Some(frame_side),
            Some(side) if self.split_ok && frame_side != Side::Both && frame_side != side => {
                self.turn_run += 1;
                if self.turn_run >= TURN_FRAMES {
                    return Some(self.frames - self.turn_run);
                }
                return None;
            }
            _ => {}
        }
        self.turn_run = 0;
        None
    }

    fn take_stretch(&mut self, upto_frames: usize) -> Option<Stretch> {
        let at = (upto_frames * self.frame).min(self.mine.len());
        let tail_mine = self.mine.split_off(at);
        let tail_theirs = self.theirs.split_off(at);
        let mine = std::mem::replace(&mut self.mine, tail_mine);
        let theirs = std::mem::replace(&mut self.theirs, tail_theirs);
        let voiced = self.voiced;
        self.frames = self.frames.saturating_sub(upto_frames);
        self.voiced = 0;
        self.quiet_run = 0;
        self.side = None;
        self.turn_run = 0;
        if voiced == 0 {
            return None;
        }

        let identical = mine == theirs;
        let (voice, samples) = if identical || !self.split_ok {
            let theirs_silent = theirs.iter().all(|x| *x == 0.0);
            ("both", if theirs_silent { mine } else { theirs })
        } else {
            match side_of(rms(&mine), rms(&theirs)) {
                Side::You => ("you", mine),
                Side::Them => ("them", theirs),
                Side::Both => (
                    "both",
                    mine.iter().zip(&theirs).map(|(a, b)| (a + b) * 0.5).collect(),
                ),
            }
        };
        Some(Stretch {
            voice,
            secs: (samples.len() as f64 / self.rate) as f32,
            voiced_secs: (voiced as f64 * FRAME_SECS) as f32,
            samples,
        })
    }
}

pub fn worth_transcribing(peak: f32, rms: f32, voiced_secs: f32) -> bool {
    peak >= 0.03 && rms >= 0.002 && (peak >= 0.10 || voiced_secs >= 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 48_000.0;

    fn speech(secs: f64) -> Vec<f32> {
        (0..(secs * RATE) as usize)
            .map(|i| 0.3 * (2.0 * std::f32::consts::PI * 220.0 * i as f32 / RATE as f32).sin())
            .collect()
    }

    fn silence(secs: f64) -> Vec<f32> {
        vec![0.0; (secs * RATE) as usize]
    }

    fn cat(parts: &[Vec<f32>]) -> Vec<f32> {
        parts.concat()
    }

    fn cut_all(mine: &[f32], theirs: &[f32], split_ok: bool) -> Vec<Stretch> {
        let mut c = Cutter::new(RATE);
        let mut out = c.push(mine, theirs, split_ok);
        out.extend(c.flush());
        out
    }

    #[test]
    fn cuts_after_half_a_second_of_silence_following_speech() {
        let theirs = cat(&[speech(2.0), silence(0.6)]);
        let out = Cutter::new(RATE).push(&silence(2.6), &theirs, true);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].voice, "them");
        assert!((2.0..2.6).contains(&out[0].secs), "{}", out[0].secs);
    }

    #[test]
    fn does_not_cut_on_a_pause_shorter_than_half_a_second() {
        let theirs = cat(&[speech(1.0), silence(0.3), speech(1.0)]);
        let out = cut_all(&silence(2.3), &theirs, true);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn forces_a_cut_after_fifteen_seconds_of_nonstop_speech() {
        let out = Cutter::new(RATE).push(&silence(20.0), &speech(20.0), true);
        assert!((out[0].secs - 15.0).abs() < 0.05, "{}", out[0].secs);
    }

    #[test]
    fn cuts_when_the_speaker_changes_from_them_to_you() {
        let mine = cat(&[silence(2.0), speech(2.0)]);
        let theirs = cat(&[speech(2.0), silence(2.0)]);
        let out = cut_all(&mine, &theirs, true);
        let voices: Vec<_> = out.iter().map(|s| s.voice).collect();
        assert_eq!(voices, ["them", "you"]);
        for stretch in &out {
            assert!((stretch.secs - 2.0).abs() < 0.1, "{}", stretch.secs);
        }
    }

    #[test]
    fn a_turn_split_by_a_short_pause_does_not_cut_inside_the_pause() {
        let mine = cat(&[silence(1.0), speech(0.2), silence(0.3), speech(0.2)]);
        let theirs = cat(&[speech(1.0), silence(0.7)]);
        let out = cut_all(&mine, &theirs, true);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn a_speaker_change_shorter_than_300ms_does_not_cut() {
        let mine = cat(&[silence(2.0), speech(0.2), silence(2.0)]);
        let theirs = cat(&[speech(2.0), silence(0.2), speech(2.0)]);
        let out = cut_all(&mine, &theirs, true);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].voice, "them");
    }

    #[test]
    fn uneven_pushes_keep_the_two_sides_aligned() {
        let theirs = cat(&[speech(2.0), silence(0.6), speech(2.3)])[..234_880].to_vec();
        let mine = silence(theirs.len() as f64 / RATE);
        let aligned = cut_all(&mine, &theirs, true);

        let mut c = Cutter::new(RATE);
        let mut uneven = Vec::new();
        let (mut at_m, mut at_t) = (0, 0);
        for round in 0..20 {
            let (m, t) = if round % 2 == 0 { (12_000, 11_488) } else { (11_488, 12_000) };
            uneven.extend(c.push(&mine[at_m..at_m + m], &theirs[at_t..at_t + t], true));
            at_m += m;
            at_t += t;
        }
        uneven.extend(c.flush());

        assert_eq!(uneven.len(), aligned.len());
        for (u, a) in uneven.iter().zip(&aligned) {
            assert_eq!(u.samples, a.samples);
        }
    }

    #[test]
    fn a_mic_only_stream_still_produces_stretches() {
        let mut c = Cutter::new(RATE);
        let mic = cat(&[speech(1.0), silence(0.6)]);
        let out = c.push(&mic, &[], false);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].voice, "both");
    }

    #[test]
    fn a_mic_only_stream_keeps_flowing_across_pushes_that_are_not_whole_frames() {
        let mut c = Cutter::new(RATE);
        let mic = cat(&[speech(1.25), silence(0.75)]);
        let out: Vec<_> = mic.chunks(12_000).flat_map(|m| c.push(m, &[], false)).collect();
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn flush_keeps_the_tail_of_the_longer_side() {
        let mut c = Cutter::new(RATE);
        c.push(&speech(1.0)[..2_000], &silence(1.0)[..1_000], true);
        assert_eq!(c.flush().expect("stretch").samples.len(), 2_000);
    }

    #[test]
    fn a_turn_cut_does_not_count_the_new_speakers_frames_as_voiced_in_the_old_stretch() {
        let mine = cat(&[silence(2.0), speech(2.0)]);
        let theirs = cat(&[speech(2.0), silence(2.0)]);
        let out = cut_all(&mine, &theirs, true);
        assert!((out[0].voiced_secs - 2.0).abs() <= 0.02, "{}", out[0].voiced_secs);
    }

    #[test]
    fn flush_keeps_the_last_partial_frame() {
        let mut c = Cutter::new(RATE);
        let theirs = speech(1.0)[..48_000 - 100].to_vec();
        c.push(&silence(1.0)[..theirs.len()], &theirs, true);
        assert_eq!(c.flush().expect("stretch").samples.len(), theirs.len());
    }

    #[test]
    fn the_speech_after_a_forced_cut_comes_back_as_the_next_stretch() {
        let mut c = Cutter::new(RATE);
        let first = c.push(&silence(20.0), &speech(20.0), true);
        assert_eq!(first.len(), 1);
        let rest = c.flush().expect("rest");
        assert!((rest.secs - 5.0).abs() < 0.05, "{}", rest.secs);
        assert!(rest.voiced_secs > 4.9);
    }

    #[test]
    fn drops_stretches_that_are_all_silence() {
        assert!(cut_all(&silence(5.0), &silence(5.0), true).is_empty());
    }

    #[test]
    fn labels_both_and_ignores_turns_when_sides_cannot_be_split() {
        let mine = cat(&[silence(2.0), speech(2.0)]);
        let theirs = cat(&[speech(2.0), silence(2.0)]);
        let out = cut_all(&mine, &theirs, false);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].voice, "both");
    }

    #[test]
    fn flush_returns_speech_still_in_progress_when_the_meeting_ends() {
        let mut c = Cutter::new(RATE);
        assert!(c.push(&silence(1.0), &speech(1.0), true).is_empty());
        let s = c.flush().expect("stretch");
        assert_eq!(s.voice, "them");
        assert!(s.voiced_secs > 0.9);
        assert!(c.flush().is_none());
    }

    #[test]
    fn worth_transcribing_rejects_noise_and_short_uncertain_tails() {
        assert!(worth_transcribing(0.43, 0.05, 3.0));
        assert!(!worth_transcribing(0.036, 0.003, 0.0));
        assert!(!worth_transcribing(0.021, 0.002, 0.0));
        assert!(worth_transcribing(0.08, 0.01, 1.2));
        assert!(!worth_transcribing(0.02, 0.01, 2.0));
    }
}
