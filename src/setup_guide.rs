#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_guides_the_user_to_the_first_missing_requirement() {
        assert_eq!(next_step(false, false), SetupStep::Engine);
        assert_eq!(next_step(true, false), SetupStep::Model);
        assert_eq!(next_step(true, true), SetupStep::Microphone);
    }

    #[test]
    fn practice_returns_text_in_the_guide_and_never_sends_it_to_the_front_app() {
        let mut pending = true;
        let mut text = String::new();
        assert!(take_practice(&mut pending, &mut text, &Ok("Hello there".into())));
        assert!(!pending);
        assert_eq!(text, "Hello there");
        assert!(!take_practice(&mut pending, &mut text, &Ok("Normal dictation".into())));
    }

    #[test]
    fn practice_errors_remain_visible_without_pasting() {
        let mut pending = true;
        let mut text = String::new();
        assert!(take_practice(&mut pending, &mut text, &Err("Microphone unavailable".into())));
        assert_eq!(text, "Microphone unavailable");
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum SetupStep { Engine, Model, Microphone }

pub fn next_step(engine_ready: bool, model_ready: bool) -> SetupStep {
    if !engine_ready { SetupStep::Engine }
    else if !model_ready { SetupStep::Model }
    else { SetupStep::Microphone }
}

pub fn take_practice(pending: &mut bool, text: &mut String, result: &Result<String, String>) -> bool {
    if !*pending { return false; }
    *pending = false;
    *text = match result {
        Ok(value) if value.trim().is_empty() => "No speech detected. Try again.".into(),
        Ok(value) => value.clone(),
        Err(error) => error.clone(),
    };
    true
}

pub fn show(app: &mut crate::app::DictateApp, ctx: &eframe::egui::Context) {
    use crate::theme::{self, b, m, para, t};
    use crate::views::{self, Btn, State};
    use crate::settings::SpeechEngine;
    use eframe::egui::{self, Margin};
    if !app.setup_open { return; }
    let mut close = false;
    let mut advanced = None;
    let response = egui::Modal::new(egui::Id::new("first-dictation-setup"))
        .frame(theme::ink_panel().inner_margin(Margin::same(22)))
        .show(ctx, |ui| {
            ui.set_width(650.0_f32.min((ui.ctx().content_rect().width() - 80.0).max(400.0)));
            ui.label(b("Set up your first dictation", 22.0, theme::TEXT()));
            ui.label(para("Get ready in a few steps. Your practice stays in this window.", 13.5, theme::MUTED()));
            ui.add_space(10.0);
            egui::ScrollArea::vertical().max_height((ui.ctx().content_rect().height() - 210.0).max(250.0)).show(ui, |ui| {
                let local = app.s.speech_engine == SpeechEngine::WhisperLocal;
                let health = app.health.lock().unwrap().clone();
                let engine_ready = !local || health.whisper_bin.is_some();
                let model_ready = if local { std::path::Path::new(&app.s.whisper_model).is_file() } else { !app.s.speech_model.trim().is_empty() };
                let step = next_step(engine_ready, model_ready);

                ui.label(m("1  Speech engine", 15.0, theme::TEXT()));
                if !health.checked && local {
                    ui.label(t("Checking this Mac…", 13.0, theme::MUTED()));
                } else if engine_ready {
                    views::status(ui, State::Ok, if local { "Speech engine ready" } else { "Speech server configured; practice below to test it" });
                } else {
                    views::status(ui, State::Warn, "Speech engine not found");
                    ui.label(para("The speech engine turns your voice into text. If it is not bundled with your app, install whisper.cpp, then check again.", 13.0, theme::MUTED()));
                    ui.horizontal(|ui| {
                        if views::btn(ui, "Copy install command").clicked() {
                            ui.ctx().copy_text("brew install whisper-cpp".to_string());
                            app.notify("Install command copied", false);
                        }
                        if views::btn(ui, "Installation help").clicked() {
                            let _ = std::process::Command::new("open").arg("https://github.com/ggml-org/whisper.cpp#quick-start").spawn();
                        }
                        if views::btn(ui, "Check again").clicked() { app.refresh_health(ctx); }
                    });
                }
                ui.add_space(18.0);

                ui.label(m("2  Speech model", 15.0, theme::TEXT()));
                if model_ready {
                    views::status(ui, State::Ok, format!("Selected: {}", crate::engines::speech_model_label(&app.s)));
                } else if local {
                    ui.label(para("Recommended: Large v3 Turbo (compressed), 574 MB. It supports many languages and offers a good balance of quality and size.", 13.0, theme::MUTED()));
                    let file = "ggml-large-v3-turbo-q5_0.bin";
                    if let Some((_, progress)) = &app.download {
                        let p = progress.lock().unwrap();
                        let fraction = if p.total > 0 { p.done as f32 / p.total as f32 } else { 0.0 };
                        views::progress(ui, "Downloading", &format!("{:.0}%", fraction * 100.0), fraction, 220.0);
                    } else if crate::settings::models_dir().join(file).is_file() {
                        if views::btn(ui, "Use downloaded model").clicked() {
                            app.s.whisper_model = crate::settings::models_dir().join(file).to_string_lossy().into_owned();
                            app.changed();
                        }
                    } else if views::btn(ui, "Download recommended model").clicked() {
                        let progress: crate::engines::SharedProgress = Default::default();
                        crate::engines::download_whisper(file, progress.clone(), ctx.clone());
                        app.download = Some((file.to_string(), progress));
                    }
                } else {
                    ui.label(para("Choose a model name in advanced Models settings.", 13.0, theme::MUTED()));
                }
                ui.add_space(18.0);

                ui.label(m("3  Microphone and permissions", 15.0, theme::TEXT()));
                ui.label(para("Test your microphone and allow access when macOS asks. Shortcuts and automatic paste may also need Accessibility or Input Monitoring access.", 13.0, theme::MUTED()));
                ui.horizontal(|ui| {
                    if views::btn(ui, if app.mic_test.is_some() { "Stop microphone test" } else { "Test microphone" }).clicked() { app.toggle_mic_test(); }
                    if views::btn(ui, "Microphone settings").clicked() { crate::settings_view::open_mic_privacy(); }
                    if views::btn(ui, "Accessibility settings").clicked() {
                        let _ = std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility").spawn();
                    }
                });
                if let Some(recording) = &app.mic_test {
                    ui.label(t(if recording.peak() > 0.001 { "Hearing sound from your microphone." } else { "Speak and check for a response…" }, 13.0, theme::MUTED()));
                }
                ui.add_space(18.0);

                ui.label(m("4  Try it here", 15.0, theme::TEXT()));
                let key = if app.s.dictate_keys.is_empty() { "your shortcut".into() } else { app.s.dictate_keys.iter().map(|k| views::pretty_key(k)).collect::<Vec<_>>().join(" or ") };
                ui.label(para(format!("Press {key} or use the button, say a sentence, then stop. The result stays here; it will not be pasted into another app."), 13.0, theme::MUTED()));
                ui.horizontal(|ui| {
                    let label = if app.setup_practice_recording { "Stop practice" } else if app.setup_practice_pending { "Transcribing…" } else { "Start practice" };
                    if ui.add_enabled(step == SetupStep::Microphone && !app.setup_practice_pending, egui::Button::new(label)).clicked() {
                        if app.setup_practice_recording { app.stop_recording(ctx); } else { app.start_setup_practice(); }
                    }
                    if views::btn(ui, "Change shortcut").clicked() { advanced = Some(crate::app::SettingsTab::General); }
                });
                ui.add(egui::TextEdit::multiline(&mut app.setup_practice_text).hint_text("Your practice text appears here").desired_width(ui.available_width()).desired_rows(3));
                ui.add_space(12.0);
                if views::link(ui, "Advanced model settings").clicked() { advanced = Some(crate::app::SettingsTab::Models); }
            });
            ui.add_space(12.0);
            if views::kit_button(ui, "Done", Btn::Primary, 110.0).clicked() { close = true; }
        });
    if response.should_close() { close = true; }
    if close || advanced.is_some() {
        if app.setup_practice_recording { app.cancel_recording(); }
        app.mic_test = None;
        app.setup_open = false;
        if let Some(tab) = advanced { app.settings_tab = Some(tab); }
    }
}

