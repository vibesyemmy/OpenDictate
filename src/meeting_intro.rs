#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn first_meeting_only_requires_intro_after_existing_recording_and_busy_checks() {
        assert_eq!(start_decision(false, false, false), StartDecision::ShowIntro);
        assert_eq!(start_decision(true, false, false), StartDecision::Stop);
        assert_eq!(start_decision(false, true, false), StartDecision::Busy);
        assert_eq!(start_decision(false, false, true), StartDecision::Start);
    }

    #[test]
    fn intro_acknowledgement_survives_settings_reload() {
        let mut settings: crate::settings::Settings = serde_json::from_str("{}").unwrap();
        assert!(!settings.meeting_intro_seen);
        acknowledge(&mut settings);
        let reloaded: crate::settings::Settings = serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert!(reloaded.meeting_intro_seen);
    }

    #[test]
    fn intro_explains_the_current_saved_audio_choice() {
        for (keep, expected) in [(true, "Speech clips will be saved for playback."), (false, "Successfully transcribed speech clips will not be saved.")] {
            let h = egui_kittest::Harness::builder().with_size(egui::vec2(650.0, 450.0))
                .with_context_setup(crate::theme::install)
                .build_ui(|ui| { draw(ui, &crate::settings::Settings { keep_meeting_audio: keep, ..Default::default() }); });
            h.get_by_label(expected);
            h.get_by_label_contains("Speech that cannot be transcribed is kept");
            h.get_by_label("Privacy & data");
            h.get_by_label_contains("not a complete recording");
            h.get_by_label("START MEETING");
            h.get_by_label("NOT NOW");
        }
    }

    #[test]
    fn start_is_only_requested_by_the_explicit_start_button() {
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(650.0, 450.0))
            .with_context_setup(crate::theme::install)
            .build_ui_state(|ui, choice| {
                if let Some(action) = draw(ui, &crate::settings::Settings::default()) { *choice = Some(action); }
            }, None::<Choice>);
        h.get_by_label("NOT NOW").click();
        h.run();
        assert_eq!(*h.state(), Some(Choice::NotNow));
    }

    #[test]
    fn intro_can_start_or_open_audio_settings() {
        for (label, expected) in [("START MEETING", Choice::Start), ("Audio settings", Choice::AudioSettings)] {
            let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(650.0, 450.0))
                .with_context_setup(crate::theme::install)
                .build_ui_state(|ui, choice| {
                    if let Some(action) = draw(ui, &crate::settings::Settings { keep_meeting_audio: true, ..Default::default() }) { *choice = Some(action); }
                }, None::<Choice>);
            h.get_by_label(label).click();
            h.run();
            assert_eq!(*h.state(), Some(expected));
        }
    }
}
use crate::theme::{self, b, para};
use crate::views::{self, Btn};
use eframe::egui;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartDecision { Stop, Busy, ShowIntro, Start }

pub fn start_decision(recording: bool, busy: bool, seen: bool) -> StartDecision {
    if recording { StartDecision::Stop }
    else if busy { StartDecision::Busy }
    else if !seen { StartDecision::ShowIntro }
    else { StartDecision::Start }
}

pub fn acknowledge(settings: &mut crate::settings::Settings) {
    settings.meeting_intro_seen = true;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice { Start, NotNow, AudioSettings, PrivacySettings }

pub fn draw(ui: &mut egui::Ui, settings: &crate::settings::Settings) -> Option<Choice> {
    let keep_audio = settings.keep_meeting_audio;
    ui.set_max_width(460.0);
    ui.label(b("Before your first meeting", 20.0, theme::TEXT()));
    ui.add_space(10.0);
    ui.label(para("Open Dictate captures your microphone and audio playing on this Mac to make a transcript.", 14.0, theme::MUTED()));
    ui.add_space(12.0);
    ui.label(b("Your audio setting", 13.0, theme::TEXT()));
    ui.add_space(4.0);
    ui.label(para(if keep_audio { "Speech clips will be saved for playback." } else { "Successfully transcribed speech clips will not be saved." }, 14.0, theme::TEXT()));
    ui.add_space(6.0);
    ui.label(para("Speech that cannot be transcribed is kept until you retry it successfully, discard it, or delete the meeting.", 13.0, theme::MUTED()));
    ui.add_space(6.0);
    ui.label(para("Saved clips are not a complete recording: quiet or skipped parts may be missing, and a clip may contain only one side.", 13.5, theme::MUTED()));
    ui.add_space(10.0);
    ui.label(para(format!("Voice to text: {}. Text clean-up and summaries: {}.", crate::privacy::speech_place(settings), crate::privacy::text_place(settings)), 12.5, theme::MUTED()));
    ui.add_space(6.0);
    let settings = views::link(ui, "Audio settings");
    settings.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Audio settings"));
    if settings.clicked() { return Some(Choice::AudioSettings); }
    let privacy = views::link(ui, "Privacy & data");
    privacy.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Privacy & data"));
    if privacy.clicked() { return Some(Choice::PrivacySettings); }
    ui.add_space(16.0);
    let mut choice = None;
    ui.horizontal(|ui| {
        let cancel = views::kit_button(ui, "Not now", Btn::Secondary, 100.0);
        cancel.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "NOT NOW"));
        if cancel.clicked() { choice = Some(Choice::NotNow); }
        ui.add_space(8.0);
        let start = views::kit_button(ui, "Start meeting", Btn::Primary, 140.0);
        start.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "START MEETING"));
        if start.clicked() { choice = Some(Choice::Start); }
    });
    choice
}

