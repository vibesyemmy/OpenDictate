//! Everything the person controls: which models run, how text is tidied, hotkeys,
//! the personal dictionary and the dictation history. Stored as JSON in the user's
//! application-support folder.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum SpeechEngine {
    /// whisper.cpp's `whisper-cli` with a local ggml model file.
    WhisperLocal,
    /// Any server speaking the OpenAI `/audio/transcriptions` API
    /// (OpenAI, Groq, a local faster-whisper server, LocalAI...).
    OpenAiCompatible,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum TextEngine {
    /// No AI tidy-up: paste the raw transcript.
    Off,
    /// A local Ollama server.
    Ollama,
    /// Any server speaking the OpenAI `/chat/completions` API (LM Studio, OpenAI, OpenRouter...).
    OpenAiCompatible,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum Tone {
    Natural,
    Professional,
    Casual,
    Concise,
}

impl Tone {
    pub const ALL: [Tone; 4] = [Tone::Natural, Tone::Professional, Tone::Casual, Tone::Concise];
    pub fn label(self) -> &'static str {
        match self {
            Tone::Natural => "Natural",
            Tone::Professional => "Professional",
            Tone::Casual => "Casual",
            Tone::Concise => "Concise",
        }
    }
    pub fn blurb(self) -> &'static str {
        match self {
            Tone::Natural => "Sounds like you, just cleaner.",
            Tone::Professional => "Polished for work email and docs.",
            Tone::Casual => "Relaxed, for chats and messages.",
            Tone::Concise => "Short and to the point.",
        }
    }
    pub fn instruction(self) -> &'static str {
        match self {
            Tone::Natural => "Keep the speaker's own voice and word choice; only make it read cleanly.",
            Tone::Professional => "Use a clear, professional tone suitable for work email and documents.",
            Tone::Casual => "Use a relaxed, friendly tone suitable for chat messages.",
            Tone::Concise => "Make it as short as possible while keeping every point.",
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct DictWord {
    pub word: String,
    /// Learned from the person's corrections rather than typed in.
    pub auto: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum KeepHistory {
    Forever,
    Days30,
    Days7,
    Day1,
    Never,
}

impl KeepHistory {
    pub const ALL: [KeepHistory; 5] =
        [KeepHistory::Forever, KeepHistory::Days30, KeepHistory::Days7, KeepHistory::Day1, KeepHistory::Never];
    pub fn label(self) -> &'static str {
        match self {
            KeepHistory::Forever => "Forever",
            KeepHistory::Days30 => "30 days",
            KeepHistory::Days7 => "7 days",
            KeepHistory::Day1 => "24 hours",
            KeepHistory::Never => "Don't keep",
        }
    }
    /// Maximum age in seconds, None = forever.
    pub fn max_age(self) -> Option<u64> {
        match self {
            KeepHistory::Forever => None,
            KeepHistory::Days30 => Some(30 * 86_400),
            KeepHistory::Days7 => Some(7 * 86_400),
            KeepHistory::Day1 => Some(86_400),
            KeepHistory::Never => Some(0),
        }
    }
}

/// Paper & Ink both ways up. The kit ships a light set of tokens and a dark
/// twin; this picks which one the app wears.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum Appearance {
    System,
    Dark,
    Light,
}

impl Appearance {
    pub const ALL: [Appearance; 3] = [Appearance::System, Appearance::Dark, Appearance::Light];
    pub fn label(self) -> &'static str {
        match self {
            Appearance::System => "System",
            Appearance::Dark => "Dark",
            Appearance::Light => "Light",
        }
    }
    /// `system_light` is what the OS is set to, which is only known once a
    /// window exists; before that it reads as None and this falls to dark.
    pub fn is_light(self, system_light: Option<bool>) -> bool {
        match self {
            Appearance::System => system_light.unwrap_or(false),
            Appearance::Dark => false,
            Appearance::Light => true,
        }
    }
}

/// The three things a shortcut can start.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Hash)]
pub enum Action {
    Dictate,
    Translate,
    Ask,
}

