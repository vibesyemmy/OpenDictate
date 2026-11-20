//! Shortcuts built on the right-hand Option key.
//!
//! "RightOption" on its own drives dictation: pressing it starts at once (so
//! the first word is not lost), holding it and letting go stops, and a quick
//! tap leaves dictation running hands-free until the next press.
//! "RightOption+T" and the like are ordinary press/release shortcuts.
//!
//! Only the right Option key is involved, so the left one keeps typing
//! special characters. If Right Option turns out to be in use as a modifier
//! (another key, a click, or another modifier while it is held) the
//! dictation it started is cancelled, never transcribed.
//!
//! This module is plain state with time passed in, so every rule is tested
//! without a keyboard; `chords::mac` feeds it real events.

#[cfg(test)]
mod tests {
    use super::*;

    const T: u16 = 17;
    const A: u16 = 0;

    fn solo_and_combos() -> RightOption {
        RightOption::new(Some(0), vec![(T, 1), (A, 2)])
    }

    #[test]
    fn names_parse_to_the_key_alone_or_with_one_key() {
        assert_eq!(parse("RightOption"), Some(None));
        assert_eq!(parse("rightoption+t"), Some(Some(T)));
        assert_eq!(parse("RightOption+Space"), Some(Some(49)));
        assert_eq!(parse("RightOption+Nope"), None);
        assert_eq!(parse("Alt+T"), None);
        assert!(is_right_option("RightOption+M"));
        assert!(!is_right_option("Space+Left"));
    }

    #[test]
    fn holding_dictates_until_release() {
        let mut ro = solo_and_combos();
        assert_eq!(ro.option(true, 0, false), vec![Signal::Start(0)]);
        assert_eq!(ro.option(false, 2_000, true), vec![Signal::Stop(0)]);
    }

    #[test]
    fn a_tap_keeps_dictating_until_the_next_press() {
        let mut ro = solo_and_combos();
        assert_eq!(ro.option(true, 0, false), vec![Signal::Start(0)]);
        assert_eq!(ro.option(false, 120, true), vec![], "a tap leaves it running");
        assert_eq!(ro.option(true, 5_000, true), vec![Signal::Stop(0)]);
        assert_eq!(ro.option(false, 5_100, false), vec![], "the stopping press's release does nothing");
        assert_eq!(ro.option(true, 9_000, false), vec![Signal::Start(0)], "and the next press starts afresh");
    }

    #[test]
    fn hands_free_that_ended_elsewhere_does_not_swallow_the_next_press() {
        let mut ro = solo_and_combos();
        ro.option(true, 0, false);
        ro.option(false, 100, true);
        // Dictation was stopped from the window, not the key.
        assert_eq!(ro.option(true, 3_000, false), vec![Signal::Start(0)]);
    }

    #[test]
    fn a_combination_cancels_the_dictation_it_began_and_fires_its_own() {
        let mut ro = solo_and_combos();
        assert_eq!(ro.option(true, 0, false), vec![Signal::Start(0)]);
        assert_eq!(ro.key(T, true, false), (true, vec![Signal::Cancel(0), Signal::Press(1)]));
        assert_eq!(ro.key(T, true, false), (true, vec![]), "auto-repeat is swallowed");
        assert_eq!(ro.key(T, false, false), (true, vec![Signal::Release(1)]));
        assert_eq!(ro.option(false, 900, false), vec![], "letting go after a combination does nothing");
    }

    #[test]
    fn right_option_used_for_typing_cancels_and_lets_the_key_through() {
        let mut ro = solo_and_combos();
        ro.option(true, 0, false);
        assert_eq!(ro.key(14, true, false), (false, vec![Signal::Cancel(0)]), "Right Option+E types é");
        assert_eq!(ro.key(14, false, false), (false, vec![]));
        assert_eq!(ro.option(false, 900, false), vec![]);
    }

    #[test]
    fn other_modifiers_or_clicks_mean_it_was_a_modifier() {
        let mut ro = solo_and_combos();
        ro.option(true, 0, false);
        assert_eq!(ro.key(T, true, true), (false, vec![Signal::Cancel(0)]), "Right Option+Cmd+T is not ours");
        let mut ro = solo_and_combos();
        ro.option(true, 0, false);
        assert_eq!(ro.other_input(), vec![Signal::Cancel(0)]);
        assert_eq!(ro.option(false, 2_000, false), vec![]);
    }

    #[test]
    fn keys_without_right_option_held_are_never_touched() {
        let mut ro = solo_and_combos();
        assert_eq!(ro.key(T, true, false), (false, vec![]));
        assert_eq!(ro.key(T, false, false), (false, vec![]));
    }

    #[test]
    fn combinations_work_without_a_solo_shortcut() {
        let mut ro = RightOption::new(None, vec![(T, 1)]);
        assert_eq!(ro.option(true, 0, false), vec![]);
        assert_eq!(ro.key(T, true, false), (true, vec![Signal::Press(1)]));
        assert_eq!(ro.key(T, false, false), (true, vec![Signal::Release(1)]));
        assert_eq!(ro.option(false, 300, false), vec![]);
    }

    #[test]
    fn releasing_option_before_the_letter_still_releases_the_shortcut() {
        let mut ro = solo_and_combos();
        ro.option(true, 0, false);
        ro.key(A, true, false);
        assert_eq!(ro.option(false, 200, false), vec![]);
        assert_eq!(ro.key(A, false, false), (true, vec![Signal::Release(2)]));
    }
}

