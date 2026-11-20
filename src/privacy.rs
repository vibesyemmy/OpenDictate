#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{Settings, SpeechEngine, TextEngine};
    #[test]
    fn local_and_remote_destinations_are_described_without_credentials() {
        let mut s = Settings::default();
        assert!(speech_destination(&s).contains("on this Mac"));
        s.speech_engine = SpeechEngine::OpenAiCompatible;
        s.speech_url = "https://user:secret@example.com:443/v1?token=abc".into();
        let speech = speech_destination(&s);
        assert!(speech.contains("example.com"));
        assert!(!speech.contains("secret") && !speech.contains("token"));
        s.text_engine = TextEngine::OpenAiCompatible;
        s.text_url = "http://localhost:1234/v1".into();
        assert!(text_destination(&s).contains("on this Mac"));
        s.text_url = "http://192.168.1.2:1234/v1".into();
        assert!(text_destination(&s).contains("192.168.1.2"));
        assert!(!text_destination(&s).contains("on this Mac"));
    }
}

use crate::settings::{Settings, SpeechEngine, TextEngine};

fn destination(url: &str) -> String {
    let host = crate::talking::host_of(url);
    if host.is_empty() { "No server address configured".into() }
    else if crate::talking::is_local(url) { format!("Server on this Mac ({host})") }
    else { format!("Server at {host} (may be outside this Mac)") }
}

pub fn speech_destination(s: &Settings) -> String {
    match s.speech_engine {
        SpeechEngine::WhisperLocal => "whisper.cpp on this Mac".into(),
        SpeechEngine::OpenAiCompatible => destination(&s.speech_url),
    }
}

pub fn text_destination(s: &Settings) -> String {
    match s.text_engine {
        TextEngine::Off => "Off; no text model selected".into(),
        TextEngine::Ollama => destination(&s.ollama_url),
        TextEngine::OpenAiCompatible => destination(&s.text_url),
    }
}

pub fn draw(ui: &mut eframe::egui::Ui, s: &Settings) {
    use crate::theme::{self, b, para};
    ui.add_space(18.0);
    ui.label(b("Where processing happens", 15.0, theme::TEXT()));
    ui.add_space(5.0);
    ui.label(para(format!("Speech audio for dictation and meetings: {}.", speech_destination(s)), 13.5, theme::TEXT()));
    ui.label(para(format!("Dictated text and meeting transcripts for text features and summaries: {}.", text_destination(s)), 13.5, theme::TEXT()));
    ui.label(para("Talking Points sends transcript excerpts to the text provider when enabled; servers outside this Mac require the existing per-host opt-in. Searches send only the selected text to Brave when you request a search.", 13.0, theme::MUTED()));
    ui.add_space(14.0);
    ui.label(b("What is stored", 15.0, theme::TEXT()));
    ui.label(para(format!("Dictation history is stored on this Mac under your history setting: {}. Failed dictation may include audio for retry.", s.keep_history.label()), 13.0, theme::TEXT()));
    ui.label(para(if s.keep_meeting_audio { "Meeting transcripts and notes stay until you delete the meeting. Speech clips are saved for playback; they are not a complete recording." } else { "Meeting transcripts and notes stay until you delete the meeting. Speech clips are normally discarded after transcription." }, 13.0, theme::TEXT()));
    ui.label(para("Speech that could not be transcribed is kept on this Mac until recovery succeeds or you discard it or delete the meeting. Meeting diagnostic logs contain error details; older logs may also contain transcript excerpts.", 13.0, theme::TEXT()));
    ui.add_space(14.0);
    ui.label(b("Removing data", 15.0, theme::TEXT()));
    ui.label(para("Delete meetings from Meetings and manage dictation retention in History. Deleting a meeting, history entry or discarded audio removes it from this Mac permanently; it does not go to the Trash and cannot be restored. Model providers and Brave may have their own retention policies.", 13.0, theme::TEXT()));
}