impl Action {
    pub const ALL: [Action; 3] = [Action::Dictate, Action::Translate, Action::Ask];
    pub fn label(self) -> &'static str {
        match self {
            Action::Dictate => "Dictate",
            Action::Translate => "Translate",
            Action::Ask => "Ask anything",
        }
    }
    pub fn verb(self) -> &'static str {
        match self {
            Action::Dictate => "dictation",
            Action::Translate => "translation",
            Action::Ask => "asking anything",
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Settings {
    pub speech_engine: SpeechEngine,
    pub whisper_bin: String,
    pub whisper_model: String,
    pub speech_url: String,
    pub speech_key: String,
    pub speech_model: String,
    /// "auto" or a language code such as "en".
    pub language: String,

    pub text_engine: TextEngine,
    pub ollama_url: String,
    pub ollama_model: String,
    /// The model meetings are summarised with, when it differs from the
    /// clean-up one. Summarising runs once at the end of a meeting, so it can
    /// afford a bigger model than dictation can; empty means use the same.
    #[serde(default)]
    pub meeting_model: String,
    pub text_url: String,
    pub text_key: String,
    pub text_model: String,

    pub talking_points: bool,
    pub talking_cloud_host: String,
    pub brave_key: String,

    pub tone: Tone,
    pub remove_fillers: bool,
    pub fix_corrections: bool,
    pub auto_format: bool,
    /// Language the Translate shortcut writes in.
    pub translate_to: String,
    pub custom_instructions: String,

    pub dictate_keys: Vec<String>,
    /// Meeting mode's own shortcut. Not an Action: the other three dictate
    /// something into the app in front, and a meeting does not.
    #[serde(default = "meeting_default")]
    pub meeting_keys: Vec<String>,
    pub translate_keys: Vec<String>,
    pub ask_keys: Vec<String>,
    /// true = hold the shortcut while speaking; false = press to start, press again to stop.
    pub hold_to_talk: bool,
    /// Microphone name; empty = whatever the system uses.
    pub microphone: String,
    /// Multiplier for quiet microphones.
    pub input_gain: f32,
    pub auto_paste: bool,
    pub restore_clipboard: bool,
    pub show_indicator: bool,
    /// Keeps each meeting stretch's audio instead of deleting it once it has
    /// been transcribed. A meeting uses about 1.9 MB a minute.
    #[serde(default)]
    pub keep_meeting_audio: bool,
    /// Other voices are also audible through the selected microphone.
    #[serde(default)]
    pub meeting_mixed_microphone: bool,
    /// The first-meeting explanation is shown only until dismissed.
    #[serde(default)]
    pub meeting_intro_seen: bool,
    /// Open at login. Off for anyone upgrading: a background app that starts
    /// itself without being asked is the sort of thing people uninstall over.
    #[serde(default)]
    pub start_at_login: bool,
    /// Set when someone declines the optional text model, so the notice about
    /// it stops asking. The notice that blocks dictation is not dismissible.
    pub hide_text_model_notice: bool,
    pub typing_wpm: f32,
    pub keep_history: KeepHistory,
    #[serde(default = "system")]
    pub appearance: Appearance,

    pub dictionary: Vec<DictWord>,
}

fn meeting_default() -> Vec<String> {
    // Preserve the old default for existing settings that predate this field.
    vec!["F16".into()]
}

#[cfg(test)]
mod shortcut_default_tests {
    use super::*;

    #[test]
    fn new_users_get_distinct_shortcuts_for_their_platform() {
        let settings = Settings::default();
        if cfg!(target_os = "macos") {
            assert_eq!(settings.dictate_keys, ["RightOption"]);
            assert_eq!(settings.translate_keys, ["RightOption+T"]);
            assert_eq!(settings.ask_keys, ["RightOption+A"]);
            assert_eq!(settings.meeting_keys, ["RightOption+M"]);
            for trigger in Trigger::ALL { assert!(crate::right_option::parse(&settings.trigger_keys(trigger)[0]).is_some()); }
        } else {
            for trigger in Trigger::ALL { assert!(settings.trigger_keys(trigger)[0].parse::<global_hotkey::hotkey::HotKey>().is_ok()); }
        }
    }

    #[test]
    fn loading_existing_settings_keeps_their_shortcuts() {
        let mut saved = serde_json::to_value(Settings::default()).unwrap();
        saved["dictate_keys"] = serde_json::json!(["F19", "Alt+Space"]);
        saved["translate_keys"] = serde_json::json!(["F18"]);
        saved["ask_keys"] = serde_json::json!(["F17"]);
        saved.as_object_mut().unwrap().remove("meeting_keys");
        let restored: Settings = serde_json::from_value(saved).unwrap();
        assert_eq!(restored.dictate_keys, ["F19", "Alt+Space"]);
        assert_eq!(restored.translate_keys, ["F18"]);
        assert_eq!(restored.ask_keys, ["F17"]);
        assert_eq!(restored.meeting_keys, ["F16"]);
    }
}

fn system() -> Appearance {
    Appearance::System
}

/// Anything a shortcut can set off. The three actions dictate into the app
/// in front; a meeting records the room instead, so it is not one of them,
/// but it still needs a key, a label and a row in Settings.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trigger {
    Act(Action),
    Meeting,
}

impl Trigger {
    pub const ALL: [Trigger; 4] = [
        Trigger::Act(Action::Dictate),
        Trigger::Act(Action::Translate),
        Trigger::Act(Action::Ask),
        Trigger::Meeting,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Trigger::Act(a) => a.label(),
            Trigger::Meeting => "Meeting",
        }
    }
}

