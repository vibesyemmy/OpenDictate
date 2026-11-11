//! App state and everything that happens outside drawing: global shortcuts,
//! recording, running the person's models, pasting, history and the screenshot mode.

use crate::audio::Recording;
use crate::engines::{self, SharedProgress};
use crate::settings::{self, Action, DictWord, Entry, Settings, TextEngine, Trigger};
use crate::stats::Stats;
use crate::theme;
use crate::tray::Tray;
use eframe::egui;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tray_icon::menu::MenuEvent;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    History,
    Meetings,
    Dictionary,
}


#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Models,
    General,
    Audio,
    Personalization,
    About,
}

pub struct JobOut {
    pub action: Action,
    pub when: u64,
    pub raw: String,
    pub result: Result<String, String>,
    pub seconds: f32,
    pub selection: String,
    pub wav: Vec<u8>,
    pub retry: bool,
    pub speech_model: String,
    pub text_model: String,
    /// Something worth telling the person even though it worked.
    pub note: Option<String>,
}

/// What we know about the models right now (refreshed in the background).
#[derive(Default, Clone)]
pub struct Health {
    pub whisper_bin: Option<PathBuf>,
    pub ollama: Option<Result<Vec<String>, String>>,
    pub remote_text: Option<Result<Vec<String>, String>>,
    pub checked: bool,
}

pub struct Toast {
    pub text: String,
    pub error: bool,
    pub at: Instant,
}

pub struct DictateApp {
    pub s: Settings,
    pub history: Vec<Entry>,
    pub stats: Stats,
    pub page: Page,
    pub settings_tab: Option<SettingsTab>,

    /// The live recording, what it is for, and the copy of the selection
    /// being fetched alongside it. Ask grabs the selection on a thread so the
    /// microphone opens at once instead of after the clipboard settles.
    pub recording: Option<(Recording, Action, Option<std::thread::JoinHandle<String>>)>,
    pub busy: Option<(Action, Instant)>,
    jobs_tx: Sender<JobOut>,
    jobs_rx: Receiver<JobOut>,

    hotkeys: Option<GlobalHotKeyManager>,
    registered: Vec<(HotKey, Trigger)>,
    hk_rx: Receiver<GlobalHotKeyEvent>,
    pub hotkey_errors: Vec<String>,
    pub capturing_key: Option<Trigger>,

    /// Microphones found on the system, and the system default's name.
    pub mics: Vec<String>,
    pub default_mic: Option<String>,
    /// Live recording used only to show the level meter in Settings.
    pub mic_test: Option<Recording>,

    pub health: Arc<Mutex<Health>>,
    pub download: Option<(String, SharedProgress)>,
    pub pull_name: String,
    pub test_input: String,
    pub test_output: Arc<Mutex<Option<Result<String, String>>>>,
    pub test_running: Arc<AtomicBool>,

    pub dict_filter: u8,
    pub dict_search: Option<String>,
    pub new_word: Option<String>,
    pub hist_filter: Option<Action>,
    pub expanded: Option<u64>,
    pub editing: Option<(u64, String)>,

    pub toast: Option<Toast>,
    told_no_cleanup: bool,
    dirty: bool,

    shot: Option<PathBuf>,
    frames: u32,
    shot_done: Arc<AtomicBool>,

    /// Menu bar icon and the channel its clicks arrive on. Absent in
    /// screenshot mode, and on any system that refuses the icon.
    tray: Option<Tray>,
    tray_rx: Option<Receiver<MenuEvent>>,
    tray_state: crate::tray::State,
    /// Whether the real window is on screen. It is a child of the 1x1 root
    /// viewport, so "closed" simply means we stop showing it.
    window_open: bool,
    focus_main: bool,
    /// Set by the menu bar's Quit, so the close that follows is a real one
    /// rather than another hide.
    quitting: bool,
    /// The meeting being recorded, if any.
    pub meeting: Option<crate::meeting::Session>,
    talking_view: Option<crate::talking_view::ViewState>,
    /// Which stored meeting the Meetings page is showing, and its summary
    /// if one has been made - read once when it opens rather than off disk
    /// on every frame.
    pub open_meeting: Option<String>,
    pub open_summary: Option<crate::meeting::Summary>,
    /// Read once when a meeting is opened, not on every frame. Drawing the
    /// detail page used to re-read every meeting's meta.json and parse the
    /// whole transcript twice per frame - 15 KB of JSON, sixty times a
    /// second, to show text that had not changed.
    pub open_meta: Option<crate::meeting::Meta>,
    pub open_segments: Vec<crate::meeting::Segment>,
    /// The list, and whether it needs reading again.
    pub meeting_list: Vec<crate::meeting::Meta>,
    pub meetings_dirty: bool,
    /// The meeting being summarised, and how far it has got.
    pub summarising: Option<String>,
    pub summary_progress: Arc<Mutex<String>>,
    summary_rx: Option<Receiver<(String, Result<crate::meeting::Summary, String>)>>,
    /// The window the backdrop is attached to, if any.
    ///
    /// Not a flag: closing the window destroys it and the backdrop with it,
    /// and resetting a flag only works if the close is noticed. Clicking the
    /// red button does not reach close_requested here, so the first attempt
    /// at this missed it entirely. Comparing window numbers needs nothing to
    /// be noticed - a different window simply has a different number.
    blurred_window: Option<isize>,
    /// Whether the app was in front last frame, for spotting a Dock click.
    was_active: bool,
    /// Whether the Dock's reopen message is being answered, and how many
    /// frames have been spent trying. eframe's delegate does not exist on
    /// the first frame, so this is attempted until it takes.
    hooked: bool,
    hook_tries: u16,
    /// What the window's visibility was last set to, so it is only sent when
    /// it changes.
    window_shown: Option<bool>,
    /// When the window was hidden, so the app can step out of the
    /// foreground a moment later rather than during the hide.
    resign_at: Option<std::time::Instant>,
    /// When that happened, and how many times the Dock tile has been set
    /// since: the Dock can put its cached icon back during launch, so this
    /// is applied again a second and three seconds later.
    window_since: Option<std::time::Instant>,
    icon_tries: u8,
}

/// Whether the OS is in light mode, or None before any window exists.
fn system_light(ctx: &egui::Context) -> Option<bool> {
    ctx.system_theme().map(|t| t == egui::Theme::Light)
}

