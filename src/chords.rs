//! Recognition of two ordinary keys held together.

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome { Pass, Hold, ReplayAndPass, Press(usize), Release(usize), Suppress }

pub struct ChordState {
    shortcuts: Vec<(u16, u16, usize)>,
    pending: Option<u16>,
    passthrough: std::collections::HashSet<u16>,
    active: Option<(u16, u16, usize, bool)>,
}

/// US ANSI physical key codes used by the macOS event tap.
pub fn mac_key(name: &str) -> Option<u16> {
    Some(match name.to_ascii_uppercase().as_str() {
        "A" => 0, "S" => 1, "D" => 2, "F" => 3, "H" => 4, "G" => 5,
        "Z" => 6, "X" => 7, "C" => 8, "V" => 9, "B" => 11, "Q" => 12,
        "W" => 13, "E" => 14, "R" => 15, "Y" => 16, "T" => 17,
        "1" => 18, "2" => 19, "3" => 20, "4" => 21, "6" => 22, "5" => 23,
        "9" => 25, "7" => 26, "8" => 28, "0" => 29, "O" => 31,
        "U" => 32, "I" => 34, "P" => 35, "L" => 37, "J" => 38,
        "K" => 40, "N" => 45, "M" => 46, "SPACE" => 49,
        "EQUALS" => 24, "MINUS" => 27, "RIGHTBRACKET" => 30, "LEFTBRACKET" => 33,
        "ENTER" => 36, "QUOTE" => 39, "SEMICOLON" => 41, "BACKSLASH" => 42,
        "COMMA" => 43, "SLASH" => 44, "PERIOD" => 47, "TAB" => 48,
        "BACKSPACE" => 51, "DELETE" => 117, "HOME" => 115, "END" => 119,
        "PAGEUP" => 116, "PAGEDOWN" => 121, "LEFT" => 123, "RIGHT" => 124,
        "DOWN" => 125, "UP" => 126,
        _ => return None,
    })
}

pub fn parse(s: &str) -> Option<(u16, u16)> {
    let (a, b) = s.split_once('+')?;
    let (a, b) = (mac_key(a.trim())?, mac_key(b.trim())?);
    (a != b).then_some((a, b))
}

pub fn conflicts(a: &str, b: &str) -> bool {
    let left = parse(a);
    let right = parse(b);
    match (left, right) {
        (Some((a,b)), Some((c,d))) => (a == c && b == d) || (a == d && b == c),
        (Some((x,y)), None) => mac_key(b.trim()).is_some_and(|k| k == x || k == y),
        (None, Some((x,y))) => mac_key(a.trim()).is_some_and(|k| k == x || k == y),
        (None, None) => a.eq_ignore_ascii_case(b),
    }
}