impl Settings {
    pub fn trigger_keys(&self, t: Trigger) -> &Vec<String> {
        match t {
            Trigger::Act(a) => self.keys(a),
            Trigger::Meeting => &self.meeting_keys,
        }
    }
    pub fn trigger_keys_mut(&mut self, t: Trigger) -> &mut Vec<String> {
        match t {
            Trigger::Act(a) => self.keys_mut(a),
            Trigger::Meeting => &mut self.meeting_keys,
        }
    }
    pub fn keys(&self, a: Action) -> &Vec<String> {
        match a {
            Action::Dictate => &self.dictate_keys,
            Action::Translate => &self.translate_keys,
            Action::Ask => &self.ask_keys,
        }
    }
    pub fn keys_mut(&mut self, a: Action) -> &mut Vec<String> {
        match a {
            Action::Dictate => &mut self.dictate_keys,
            Action::Translate => &mut self.translate_keys,
            Action::Ask => &mut self.ask_keys,
        }
    }
    pub fn words(&self) -> Vec<String> {
        self.dictionary.iter().map(|w| w.word.clone()).collect()
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            speech_engine: SpeechEngine::WhisperLocal,
            whisper_bin: "whisper-cli".into(),
            whisper_model: String::new(),
            speech_url: "https://api.openai.com/v1".into(),
            speech_key: String::new(),
            speech_model: "whisper-1".into(),
            language: "auto".into(),

            text_engine: TextEngine::Ollama,
            ollama_url: "http://localhost:11434".into(),
            ollama_model: "llama3.2:3b".into(),
            meeting_model: String::new(),
            text_url: "http://localhost:1234/v1".into(),
            text_key: String::new(),
            text_model: String::new(),

            talking_points: true,
            talking_cloud_host: String::new(),
            brave_key: String::new(),

            tone: Tone::Natural,
            remove_fillers: true,
            fix_corrections: true,
            auto_format: true,
            translate_to: "Spanish".into(),
            custom_instructions: String::new(),

            // One family on the right Option key, which nothing in macOS uses
            // on its own: hold or tap it to dictate, and add a letter for the
            // rest. The left Option key keeps typing special characters.
            dictate_keys: vec![if cfg!(target_os = "macos") { "RightOption" } else { "Alt+Cmd+1" }.into()],
            meeting_keys: vec![if cfg!(target_os = "macos") { "RightOption+M" } else { "Alt+Cmd+4" }.into()],
            translate_keys: vec![if cfg!(target_os = "macos") { "RightOption+T" } else { "Alt+Cmd+2" }.into()],
            ask_keys: vec![if cfg!(target_os = "macos") { "RightOption+A" } else { "Alt+Cmd+3" }.into()],
            hold_to_talk: false,
            microphone: String::new(),
            input_gain: 1.0,
            auto_paste: true,
            restore_clipboard: true,
            show_indicator: true,
            keep_meeting_audio: false,
            meeting_mixed_microphone: false,
            meeting_intro_seen: false,
            start_at_login: false,
            hide_text_model_notice: false,
            typing_wpm: 40.0,
            keep_history: KeepHistory::Forever,
            appearance: Appearance::System,

            dictionary: Vec::new(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Entry {
    /// Unix seconds.
    pub when: u64,
    pub action: Action,
    pub raw: String,
    pub text: String,
    /// Length of the recording in seconds.
    pub seconds: f32,
    pub speech_model: String,
    pub text_model: String,
    /// Why it failed, if it did ("Audio is silent.").
    #[serde(default)]
    pub error: Option<String>,
    /// Saved recording for failed entries, so they can be retried.
    #[serde(default)]
    pub audio: Option<String>,
    /// Selected text an "Ask anything" request worked on.
    #[serde(default)]
    pub selection: String,
}

impl Entry {
    pub fn words(&self) -> usize {
        if self.error.is_some() { 0 } else { self.raw.split_whitespace().count() }
    }
}

pub fn audio_dir() -> PathBuf {
    let dir = data_dir().join("audio");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn data_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Open Dictate")
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or(home)
            .join("Open Dictate")
    } else {
        home.join(".local/share/open-dictate")
    };
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn models_dir() -> PathBuf {
    let dir = data_dir().join("models");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn load<T: for<'de> Deserialize<'de> + Default>(name: &str) -> T {
    std::fs::read_to_string(data_dir().join(name))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save<T: Serialize>(name: &str, value: &T) {
    if let Ok(s) = serde_json::to_string_pretty(value) {
        let path = data_dir().join(name);
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(tmp, path);
        }
    }
}

pub fn load_settings() -> Settings {
    load("settings.json")
}
pub fn save_settings(s: &Settings) {
    save("settings.json", s)
}
pub fn try_save_settings(s: &Settings) -> Result<(), String> {
    let body = serde_json::to_vec_pretty(s).map_err(|e| format!("Could not save settings: {e}"))?;
    let path = data_dir().join("settings.json");
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("Could not save settings: {e}"))?;
    std::fs::rename(tmp, path).map_err(|e| format!("Could not save settings: {e}"))
}
pub fn load_history() -> Vec<Entry> {
    load("history.json")
}
pub fn try_save_history(h: &[Entry]) -> Result<(), String> {
    let body = serde_json::to_vec_pretty(h).map_err(|e| format!("Could not save history: {e}"))?;
    let path = data_dir().join("history.json");
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, body).map_err(|e| format!("Could not save history: {e}"))?;
    std::fs::rename(tmp, path).map_err(|e| format!("Could not save history: {e}"))
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