impl DictateApp {
    pub fn new(cc: &eframe::CreationContext<'_>, shot: Option<PathBuf>) -> Self {
        let mut s = settings::load_settings();
        theme::set_light(s.appearance.is_light(system_light(&cc.egui_ctx)));
        theme::install(&cc.egui_ctx);
        let (jobs_tx, jobs_rx) = channel();
        let (hk_tx, hk_rx) = channel();
        let ctx = cc.egui_ctx.clone();
        GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
            let _ = hk_tx.send(e);
            ctx.request_repaint();
        }));

        if s.whisper_model.is_empty() {
            // Pick a downloaded model automatically on first run.
            if let Some(p) = engines::local_whisper_models().into_iter().next_back() {
                s.whisper_model = p.to_string_lossy().into_owned();
            }
        }

        let mut app = Self {
            s,
            history: settings::load_history(),
            stats: Stats::load(),
            page: Page::Home,
            settings_tab: None,
            recording: None,
            busy: None,
            jobs_tx,
            jobs_rx,
            hotkeys: None,
            registered: Vec::new(),
            hk_rx,
            hotkey_errors: Vec::new(),
            capturing_key: None,
            mics: Vec::new(),
            default_mic: None,
            mic_test: None,
            health: Default::default(),
            download: None,
            pull_name: String::new(),
            test_input: "um so I think we should, uh, meet on tuesday, no wait, wednesday at 3 and bring the the slides"
                .into(),
            test_output: Default::default(),
            test_running: Default::default(),
            dict_filter: 0,
            dict_search: None,
            new_word: None,
            hist_filter: None,
            expanded: None,
            editing: None,
            toast: None,
            told_no_cleanup: false,
            dirty: false,
            shot,
            frames: 0,
            shot_done: Default::default(),
            tray: None,
            tray_rx: None,
            tray_state: crate::tray::State::Idle,
            window_open: true,
            focus_main: false,
            quitting: false,
            meeting: None,
            talking_view: None,
            open_meeting: None,
            open_summary: None,
            open_meta: None,
            open_segments: Vec::new(),
            meeting_list: Vec::new(),
            meetings_dirty: true,
            summarising: None,
            summary_progress: Arc::new(Mutex::new(String::new())),
            summary_rx: None,
            blurred_window: None,
            was_active: true,
            hooked: false,
            hook_tries: 0,
            window_shown: None,
            resign_at: None,
            window_since: None,
            icon_tries: 0,
        };
        // Lets a screenshot start on another screen:
        // --page history|meetings|dictionary|models|settings|audio|personal
        // --open <meeting id> opens that stored meeting
        let args: Vec<String> = std::env::args().collect();
        if let Some(p) = args.iter().position(|a| a == "--page").and_then(|i| args.get(i + 1)) {
            match p.as_str() {
                "history" => app.page = Page::History,
                "meetings" => app.page = Page::Meetings,
                "dictionary" => app.page = Page::Dictionary,
                "models" => app.settings_tab = Some(SettingsTab::Models),
                "settings" => app.settings_tab = Some(SettingsTab::General),
                "audio" => app.settings_tab = Some(SettingsTab::Audio),
                "personal" => app.settings_tab = Some(SettingsTab::Personalization),
                _ => {}
            }
        }
        // --open <meeting id> goes straight into one, which is the only way
        // to put the meeting detail in front of a screenshot.
        if let Some(id) = args.iter().position(|a| a == "--open").and_then(|i| args.get(i + 1)) {
            app.page = Page::Meetings;
            app.open_meeting(id.clone());
        }
        // Started by the login item, not by a person: come up in the menu
        // bar with no window. Logging in is not a request to be shown
        // anything, and a window across the desktop at every boot is the
        // reason people turn a login item back off.
        if args.iter().any(|a| a == "--hidden") {
            app.window_open = false;
        }
        app.prune_history();
        app.refresh_mics();
        if app.shot.is_none() {
            app.hotkeys = GlobalHotKeyManager::new().ok();
            app.register_hotkeys();
            if let Some((t, rx)) = crate::tray::build(&cc.egui_ctx) {
                app.tray = Some(t);
                app.tray_rx = Some(rx);
            }
        }
        app.check_health_and_start(&cc.egui_ctx);
        // The login item carries the path to this binary, so it goes stale
        // the moment the app is moved - out of Downloads into Applications,
        // most often. Rewriting it on every launch keeps it pointing here.
        #[cfg(target_os = "macos")]
        if app.shot.is_none() {
            crate::login_item::sync(app.s.start_at_login);
        }
        app
    }

    /// Keeps the app running when its window is closed.
    ///
    /// The shortcuts only work while the process is alive, so closing the
    /// window hides it instead of quitting; the menu bar icon brings it back
    /// and is the only way to quit for real. Screenshot mode is left alone,
    /// because it closes the window to signal that it has finished.
    ///
    /// A hidden window is sent no frames, and this runs once per frame, so
    /// while hidden it keeps asking for repaints. Without that pump the
    /// shortcuts and the menu below are never read and the app only looks alive.
    fn window_lifecycle(&mut self, ctx: &egui::Context) {
        let ids = self.tray.as_ref().map(|t| (t.show.clone(), t.quit.clone()));
        let clicks: Vec<MenuEvent> = self.tray_rx.as_ref().map_or_else(Vec::new, |rx| rx.try_iter().collect());
        if let Some((show, quit)) = ids {
            for e in clicks {
                if e.id == show {
                    self.window_open = true;
                    self.focus_main = true;
                } else if e.id == quit {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        // Clicking the Dock icon brings an app's window back on macOS, and
        // the message that says so is applicationShouldHandleReopen. eframe
        // owns the delegate and ignores it, so the method is added to the
        // delegate's class the first time it exists.
        #[cfg(target_os = "macos")]
        {
            if !self.hooked && self.hook_tries < 600 {
                self.hook_tries += 1;
                self.hooked = crate::macos_look::install_reopen_hook();
                if self.hooked {
                    crate::macos_look::log_backdrop(&format!("reopen hook after {} frames", self.hook_tries));
                }
            }
            if crate::macos_look::take_reopen() && self.shot.is_none() {
                self.window_open = true;
                self.focus_main = true;
            }
            // Fallback while the hook has not taken: the app coming to the
            // front with nothing open means the same thing. It only catches
            // a Dock click made from another app, which is why it is not
            // enough on its own.
            let active = crate::macos_look::app_is_active();
            if !self.hooked && active && !self.was_active && !self.window_open && self.shot.is_none() {
                self.window_open = true;
                self.focus_main = true;
            }
            self.was_active = active;
        }
        #[cfg(target_os = "macos")]
        if let Some(at) = self.resign_at {
            if at.elapsed().as_millis() > 250 {
                self.resign_at = None;
                if !self.window_open {
                    crate::macos_look::resign_active();
                    self.was_active = false;
                }
            }
        }
        // A closing root means quit; the real window closes on its own below.
        if self.shot.is_none() && !self.quitting && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        // The root is tiny and always drawn, but egui is still only redrawing
        // on demand. This keeps shortcuts and menu clicks being read while the
        // app sits idle in the background.
        ctx.request_repaint_after(std::time::Duration::from_millis(200));

        // A lit menu bar icon, so the state is visible even when the indicator
        // is not where the eye happens to be.
        let state = if self.recording.is_some() {
            crate::tray::State::Recording
        } else if self.busy.is_some() {
            crate::tray::State::Thinking
        } else {
            crate::tray::State::Idle
        };
        if state != self.tray_state {
            self.tray_state = state;
            if let Some(t) = &self.tray {
                t.set_state(state);
            }
        }
    }

    pub fn changed(&mut self) {
        self.dirty = true;
    }

    pub fn notify(&mut self, text: impl Into<String>, error: bool) {
        self.toast = Some(Toast { text: text.into(), error, at: Instant::now() });
    }

    // -----------------------------------------------------------------------
    // Shortcuts

    pub fn register_hotkeys(&mut self) {
        self.hotkey_errors.clear();
        let Some(mgr) = &self.hotkeys else {
            if self.shot.is_none() {
                self.hotkey_errors.push("Global shortcuts are unavailable on this system.".into());
            }
            return;
        };
        let old: Vec<HotKey> = self.registered.drain(..).map(|(h, _)| h).collect();
        let _ = mgr.unregister_all(&old);
        let want: Vec<(String, Trigger)> = Trigger::ALL
            .iter()
            .flat_map(|t| self.s.trigger_keys(*t).iter().map(|k| (k.clone(), *t)))
            .collect();
        for (k, trigger) in want {
            match k.parse::<HotKey>() {
                Ok(h) => match mgr.register(h) {
                    Ok(()) => self.registered.push((h, trigger)),
                    Err(e) => self.hotkey_errors.push(format!("{k}: {e}")),
                },
                Err(e) => self.hotkey_errors.push(format!("{k}: {e}")),
            }
        }
    }

    fn on_hotkey(&mut self, ctx: &egui::Context, e: GlobalHotKeyEvent) {
        let Some(trigger) = self.registered.iter().find(|(h, _)| h.id() == e.id).map(|(_, a)| *a) else {
            return;
        };
        // A meeting is always a toggle: hold-to-talk makes no sense for
        // something that runs for an hour.
        let action = match trigger {
            Trigger::Meeting => {
                if e.state == HotKeyState::Pressed {
                    self.toggle_meeting();
                }
                return;
            }
            Trigger::Act(a) => a,
        };
        match e.state {
            HotKeyState::Pressed => {
                if let Some((_, current, _)) = &self.recording {
                    if !self.s.hold_to_talk && *current == action {
                        self.stop_recording(ctx);
                    }
                } else if self.busy.is_none() {
                    self.start_recording(action);
                }
            }
            HotKeyState::Released => {
                if self.s.hold_to_talk && self.recording.as_ref().is_some_and(|(_, a, _)| *a == action) {
                    self.stop_recording(ctx);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Recording and the model pipeline

    /// Starts or ends a meeting. Dictation and meetings do not overlap:
    /// they would be fighting over the same microphone.
    pub fn toggle_meeting(&mut self) {
        if let Some(session) = self.meeting.take() {
            self.notify("Finishing the meeting…", false);
            let id = session.stop();
            self.meetings_dirty = true;
            self.open_meeting(id.clone());
            self.page = Page::Meetings;
            // The whole point of the end-of-meeting summary is that nobody
            // has to ask for it.
            if crate::meeting::transcript(&id).is_empty() {
                self.notify("Meeting saved, but nothing was transcribed", true);
            } else {
                self.notify("Meeting saved · summarising", false);
                self.start_summary(id);
            }
            return;
        }
        if self.recording.is_some() || self.busy.is_some() {
            self.notify("Finish the dictation first.", true);
            return;
        }
        match crate::meeting::Session::start(&self.s) {
            Ok(session) => {
                self.notify(format!("Recording: {}", session.source.to_lowercase()), false);
                self.meeting = Some(session);
                self.talking_view = Some(Default::default());
            }
            Err(e) => self.notify(e, true),
        }
    }

    /// Reads the meeting back and works out what it came to, on a thread:
    /// a long meeting is a dozen model calls and the window must stay alive.
    pub fn start_summary(&mut self, id: String) {
        if self.summarising.is_some() {
            self.notify("Already summarising a meeting.", true);
            return;
        }
        let (tx, rx) = channel();
        let progress = self.summary_progress.clone();
        let settings = self.s.clone();
        let for_thread = id.clone();
        std::thread::spawn(move || {
            let out = crate::meeting::summarise(&settings, &for_thread, &progress);
            let _ = tx.send((for_thread, out));
        });
        if let Ok(mut p) = self.summary_progress.lock() {
            *p = "Reading the meeting…".into();
        }
        self.summarising = Some(id);
        self.summary_rx = Some(rx);
    }

    /// Picks up a finished summary. Called once a frame.
    fn collect_summary(&mut self) {
        let Some(rx) = &self.summary_rx else { return };
        let Ok((id, out)) = rx.try_recv() else { return };
        self.summary_rx = None;
        self.summarising = None;
        match out {
            Ok(summary) => {
                if self.open_meeting.as_deref() == Some(id.as_str()) {
                    self.open_summary = Some(summary);
                }
                // The last step of a summary renames the meeting on disk, so
                // the list and the open header are both out of date the
                // moment it lands. Without this the new title only appeared
                // after leaving Meetings and coming back, which is the one
                // thing you would not think to do.
                self.refresh_meetings();
                if self.open_meeting.as_deref() == Some(id.as_str()) {
                    self.open_meta = self.meeting_list.iter().find(|m| m.id == id).cloned();
                }
                self.notify("Meeting summarised", false);
            }
            Err(e) => self.notify(e, true),
        }
    }

    /// Opens a stored meeting, reading it from disk once.
    pub fn open_meeting(&mut self, id: String) {
        self.open_summary = crate::meeting::summary(&id);
        self.open_segments = crate::meeting::transcript(&id);
        self.refresh_meetings();
        self.open_meta = self.meeting_list.iter().find(|m| m.id == id).cloned();
        self.open_meeting = Some(id);
    }

    /// Re-reads the list of meetings. Called when the page is opened and
    /// when a meeting is added or removed, not once a frame.
    pub fn refresh_meetings(&mut self) {
        self.meeting_list = crate::meeting::list();
        self.meetings_dirty = false;
    }

    pub fn start_recording(&mut self, action: Action) {
        // The selection is not needed until the speaking stops, seconds from
        // now, so fetching it must not hold the microphone shut meanwhile.
        let selection = if action == Action::Ask { begin_selection_grab() } else { None };
        self.mic_test = None;
        match Recording::start(&self.s.microphone, self.s.input_gain) {
            Ok(r) => self.recording = Some((r, action, selection)),
            Err(e) => self.notify(e, true),
        }
    }

    pub fn refresh_mics(&mut self) {
        self.mics = crate::audio::input_devices();
        self.default_mic = crate::audio::default_input_name();
    }

    /// Starts or stops the microphone test in Settings.
    pub fn toggle_mic_test(&mut self) {
        if self.mic_test.take().is_some() {
            return;
        }
        match Recording::start(&self.s.microphone, self.s.input_gain) {
            Ok(r) => self.mic_test = Some(r),
            Err(e) => self.notify(e, true),
        }
    }

    pub fn cancel_recording(&mut self) {
        self.recording = None;
    }

    pub fn stop_recording(&mut self, ctx: &egui::Context) {
        let Some((rec, action, selection)) = self.recording.take() else { return };
        // Long finished by now; joining only collects it.
        let selection = selection.and_then(|h| h.join().ok()).unwrap_or_default();
        let clip = rec.finish();
        let when = settings::now_secs();
        let seconds = clip.seconds();
        if clip.is_silent() {
            if seconds >= 0.3 {
                let wav = clip.to_wav();
                self.add_failure(action, when, seconds, &selection, "Audio is silent.", &wav);
            }
            let blocked = seconds >= 0.3 && clip.samples.iter().all(|v| *v == 0.0);
            self.notify(
                if blocked {
                    "macOS is blocking the microphone. Allow Open Dictate in Privacy & Security › Microphone (Settings › Audio input)."
                } else {
                    "Audio is silent. Check your microphone in Settings › Audio input."
                },
                true,
            );
            return;
        }
        self.run_job(ctx, action, when, clip.to_wav(), seconds, selection, false);
    }

    #[allow(clippy::too_many_arguments)]
    fn run_job(
        &mut self,
        ctx: &egui::Context,
        action: Action,
        when: u64,
        wav: Vec<u8>,
        seconds: f32,
        selection: String,
        retry: bool,
    ) {
        self.busy = Some((action, Instant::now()));
        let s = self.s.clone();
        let tx = self.jobs_tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let mut text_model = engines::text_model_label(&s);
            let mut note = None;
            let (raw, result) = match engines::transcribe(&s, &wav) {
                Err(e) => (String::new(), Err(e)),
                Ok(raw) if raw.trim().is_empty() => (raw, Err("No speech detected.".into())),
                Ok(raw) => {
                    let r = match action {
                        // Dictation only needs the speech model: if the text model
                        // can't be reached, paste what was heard instead of failing.
                        Action::Dictate => engines::polish(&s, &raw, false).or_else(|e| {
                            note = Some(format!("Pasted as heard, without clean-up ({e})"));
                            text_model = "none".into();
                            Ok(raw.clone())
                        }),
                        Action::Translate => engines::polish(&s, &raw, true),
                        Action::Ask => engines::ask(&s, &selection, &raw),
                    };
                    (raw, r)
                }
            };
            let _ = tx.send(JobOut {
                action,
                when,
                raw,
                result,
                seconds,
                selection,
                wav,
                retry,
                speech_model: engines::speech_model_label(&s),
                text_model,
                note,
            });
            ctx.request_repaint();
        });
    }

    fn finish_job(&mut self, out: JobOut) {
        self.busy = None;
        match out.result {
            Ok(text) => {
                if self.s.auto_paste {
                    if let Err(e) = paste(&text, self.s.restore_clipboard) {
                        self.notify(format!("Copied, but couldn't paste: {e}"), true);
                    }
                } else {
                    let _ = arboard::Clipboard::new().and_then(|mut c| c.set_text(text.clone()));
                    self.notify("Copied to clipboard", false);
                }
                // Say it once per session; after that it's just how dictation works without a text model.
                if let Some(n) = out.note.filter(|_| !self.told_no_cleanup) {
                    self.told_no_cleanup = true;
                    self.notify(n, false);
                }
                let entry = Entry {
                    when: out.when,
                    action: out.action,
                    raw: out.raw,
                    text,
                    seconds: out.seconds,
                    speech_model: out.speech_model,
                    text_model: out.text_model,
                    error: None,
                    audio: None,
                    selection: out.selection,
                };
                self.stats.record(entry.when, entry.words(), entry.seconds);
                if out.retry {
                    if let Some(old) = self.history.iter_mut().find(|e| e.when == out.when) {
                        if let Some(a) = old.audio.take() {
                            let _ = std::fs::remove_file(a);
                        }
                        *old = entry;
                    }
                } else {
                    self.history.push(entry);
                }
                self.save_history();
            }
            Err(e) => {
                self.notify(e.clone(), true);
                if out.retry {
                    if let Some(old) = self.history.iter_mut().find(|x| x.when == out.when) {
                        old.error = Some(e);
                    }
                    self.save_history();
                } else {
                    self.add_failure(out.action, out.when, out.seconds, &out.selection, &e, &out.wav);
                }
            }
        }
    }

    fn add_failure(&mut self, action: Action, when: u64, seconds: f32, selection: &str, error: &str, wav: &[u8]) {
        let path = settings::audio_dir().join(format!("{when}.wav"));
        let audio = std::fs::write(&path, wav).ok().map(|_| path.to_string_lossy().into_owned());
        self.history.push(Entry {
            when,
            action,
            raw: String::new(),
            text: String::new(),
            seconds,
            speech_model: engines::speech_model_label(&self.s),
            text_model: engines::text_model_label(&self.s),
            error: Some(error.to_string()),
            audio,
            selection: selection.to_string(),
        });
        self.save_history();
    }

    pub fn retry(&mut self, ctx: &egui::Context, when: u64) {
        if self.busy.is_some() {
            return;
        }
        let Some(e) = self.history.iter().find(|e| e.when == when) else { return };
        let Some(wav) = e.audio.as_ref().and_then(|p| std::fs::read(p).ok()) else {
            self.notify("The recording for this item is gone.", true);
            return;
        };
        let (action, seconds, selection) = (e.action, e.seconds, e.selection.clone());
        self.run_job(ctx, action, when, wav, seconds, selection, true);
    }

    pub fn delete_entry(&mut self, when: u64) {
        if let Some(i) = self.history.iter().position(|e| e.when == when) {
            let e = self.history.remove(i);
            if let Some(a) = e.audio {
                let _ = std::fs::remove_file(a);
            }
            self.save_history();
        }
    }

    pub fn clear_history(&mut self) {
        for e in self.history.drain(..) {
            if let Some(a) = e.audio {
                let _ = std::fs::remove_file(a);
            }
        }
        self.save_history();
    }

    /// Saves a correction and learns new names/terms from it into the dictionary.
    pub fn save_edit(&mut self, when: u64, text: String) {
        let Some(e) = self.history.iter_mut().find(|e| e.when == when) else { return };
        let before: std::collections::HashSet<String> =
            e.text.split_whitespace().map(|w| clean_word(w).to_lowercase()).collect();
        let learned: Vec<String> = text
            .split_whitespace()
            .map(clean_word)
            .filter(|w| w.len() > 1 && !before.contains(&w.to_lowercase()))
            .filter(|w| w.chars().any(|c| c.is_uppercase() || c.is_ascii_digit()) || w.contains(['-', '/', '.']))
            .map(str::to_owned)
            .collect();
        e.text = text;
        let mut added = 0;
        for w in learned {
            if !self.s.dictionary.iter().any(|d| d.word.eq_ignore_ascii_case(&w)) {
                self.s.dictionary.push(DictWord { word: w, auto: true });
                added += 1;
            }
        }
        if added > 0 {
            self.notify(format!("Learned {added} new word{} for your dictionary", if added == 1 { "" } else { "s" }), false);
            self.changed();
        }
        self.save_history();
    }

    fn prune_history(&mut self) {
        if let Some(max) = self.s.keep_history.max_age() {
            let now = settings::now_secs();
            let (keep, drop): (Vec<Entry>, Vec<Entry>) =
                self.history.drain(..).partition(|e| now.saturating_sub(e.when) < max);
            for e in drop {
                if let Some(a) = e.audio {
                    let _ = std::fs::remove_file(a);
                }
            }
            self.history = keep;
            settings::save_history(&self.history);
        }
    }

    fn save_history(&mut self) {
        self.prune_history();
        settings::save_history(&self.history);
    }

    pub fn set_keep_history(&mut self, k: settings::KeepHistory) {
        self.s.keep_history = k;
        self.changed();
        self.prune_history();
    }

    // -----------------------------------------------------------------------
    // Model health

    pub fn refresh_health(&self, ctx: &egui::Context) {
        self.check_health(ctx, false);
    }

    /// The launch-time check, which also starts Ollama if it is not up.
    ///
    /// Ollama does not run on its own, so the first dictation after a reboot
    /// used to fail - or the Models tab sat on "Ollama not running" until you
    /// found the Start Ollama button and pressed it yourself. The app knows
    /// it needs the server, so it asks for it.
    pub fn check_health_and_start(&self, ctx: &egui::Context) {
        self.check_health(ctx, true);
    }

    fn check_health(&self, ctx: &egui::Context, autostart: bool) {
        let s = self.s.clone();
        let health = self.health.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let h = Health {
                whisper_bin: engines::find_whisper_bin(&s.whisper_bin),
                ollama: Some(engines::ollama_models(&s.ollama_url)),
                remote_text: (s.text_engine == TextEngine::OpenAiCompatible)
                    .then(|| engines::openai_models(&s.text_url, &s.text_key)),
                checked: true,
            };
            // Publish what we found before going off to start anything: the
            // wait below can run for seconds, and the rest of this - the
            // speech model, a remote endpoint - is known now and the window
            // is already asking for it.
            let down = s.text_engine == TextEngine::Ollama && matches!(h.ollama, Some(Err(_)));
            *health.lock().unwrap() = h;
            ctx.request_repaint();

            if autostart && down {
                let started = engines::start_ollama_and_wait(&s.ollama_url);
                health.lock().unwrap().ollama = Some(started);
                ctx.request_repaint();
            }
        });
    }

    /// (ready, short label) for the speech model.
    pub fn speech_ready(&self) -> (bool, String) {
        let h = self.health.lock().unwrap();
        let label = engines::speech_model_label(&self.s);
        match self.s.speech_engine {
            settings::SpeechEngine::WhisperLocal => {
                let ok = h.whisper_bin.is_some() && std::path::Path::new(&self.s.whisper_model).is_file();
                (ok, if ok { label } else if h.whisper_bin.is_none() { "whisper.cpp missing".into() } else { "Choose a model".into() })
            }
            settings::SpeechEngine::OpenAiCompatible => (!self.s.speech_model.trim().is_empty(), label),
        }
    }

    pub fn text_ready(&self) -> (bool, String) {
        let h = self.health.lock().unwrap();
        match self.s.text_engine {
            TextEngine::Off => (true, "Off (raw text)".into()),
            TextEngine::Ollama => match &h.ollama {
                Some(Ok(list)) => {
                    let ok = list.iter().any(|m| m == &self.s.ollama_model || m.trim_end_matches(":latest") == self.s.ollama_model);
                    (ok, if ok { self.s.ollama_model.clone() } else { format!("{} not downloaded", self.s.ollama_model.trim_end_matches(":latest")) })
                }
                Some(Err(_)) => (false, "Ollama not running".into()),
                None => (false, "Checking…".into()),
            },
            TextEngine::OpenAiCompatible => (!self.s.text_model.trim().is_empty(), self.s.text_model.clone()),
        }
    }

    pub fn run_test(&self, ctx: &egui::Context) {
        let s = self.s.clone();
        let input = self.test_input.clone();
        let out = self.test_output.clone();
        let running = self.test_running.clone();
        let ctx = ctx.clone();
        running.store(true, Ordering::SeqCst);
        std::thread::spawn(move || {
            let r = if s.text_engine == TextEngine::Off {
                Err("Text clean-up is off: your words are pasted exactly as heard.".into())
            } else {
                engines::polish(&s, &input, false)
            };
            *out.lock().unwrap() = Some(r);
            running.store(false, Ordering::SeqCst);
            ctx.request_repaint();
        });
    }

    // -----------------------------------------------------------------------
    // Floating indicator

    /// The real window, drawn as a child of the 1x1 root viewport.
    ///
    /// Closing it just stops this being called, so the app and its shortcuts
    /// carry on, and the root keeps producing the frames the indicator needs.
    /// The real window, kept alive and hidden rather than destroyed.
    ///
    /// It used to be dropped when closed and rebuilt on reopen, and the
    /// AppKit backdrop could not survive that: attaching to the new window
    /// reported success, the view was in the hierarchy and the right size,
    /// and it still drew nothing - a reopened window showed the desktop
    /// straight through its page. Hiding keeps one window, one backdrop, and
    /// nothing to rebuild.
    fn main_window(&mut self, ctx: &egui::Context) {
        let builder = egui::ViewportBuilder::default()
            .with_title("Open Dictate")
            .with_inner_size([1360.0, 920.0])
            .with_min_inner_size([980.0, 640.0])
            // The page behind the cards is painted with alpha, so the window
            // has to be able to carry it.
            .with_transparent(true);
        let focus = std::mem::take(&mut self.focus_main);
        let want_visible = self.window_open;
        let visibility_changed = self.window_shown != Some(want_visible);
        self.window_shown = Some(want_visible);
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("main"), builder, |ui, _| {
            let vctx = ui.ctx().clone();
            if visibility_changed {
                vctx.send_viewport_cmd(egui::ViewportCommand::Visible(want_visible));
                // Stepping out of the foreground as the window goes makes
                // the next Dock click an activation, which is the only
                // signal there is that the user wants it back. Not in this
                // frame though: done while the window is still going away,
                // AppKit puts the app straight back in front.
                #[cfg(target_os = "macos")]
                if want_visible {
                    crate::macos_look::activate();
                } else if !self.hooked {
                    // Only needed by the fallback that reads activation.
                    // With the delegate answering the Dock directly, pushing
                    // the app into the background buys nothing and costs the
                    // menu-bar path its chance to come forward.
                    self.resign_at = Some(std::time::Instant::now());
                }
            }
            if focus {
                vctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            // The red button asks to close; the app only hides, so the close
            // is refused and the window goes away without being destroyed.
            if vctx.input(|i| i.viewport().close_requested()) {
                vctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.window_open = false;
            }
            if want_visible {
                crate::views::draw(self, ui);
            }
        });
        if !want_visible {
            return;
        }
        // 300 is wide enough to tell the real window from the 1x1 root
        // anchor, which carries the same title. Checked every frame rather
        // than once: a window that has been closed and reopened is a new
        // window with a new number, and its backdrop has to be built again.
        #[cfg(target_os = "macos")]
        {
            let current = crate::macos_look::main_window_number(300.0);
            // Asking the view whether it is still there, rather than
            // trusting that attaching it once worked.
            if !crate::macos_look::backdrop_ok(300.0) {
                let before = self.blurred_window;
                self.blurred_window = match current {
                    Some(_) => crate::macos_look::blur_window(300.0, theme::sheer_rgba()),
                    None => None,
                };
                crate::macos_look::log_backdrop(&format!(
                    "window {before:?} -> {current:?}, attached {:?}",
                    self.blurred_window
                ));
                // While there is no backdrop, egui paints the page itself.
                // Leaving it clear is what made a reopened window show the
                // desktop straight through it.
                theme::set_backdrop(self.blurred_window.is_some());
                if self.blurred_window.is_some() && self.window_since.is_none() {
                    self.window_since = Some(std::time::Instant::now());
                }
            }
            // A window that has just appeared has not been laid out yet, so
            // the backdrop is sized against it every frame rather than once.
            crate::macos_look::fit_backdrop();
        }
        // Launch Services caches a bundle's icon against its path, so an app
        // that once had none can keep the generic tile however correct the
        // bundle becomes. Setting it on the running NSApplication skips that
        // cache, but the Dock can put the cached one back while the app is
        // still launching, so it is set again at one second and three.
        #[cfg(target_os = "macos")]
        if let Some(since) = self.window_since {
            let secs = since.elapsed().as_secs_f32();
            let want = if secs > 3.0 { 3 } else if secs > 1.0 { 2 } else { 1 };
            if self.icon_tries < want {
                self.icon_tries = want;
                crate::macos_look::set_dock_icon(include_bytes!("../assets/AppIcon.png"));
            }
        }
    }

    fn indicator(&mut self, ctx: &egui::Context) {
        // A meeting runs for an hour with the window closed. Without this
        // there is nothing on screen saying the Mac is being listened to,
        // which is both unnerving and easy to leave running by accident.
        let meeting = self.meeting.as_ref().map(|m| {
            let (elapsed, peak) = m.live.lock().map(|l| (l.elapsed, l.peak)).unwrap_or((0.0, 0.0));
            (elapsed, peak)
        });
        let state = if let Some((elapsed, peak)) = meeting {
            Some((true, None, peak, elapsed))
        } else if let Some((rec, a, _)) = &self.recording {
            Some((true, Some(*a), rec.level(), rec.elapsed()))
        } else {
            self.busy.as_ref().map(|(a, t)| (false, Some(*a), 0.0, t.elapsed().as_secs_f32()))
        };
        let Some((listening, action, level, secs)) = state else { return };
        if !self.s.show_indicator || self.shot.is_some() {
            return;
        }
        let size = egui::vec2(236.0, 46.0);
        let monitor = ctx.input(|i| i.viewport().monitor_size).unwrap_or(egui::vec2(1440.0, 900.0));
        let pos = egui::pos2((monitor.x - size.x) / 2.0, monitor.y - size.y - 90.0);
        let builder = egui::ViewportBuilder::default()
            .with_title("Listening")
            .with_inner_size(size)
            .with_position(pos)
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_active(false)
            .with_taskbar(false)
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
            .with_mouse_passthrough(true);
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("indicator"), builder, |ui, _| {
            // Inset so the hard offset shadow has room inside the viewport.
            let rect = ui.max_rect().shrink(5.0);
            let p = ui.painter();
            p.rect_filled(rect.translate(egui::vec2(4.0, 5.0)), 23.0, theme::INK());
            p.rect_filled(rect, 23.0, theme::SURFACE());
            p.rect_stroke(rect, 23.0, egui::Stroke::new(3.0, theme::INK()), egui::StrokeKind::Inside);
            let t = ui.input(|i| i.time) as f32;
            let left = rect.left() + 22.0;
            for i in 0..9 {
                let x = left + i as f32 * 6.0;
                let h = if listening {
                    let wobble = ((t * 9.0 + i as f32 * 0.9).sin() * 0.5 + 0.5) * 0.6 + 0.4;
                    (4.0 + level.min(0.5) * 2.0 * 26.0 * wobble).min(26.0)
                } else {
                    4.0 + ((t * 5.0 - i as f32 * 0.6).sin() * 0.5 + 0.5) * 10.0
                };
                // Not white: the pill is the surface colour, and in the
                // light theme that is white too, so the level meter drew
                // itself invisible. The bars are the same ink as the label
                // beside them, which is what they are - a reading, not a
                // highlight.
                let c = if listening { theme::TEXT() } else { theme::ACCENT() };
                p.rect_filled(egui::Rect::from_center_size(egui::pos2(x, rect.center().y), egui::vec2(3.0, h)), 1.5, c);
            }
            let label = match action {
                None => "Meeting".to_string(),
                Some(a) if listening => a.label().to_string(),
                Some(_) => "Thinking…".to_string(),
            };
            p.text(
                egui::pos2(left + 64.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::new(14.0, theme::medium()),
                theme::TEXT(),
            );
            p.text(
                egui::pos2(rect.right() - 20.0, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                {
                    let s = secs as u32;
                    if s >= 3600 {
                        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
                    } else {
                        format!("{}:{:02}", s / 60, s % 60)
                    }
                },
                egui::FontId::new(13.0, egui::FontFamily::Proportional),
                theme::MUTED(),
            );
        });
    }

    fn talking_window(&mut self, ctx: &egui::Context) {
        let Some(session) = &self.meeting else { return };
        let (Some(talking), Some(vs)) = (&session.talking, self.talking_view.as_mut()) else { return };
        let builder = egui::ViewportBuilder::default()
            .with_title("Talking Points")
            .with_inner_size([640.0, 460.0])
            .with_min_inner_size([480.0, 320.0])
            .with_window_level(egui::WindowLevel::AlwaysOnTop);
        let mut closed = false;
        let brave_key = self.s.brave_key.trim().to_string();
        let mut open_models = false;
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("talking"), builder, |ui, _| {
            closed = ui.input(|i| i.viewport().close_requested());
            let Some(elapsed) = session.live.lock().ok().map(|m| {
                vs.catch_up(&m);
                m.elapsed
            }) else {
                return;
            };
            let Some(snapshot) = talking.lock().ok().map(|t| t.clone()) else { return };
            match crate::talking_view::draw(ui, elapsed, &snapshot, !brave_key.is_empty(), vs) {
                Some(crate::talking_view::ViewAction::Search(text)) => {
                    let (live, key) = (talking.clone(), brave_key.clone());
                    std::thread::spawn(move || crate::search::search_into(&live, &key, &text));
                }
                Some(crate::talking_view::ViewAction::Copy(text)) => ui.ctx().copy_text(text),
                Some(crate::talking_view::ViewAction::RemoveChip(query)) => {
                    if let Ok(mut t) = talking.lock() {
                        t.chips.retain(|c| c.query != query);
                    }
                }
                Some(crate::talking_view::ViewAction::OpenModels) => open_models = true,
                None => {}
            }
        });
        if open_models {
            self.settings_tab = Some(SettingsTab::Models);
            self.window_open = true;
            self.focus_main = true;
            self.refresh_health(ctx);
        }
        if closed {
            self.talking_view = None;
        }
    }

    // -----------------------------------------------------------------------
    // Screenshot mode

    fn screenshot_step(&mut self, ctx: &egui::Context) {
        let Some(path) = self.shot.clone() else { return };
        self.frames += 1;
        ctx.request_repaint();
        if self.shot_done.load(Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.frames == 120 {
            let done = self.shot_done.clone();
            let ctx2 = ctx.clone();
            ctx.request_screenshot(move |img| {
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let [w, h] = img.size;
                if let Err(e) = image::save_buffer(&path, img.as_raw(), w as u32, h as u32, image::ExtendedColorType::Rgba8) {
                    eprintln!("could not save screenshot: {e}");
                }
                done.store(true, Ordering::SeqCst);
                ctx2.request_repaint();
            });
        }
    }
}

impl eframe::App for DictateApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // On System this follows the OS while the app is open, and it is also
        // what applies the switch in Settings a frame after it is clicked.
        let light = self.s.appearance.is_light(system_light(ctx));
        if light != theme::is_light() {
            theme::set_light(light);
            theme::apply_style(ctx);
            #[cfg(target_os = "macos")]
            crate::macos_look::set_backdrop_tint(theme::sheer_rgba());
        }
        self.collect_summary();
        self.window_lifecycle(ctx);
        while let Ok(e) = self.hk_rx.try_recv() {
            if self.capturing_key.is_none() {
                self.on_hotkey(ctx, e);
            }
        }
        if self.recording.is_some() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.cancel_recording();
        }
        while let Ok(out) = self.jobs_rx.try_recv() {
            self.finish_job(out);
        }
        if let Some((_, p)) = &self.download {
            let done = p.lock().unwrap().clone();
            if done.finished {
                let (file, _) = self.download.take().unwrap();
                match done.error {
                    Some(e) => self.notify(format!("Download failed: {e}"), true),
                    None => {
                        if file.starts_with("ollama:") {
                            self.notify("Model downloaded", false);
                        } else {
                            self.s.whisper_model = settings::models_dir().join(&file).to_string_lossy().into_owned();
                            self.changed();
                            self.notify("Speech model ready", false);
                        }
                        self.refresh_health(ctx);
                    }
                }
            }
        }
        if self.mic_test.is_some() && self.settings_tab != Some(SettingsTab::Audio) {
            self.mic_test = None;
        }
        // A meeting belongs in here too. Without it nothing asked for the
        // next frame while one was running, so the indicator's bars moved
        // only when some other event happened to wake the app up - which
        // reads as stutter rather than as a stopped animation.
        if self.recording.is_some()
            || self.busy.is_some()
            || self.download.is_some()
            || self.mic_test.is_some()
            || self.meeting.is_some()
            || self.summarising.is_some()
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
        if self.toast.as_ref().is_some_and(|t| t.at.elapsed().as_secs_f32() > 5.0) {
            self.toast = None;
        }
        if self.dirty {
            self.dirty = false;
            settings::save_settings(&self.s);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.screenshot_step(&ctx);
        if self.shot.is_some() {
            // Screenshot mode: the root really is the window being captured.
            crate::views::draw(self, ui);
        } else {
            self.main_window(&ctx);
        }
        self.indicator(&ctx);
        self.talking_window(&ctx);
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }
}

fn clean_word(w: &str) -> &str {
    w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '/')
}