impl ChordState {
    pub fn new(shortcuts: Vec<(u16, u16, usize)>) -> Self {
        Self { shortcuts, pending: None, passthrough: Default::default(), active: None }
    }
    pub fn cancel_pending(&mut self) {
        if let Some(key) = self.pending.take() { self.passthrough.insert(key); }
    }
    pub fn cancel_active(&mut self) -> Option<usize> {
        self.active.take().and_then(|(_, _, id, released)| (!released).then_some(id))
    }
    pub fn observe_unintercepted_release(&mut self, key: u16) { self.passthrough.remove(&key); }
    pub fn event(&mut self, key: u16, down: bool, repeat: bool) -> Outcome {
        if let Some((a,b,id,released)) = self.active {
            if key == a || key == b {
                if !down {
                    if !released { self.active = Some((a,b,id,true)); return Outcome::Release(id); }
                    self.active = None;
                }
                return Outcome::Suppress;
            }
        }
        if self.passthrough.contains(&key) {
            if !down { self.passthrough.remove(&key); }
            return Outcome::Pass;
        }
        if let Some(first) = self.pending {
            if down && key == first && repeat {
                self.pending = None;
                self.passthrough.insert(first);
                return Outcome::ReplayAndPass;
            }
            if !down && key == first { self.pending = None; return Outcome::ReplayAndPass; }
            if down {
                if let Some(&(_,_,id)) = self.shortcuts.iter().find(|(a,b,_)| (*a == first && *b == key) || (*b == first && *a == key)) {
                    self.pending = None;
                    self.active = Some((first,key,id,false));
                    return Outcome::Press(id);
                }
                self.pending = None;
                self.passthrough.insert(first);
                return Outcome::ReplayAndPass;
            }
            return Outcome::Pass;
        }
        if down && !repeat && self.shortcuts.iter().any(|(a,b,_)| *a == key || *b == key) {
            self.pending = Some(key);
            return Outcome::Hold;
        }
        Outcome::Pass
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chords_are_two_distinct_non_modifier_keys() {
        assert_eq!(parse("M+Space"), Some((46, 49)));
        assert_eq!(parse("Space+m"), Some((49, 46)));
        assert_eq!(parse("M+M"), None);
        assert_eq!(parse("Alt+Space"), None);
        assert_eq!(parse("M+Space+A"), None);
    }
    #[test]
    fn rejects_reversed_duplicates_and_unmodified_single_key_conflicts() {
        assert!(conflicts("M+Space", "Space+M"));
        assert!(conflicts("M+Space", "M"));
        assert!(conflicts("Space", "M+Space"));
        assert!(!conflicts("M+Space", "Alt+Space"));
        assert!(!conflicts("M+Space", "F19"));
    }
    #[test]
    fn ordinary_key_is_replayed_if_released_alone() {
        let mut state = ChordState::new(vec![(1, 2, 7)]);
        assert_eq!(state.event(1, true, false), Outcome::Hold);
        assert_eq!(state.event(1, false, false), Outcome::ReplayAndPass);
    }
    #[test]
    fn either_order_activates_once_and_releases_on_first_key_up() {
        for (first, second) in [(1, 2), (2, 1)] {
            let mut state = ChordState::new(vec![(1, 2, 7)]);
            assert_eq!(state.event(first, true, false), Outcome::Hold);
            assert_eq!(state.event(second, true, false), Outcome::Press(7));
            assert_eq!(state.event(second, true, true), Outcome::Suppress);
            assert_eq!(state.event(first, false, false), Outcome::Release(7));
            assert_eq!(state.event(second, false, false), Outcome::Suppress);
        }
    }
    #[test]
    fn unrelated_typing_replays_first_key_before_the_next_key() {
        let mut state = ChordState::new(vec![(1, 2, 7)]);
        assert_eq!(state.event(1, true, false), Outcome::Hold);
        assert_eq!(state.event(3, true, false), Outcome::ReplayAndPass);
        assert_eq!(state.event(1, false, false), Outcome::Pass);
    }
    #[test]
    fn a_held_key_repeats_normally_instead_of_waiting_for_release() {
        let mut state = ChordState::new(vec![(1, 2, 7)]);
        assert_eq!(state.event(1, true, false), Outcome::Hold);
        assert_eq!(state.event(1, true, true), Outcome::ReplayAndPass);
        assert_eq!(state.event(1, true, true), Outcome::Pass);
        assert_eq!(state.event(1, false, false), Outcome::Pass);
    }
    #[test]
    fn releasing_a_replayed_key_with_a_modifier_does_not_disable_the_next_chord() {
        let mut state = ChordState::new(vec![(1, 2, 7)]);
        assert_eq!(state.event(1, true, false), Outcome::Hold);
        state.cancel_pending();
        state.observe_unintercepted_release(1);
        assert_eq!(state.event(1, true, false), Outcome::Hold);
        assert_eq!(state.event(2, true, false), Outcome::Press(7));
    }
}

#[cfg(target_os = "macos")]
pub mod mac {
    use super::{ChordState, Outcome};
    use core_foundation::runloop::{CFRunLoop, CFRunLoopSource, kCFRunLoopCommonModes};
    use core_graphics::event::{CallbackResult, CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType, EventField};
    use std::{cell::RefCell, rc::Rc, sync::mpsc::Sender};

