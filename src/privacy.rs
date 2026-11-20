#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{Settings, SpeechEngine, TextEngine};
    #[test]
    fn plain_places_say_on_this_mac_only_when_it_is_true() {
        let mut s = Settings::default();
        s.ollama_url = "http://localhost:11434".into();
        s.text_engine = TextEngine::Ollama;
        assert_eq!(speech_place(&s), "On this Mac");
        assert_eq!(text_place(&s), "On this Mac (Ollama)");
        assert!(!leaves_this_mac(&s));

        s.speech_engine = SpeechEngine::OpenAiCompatible;
        s.speech_url = "https://user:secret@api.openai.com/v1?token=abc".into();
        assert_eq!(speech_place(&s), "Sent to api.openai.com");
        assert!(!speech_place(&s).contains("secret") && !speech_place(&s).contains("token"));
        assert!(leaves_this_mac(&s));

        s.text_engine = TextEngine::OpenAiCompatible;
        s.text_url = "https://api.groq.com/openai/v1".into();
        assert_eq!(text_place(&s), "Sent to api.groq.com");
        s.text_url = String::new();
        assert_eq!(text_place(&s), "Not set up yet");
        s.text_engine = TextEngine::Off;
        assert_eq!(text_place(&s), "Off");
    }
}

use crate::settings::{Settings, SpeechEngine, TextEngine};

/// Where a server is, in plain words: "On this Mac" only when it is.
fn place(url: &str) -> String {
    let host = crate::talking::host_of(url);
    if host.is_empty() { "Not set up yet".into() }
    else if crate::talking::is_local(url) { "On this Mac".into() }
    else { format!("Sent to {host}") }
}

/// Where speech is turned into text.
pub fn speech_place(s: &Settings) -> String {
    match s.speech_engine {
        SpeechEngine::WhisperLocal => "On this Mac".into(),
        SpeechEngine::OpenAiCompatible => place(&s.speech_url),
    }
}

/// Where text is cleaned up, translated and summarised.
pub fn text_place(s: &Settings) -> String {
    match s.text_engine {
        TextEngine::Off => "Off".into(),
        TextEngine::Ollama => match place(&s.ollama_url).as_str() {
            "On this Mac" => "On this Mac (Ollama)".into(),
            other => other.into(),
        },
        TextEngine::OpenAiCompatible if crate::lms::is_lm_studio(s) => "On this Mac (LM Studio)".into(),
        TextEngine::OpenAiCompatible => place(&s.text_url),
    }
}

/// True when anything is sent to a server outside this Mac.
fn leaves_this_mac(s: &Settings) -> bool {
    speech_place(s).starts_with("Sent to") || text_place(s).starts_with("Sent to")
}

/// One fact: a short label on the left, the answer beside it.
fn fact(ui: &mut eframe::egui::Ui, label: &str, value: impl Into<String>) {
    use crate::theme::{self, m, para};
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        ui.allocate_ui(eframe::egui::vec2(210.0, 0.0), |ui| {
            ui.set_width(210.0);
            ui.label(m(label, 13.5, theme::TEXT()));
        });
        // A column of its own, so a long answer wraps inside the sheet
        // instead of running off its edge.
        let width = ui.available_width();
        ui.vertical(|ui| {
            ui.set_width(width);
            ui.label(para(value.into(), 13.5, theme::MUTED()));
        });
    });
    ui.add_space(6.0);
}

/// `stored` draws the storage bar and history setting at the top of "What
/// is stored", where people look for what takes space and how long it stays.
pub fn draw(ui: &mut eframe::egui::Ui, s: &Settings, stored: impl FnOnce(&mut eframe::egui::Ui)) {
    use crate::theme::{self, b, para};
    ui.add_space(18.0);
    ui.label(b("Where your data goes", 15.0, theme::TEXT()));
    ui.add_space(8.0);
    fact(ui, "Voice to text", speech_place(s));
    fact(ui, "Text clean-up and summaries", text_place(s));
    fact(ui, "Web search", if s.brave_key.trim().is_empty() { "Off".to_string() } else { "Only when you ask. Sends the selected text to Brave.".to_string() });
    // Talking Points only matters here when it could send a transcript away.
    let text = text_place(s);
    if s.talking_points && text.starts_with("Sent to") {
        let host = text.trim_start_matches("Sent to ");
        fact(ui, "Talking Points", format!("Sends parts of the meeting transcript to {host}, only if you have allowed that server."));
    }
    ui.add_space(14.0);
    ui.label(b("What is stored", 15.0, theme::TEXT()));
    stored(ui);
    ui.add_space(10.0);
    fact(ui, "Dictation history", "Kept for as long as your setting above says.");
    fact(ui, "Meetings", if s.keep_meeting_audio {
        "Transcripts and notes stay until you delete the meeting. Speech clips are kept for playback."
    } else {
        "Transcripts and notes stay until you delete the meeting. Speech clips are kept only if \u{201c}Keep speech clips\u{201d} is on."
    });
    fact(ui, "Speech that failed to transcribe", "Kept until you retry or discard it.");
    ui.add_space(14.0);
    ui.label(b("Deleting", 15.0, theme::TEXT()));
    ui.add_space(4.0);
    ui.label(para("Deleting anything in Open Dictate is permanent. It doesn't go to the Trash.", 13.5, theme::TEXT()));
    if leaves_this_mac(s) || !s.brave_key.trim().is_empty() {
        ui.label(para("Services outside this Mac, like your model provider or Brave, keep data under their own policies.", 13.0, theme::MUTED()));
    }
}