fn send_combo(modifier: enigo::Key, key: char) -> Result<(), String> {
    use enigo::{Direction, Enigo, Key, Keyboard};
    let mut e = Enigo::new(&enigo::Settings::default()).map_err(|e| format!("{e} (allow Accessibility access)"))?;
    // Let go of whatever modifiers the shortcut used, so they don't mix with ours.
    for m in [Key::Alt, Key::Shift, Key::Control] {
        let _ = e.key(m, Direction::Release);
    }
    e.key(modifier, Direction::Press).map_err(|e| e.to_string())?;
    let r = e.key(Key::Unicode(key), Direction::Click).map_err(|e| e.to_string());
    let _ = e.key(modifier, Direction::Release);
    r
}

fn cmd_key() -> enigo::Key {
    if cfg!(target_os = "macos") { enigo::Key::Meta } else { enigo::Key::Control }
}

/// Copies whatever is selected in the front app (for "Ask anything").
/// Copies whatever is selected in the front app, and puts the clipboard back.
///
/// This used to empty the clipboard, sleep a flat 180ms and read it. When a
/// copy took longer than that - a big selection, a slow app, a busy machine -
/// the read came back empty, which is indistinguishable from nothing being
/// selected. Ask would then quietly write something new instead of rewriting
/// what was highlighted, with nothing to say why.
///
/// A marker tells the two apart. Anything other than the marker is the copy,
/// so it can stop the moment the copy lands rather than always waiting, and
/// the marker still sitting there means nothing was selected.
/// Puts back whatever was on the clipboard before the grab.
fn restore_clipboard(cb: &mut arboard::Clipboard, text: Option<String>, image: Option<arboard::ImageData<'static>>) {
    if let Some(t) = text {
        let _ = cb.set_text(t);
    } else if let Some(img) = image {
        let _ = cb.set_image(img);
    } else {
        let _ = cb.clear();
    }
}