    struct Inner { state: ChordState, pending: Vec<CGEvent>, tx: Sender<(usize, bool)>, ctx: eframe::egui::Context }
    pub struct Listener { tap: Option<CGEventTap<'static>>, source: CFRunLoopSource, inner: Rc<RefCell<Inner>> }
    impl Listener {
        pub fn new(shortcuts: Vec<(u16,u16,usize)>, tx: Sender<(usize,bool)>, ctx: eframe::egui::Context) -> Result<Self, String> {
            let inner = Rc::new(RefCell::new(Inner { state: ChordState::new(shortcuts), pending: Vec::new(), tx, ctx }));
            let listener = inner.clone();
            // The tap is only attached to the current (main) thread's run loop.
            let tap = unsafe { CGEventTap::new_unchecked(CGEventTapLocation::Session, CGEventTapPlacement::HeadInsertEventTap,
                CGEventTapOptions::Default, vec![CGEventType::KeyDown, CGEventType::KeyUp, CGEventType::FlagsChanged, CGEventType::LeftMouseDown, CGEventType::RightMouseDown, CGEventType::OtherMouseDown],
                move |proxy, kind, event| {
                    let mut inner = listener.borrow_mut();
                    if matches!(kind, CGEventType::FlagsChanged | CGEventType::LeftMouseDown | CGEventType::RightMouseDown | CGEventType::OtherMouseDown) {
                        for saved in inner.pending.drain(..) { saved.post_from_tap(proxy); }
                        inner.state.cancel_pending();
                        if let Some(id) = inner.state.cancel_active() { let _ = inner.tx.send((id, false)); inner.ctx.request_repaint(); }
                        return CallbackResult::Keep;
                    }
                    if !matches!(kind, CGEventType::KeyDown | CGEventType::KeyUp) { return CallbackResult::Keep; }
                    let flags = event.get_flags();
                    let modifiers = CGEventFlags::CGEventFlagShift | CGEventFlags::CGEventFlagControl | CGEventFlags::CGEventFlagAlternate | CGEventFlags::CGEventFlagCommand;
                    if flags.intersects(modifiers) {
                        for saved in inner.pending.drain(..) { saved.post_from_tap(proxy); }
                        inner.state.cancel_pending();
                        if let Some(id) = inner.state.cancel_active() { let _ = inner.tx.send((id, false)); inner.ctx.request_repaint(); }
                        if matches!(kind, CGEventType::KeyUp) {
                            inner.state.observe_unintercepted_release(event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16);
                        }
                        return CallbackResult::Keep;
                    }
                    let key = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16;
                    let down = matches!(kind, CGEventType::KeyDown);
                    let repeat = event.get_integer_value_field(EventField::KEYBOARD_EVENT_AUTOREPEAT) != 0;
                    match inner.state.event(key, down, repeat) {
                        Outcome::Pass => CallbackResult::Keep,
                        Outcome::Hold => { inner.pending.push(event.clone()); CallbackResult::Drop },
                        Outcome::ReplayAndPass => { for saved in inner.pending.drain(..) { saved.post_from_tap(proxy); } CallbackResult::Keep },
                        Outcome::Press(id) => { inner.pending.clear(); let _ = inner.tx.send((id,true)); inner.ctx.request_repaint(); CallbackResult::Drop },
                        Outcome::Release(id) => { let _ = inner.tx.send((id,false)); inner.ctx.request_repaint(); CallbackResult::Drop },
                        Outcome::Suppress => CallbackResult::Drop,
                    }
                }) }.map_err(|_| "Two-key shortcuts need Accessibility and Input Monitoring permissions.".to_string())?;
            let source = tap.mach_port().create_runloop_source(0).map_err(|_| "Could not start the keyboard listener.".to_string())?;
            CFRunLoop::get_current().add_source(&source, unsafe { kCFRunLoopCommonModes });
            tap.enable();
            Ok(Self { tap: Some(tap), source, inner })
        }
    }
    impl Drop for Listener {
        fn drop(&mut self) {
            CFRunLoop::get_current().remove_source(&self.source, unsafe { kCFRunLoopCommonModes });
            self.tap.take();
            for event in self.inner.borrow_mut().pending.drain(..) { event.post(CGEventTapLocation::Session); }
        }
    }
}