/// macOS key code of the right Option key.
pub const KEY: u16 = 61;
/// A press shorter than this is a tap: dictation keeps going hands-free.
pub const TAP_MS: u64 = 300;
const NAME: &str = "RightOption";

/// `Some(None)` for Right Option alone, `Some(Some(key))` for Right Option
/// plus one key, `None` if `s` is not a Right Option shortcut.
pub fn parse(s: &str) -> Option<Option<u16>> {
    let s = s.trim();
    if s.eq_ignore_ascii_case(NAME) { return Some(None); }
    let (first, second) = s.split_once('+')?;
    if !first.trim().eq_ignore_ascii_case(NAME) { return None; }
    crate::chords::mac_key(second.trim()).map(Some)
}

pub fn is_right_option(s: &str) -> bool { parse(s).is_some() }

/// Whether the right Option key is down right now. Settings uses this while
/// recording a new shortcut, since the window's own key events can't tell
/// the two Option keys apart and say nothing about a modifier on its own.
#[cfg(target_os = "macos")]
pub fn held() -> bool {
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" { fn CGEventSourceFlagsState(state: i32) -> u64; }
    // 1 = kCGEventSourceStateHIDSystemState; 0x40 = right Option's device bit.
    unsafe { CGEventSourceFlagsState(1) & 0x40 != 0 }
}

#[cfg(not(target_os = "macos"))]
pub fn held() -> bool { false }

/// How a Right Option shortcut reads in Settings.
pub fn label(s: &str) -> Option<String> {
    match parse(s)? {
        None => Some("Right Option".into()),
        Some(_) => Some(format!("Right Option + {}", s.split_once('+')?.1.trim().to_uppercase())),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    /// Right Option alone: begin dictating.
    Start(usize),
    /// Right Option alone: finish and transcribe.
    Stop(usize),
    /// Right Option alone turned out to be a modifier: drop what it began.
    Cancel(usize),
    /// A Right Option combination went down / came up.
    Press(usize),
    Release(usize),
}

pub struct RightOption {
    solo: Option<usize>,
    combos: Vec<(u16, usize)>,
    /// When the key went down, while it is held.
    down_at: Option<u64>,
    /// This hold emitted Start and nothing has ended it yet.
    started: bool,
    /// This hold was used for something else; its release means nothing.
    used: bool,
    /// A tap left dictation running.
    hands_free: bool,
    active_combo: Option<(u16, usize)>,
}

impl RightOption {
    pub fn new(solo: Option<usize>, combos: Vec<(u16, usize)>) -> Self {
        Self { solo, combos, down_at: None, started: false, used: false, hands_free: false, active_combo: None }
    }

    pub fn is_empty(&self) -> bool { self.solo.is_none() && self.combos.is_empty() }

    fn cancel_started(&mut self) -> Vec<Signal> {
        if std::mem::take(&mut self.started) { self.solo.map(Signal::Cancel).into_iter().collect() } else { Vec::new() }
    }

    /// Right Option went down or up. `dictating` says whether the app is
    /// still recording the dictation a tap left running.
    pub fn option(&mut self, down: bool, now_ms: u64, dictating: bool) -> Vec<Signal> {
        if down {
            if self.down_at.is_some() { return Vec::new(); }
            self.down_at = Some(now_ms);
            self.used = false;
            self.started = false;
            let Some(solo) = self.solo else { return Vec::new() };
            if std::mem::take(&mut self.hands_free) && dictating {
                self.used = true;
                return vec![Signal::Stop(solo)];
            }
            self.started = true;
            return vec![Signal::Start(solo)];
        }
        let Some(at) = self.down_at.take() else { return Vec::new() };
        if self.used || !self.started { self.started = false; return Vec::new(); }
        self.started = false;
        let Some(solo) = self.solo else { return Vec::new() };
        if now_ms.saturating_sub(at) >= TAP_MS { vec![Signal::Stop(solo)] } else { self.hands_free = true; Vec::new() }
    }

    /// Any other key. Returns whether to swallow it, and what it means.
    /// `other_modifiers` is Shift, Control or Command being held too.
    pub fn key(&mut self, key: u16, down: bool, other_modifiers: bool) -> (bool, Vec<Signal>) {
        if let Some((k, id)) = self.active_combo {
            if k == key {
                if down { return (true, Vec::new()); }
                self.active_combo = None;
                return (true, vec![Signal::Release(id)]);
            }
        }
        if self.down_at.is_none() || !down { return (false, Vec::new()); }
        self.used = true;
        let mut signals = self.cancel_started();
        if !other_modifiers && self.active_combo.is_none() {
            if let Some(&(_, id)) = self.combos.iter().find(|(k, _)| *k == key) {
                self.active_combo = Some((key, id));
                signals.push(Signal::Press(id));
                return (true, signals);
            }
        }
        (false, signals)
    }

    /// A click or another modifier while Right Option is held: it was a
    /// modifier, so whatever it started is cancelled.
    pub fn other_input(&mut self) -> Vec<Signal> {
        if self.down_at.is_none() { return Vec::new(); }
        self.used = true;
        self.cancel_started()
    }
}