/// Starts copying whatever is selected in the front app. The handle yields
/// the selection, or an empty string when there was none.
///
/// Split in two on purpose. enigo's key posting goes through Text Input
/// Sources, which assert they are on the main queue - running the whole grab
/// on a thread crashed the app with SIGTRAP the moment Ask was pressed. So
/// the keystroke happens here, on the caller's thread, and only the waiting
/// moves off it.
///
/// The wait is a poll against a marker rather than a fixed sleep. A copy that
/// takes longer than the sleep used to read back empty, which is
/// indistinguishable from nothing being selected, and Ask would quietly write
/// something new instead of rewriting what was highlighted. Anything other
/// than the marker is the copy; the marker still sitting there means there
/// was nothing to copy.
fn begin_selection_grab() -> Option<std::thread::JoinHandle<String>> {
    use std::time::{Duration, Instant};
    let mut cb = arboard::Clipboard::new().ok()?;
    let text_before = cb.get_text().ok();
    let image_before = if text_before.is_none() { cb.get_image().ok() } else { None };

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let marker = format!("\u{200B}open-dictate-selection-{stamp}");
    if cb.set_text(marker.clone()).is_err() {
        return None;
    }
    if send_combo(cmd_key(), 'c').is_err() {
        // The clipboard still holds the marker; it must not be left there.
        restore_clipboard(&mut cb, text_before, image_before);
        return None;
    }

    Some(std::thread::spawn(move || {
        let Ok(mut cb) = arboard::Clipboard::new() else { return String::new() };
        let mut found = String::new();
        // Most copies land inside 50ms; the ceiling is only ever reached when
        // nothing was selected at all, which is why this is off the main
        // thread - it would otherwise hold the microphone shut that long.
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(15));
            if let Ok(t) = cb.get_text() {
                if t != marker {
                    found = t;
                    break;
                }
            }
        }
        restore_clipboard(&mut cb, text_before, image_before);
        found
    }))
}

/// Puts the text into the front app by pasting it, then puts the old clipboard back.
fn paste(text: &str, restore: bool) -> Result<(), String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    let before = cb.get_text().ok();
    cb.set_text(text.to_string()).map_err(|e| e.to_string())?;
    std::thread::sleep(std::time::Duration::from_millis(40));
    send_combo(cmd_key(), 'v')?;
    if restore {
        if let Some(before) = before {
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(600));
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    let _ = cb.set_text(before);
                }
            });
        }
    }
    Ok(())
}
