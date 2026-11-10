//! The Settings window: Models, Settings (shortcuts, language, output),
//! Personalization and About. Plus the "press your shortcut" capture overlay.

use crate::app::{DictateApp, SettingsTab};
use crate::engines;
use crate::settings::{Action, Appearance, SpeechEngine, TextEngine, Tone, Trigger};
use crate::theme::{self, b, d, m, para, t};
use crate::views::{VERSION, icon_button, kbd, nav_item};
use eframe::egui::{self, Align, Align2, Color32, CornerRadius, Layout, Margin, Sense, Stroke, vec2};

/// Each tab's title and standfirst. They belong to the sheet, not to the
/// tab's body: inside the scroll area they scrolled away from the close
/// button and left the sheet with no heading.
/// A second control that stays hidden until someone asks for it.
///
/// Settings here kept showing a dropdown and a text field for one value. The
/// field is the escape hatch - a path or a name the list does not carry - so
/// it waits behind a link unless the current value already needs it.
fn disclosed(ui: &mut egui::Ui, key: &str, label: &str, force: bool) -> bool {
    let id = ui.id().with(("disclose", key));
    if force || ui.data(|d| d.get_temp::<bool>(id).unwrap_or(false)) {
        return true;
    }
    if crate::views::link(ui, label).clicked() {
        ui.data_mut(|d| d.insert_temp(id, true));
        return true;
    }
    false
}

fn tab_header(tab: SettingsTab) -> (&'static str, &'static str) {
    match tab {
        SettingsTab::Models => (
            "Models",
            "You choose what turns your voice into text and what tidies it up. Nothing runs anywhere you didn't pick.",
        ),
        SettingsTab::General => ("Settings", "Shortcuts, language and what happens to the text."),
        SettingsTab::Audio => ("Audio input", "Which microphone to listen to, and how loud it comes in."),
        SettingsTab::Personalization => ("Personalization", "How your dictation should read once it is tidied up."),
        SettingsTab::About => ("About", "What this is and where your data lives."),
    }
}

/// Spoken language: what Whisper is told to listen for. It takes a code, so
/// the label people read and the code it gets are kept together here rather
/// than typed in by hand.
const SPOKEN: [(&str, &str); 15] = [
    ("auto", "Detect automatically"),
    ("en", "English"),
    ("es", "Spanish"),
    ("fr", "French"),
    ("de", "German"),
    ("pt", "Portuguese"),
    ("it", "Italian"),
    ("yo", "Yoruba"),
    ("ig", "Igbo"),
    ("ha", "Hausa"),
    ("zh", "Chinese"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("ar", "Arabic"),
    ("hi", "Hindi"),
];

fn spoken_label(code: &str) -> String {
    SPOKEN
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, l)| (*l).to_string())
        .unwrap_or_else(|| code.to_string())
}

fn spoken_picker(ui: &mut egui::Ui, code: &mut String, w: f32, salt: &str) -> bool {
    let mut v = code.clone();
    egui::ComboBox::from_id_salt(salt)
        .width(w)
        .selected_text(spoken_label(&v))
        .show_ui(ui, |ui| {
            for (c, label) in SPOKEN {
                crate::views::combo_value(ui, &mut v, c.to_string(), label);
            }
        });
    if v != *code {
        *code = v;
        return true;
    }
    false
}

pub fn settings_window(app: &mut DictateApp, ctx: &egui::Context) {
    let screen = ctx.content_rect();
    let size = vec2((screen.width() - 80.0).min(940.0), (screen.height() - 80.0).min(660.0));
    let modal = egui::Modal::new(egui::Id::new("settings"))
        .backdrop_color(Color32::from_black_alpha(150))
        .frame(
            theme::ink_panel(),
        );
    let resp = modal.show(ctx, |ui| {
        // Bind the sheet to an exact rect and build inside it. set_max_size
        // alone does not hold: a child that asks for more - an infinitely wide
        // text field, say - widens the whole modal, which is why one tab used
        // to be noticeably broader than the rest.
        let (area, _) = ui.allocate_exact_size(size, Sense::hover());
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(area).layout(Layout::top_down(Align::Min)));
        let ui = &mut ui;
        let tab = app.settings_tab.unwrap_or(SettingsTab::General);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let (nav, _) = ui.allocate_exact_size(vec2(202.0, size.y), Sense::hover());
            ui.painter().rect_filled(nav, CornerRadius { nw: 12, sw: 12, ne: 0, se: 0 }, theme::SURFACE());
            let mut nav_ui = ui.new_child(
                egui::UiBuilder::new().max_rect(nav.shrink2(vec2(12.0, 22.0))).layout(Layout::top_down(Align::Min)),
            );
            for (tab_, icon, label) in [
                (SettingsTab::Models, "mic", "Models"),
                (SettingsTab::General, "gear", "Settings"),
                (SettingsTab::Audio, "mic", "Audio input"),
                (SettingsTab::Personalization, "spark", "Personalization"),
                (SettingsTab::About, "info", "About"),
            ] {
                if nav_item(&mut nav_ui, icon, label, tab == tab_).clicked() {
                    app.settings_tab = Some(tab_);
                    if tab_ == SettingsTab::Models {
                        app.refresh_health(ctx);
                    }
                    if tab_ == SettingsTab::Audio {
                        app.refresh_mics();
                    }
                }
            }
            ui.vertical(|ui| {
                ui.set_width(size.x - 202.0);
                ui.set_height(size.y);
                // Pinned header: the heading stays put while the body scrolls,
                // and the close button keeps its shadow clear of the corner.
                ui.add_space(14.0);
                let (title, standfirst) = tab_header(tab);
                ui.horizontal_top(|ui| {
                    ui.add_space(36.0);
                    ui.vertical(|ui| {
                        ui.set_width(size.x - 202.0 - 36.0 - 76.0);
                        ui.spacing_mut().item_spacing.y = 4.0;
                        ui.label(d(title, 24.0, theme::TEXT()));
                        ui.label(para(standfirst, 13.0, theme::MUTED()));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        ui.add_space(20.0);
                        if icon_button(ui, "close", "Close").clicked() {
                            app.settings_tab = None;
                        }
                    });
                });
                ui.add_space(14.0);
                let (hr, _) = ui.allocate_exact_size(vec2(size.x - 202.0, 1.5), Sense::hover());
                ui.painter().rect_filled(hr, 0.0, theme::LINE());
                egui::ScrollArea::vertical().id_salt(("settings-scroll", tab as u8)).auto_shrink(false).show(ui, |ui| {
                    egui::Frame::new().inner_margin(Margin { left: 36, right: 36, top: 0, bottom: 30 }).show(ui, |ui| {
                        ui.set_width(size.x - 202.0 - 72.0);
                        match tab {
                            SettingsTab::Models => models_tab(app, ui),
                            SettingsTab::General => general_tab(app, ui),
                            SettingsTab::Audio => audio_tab(app, ui),
                            SettingsTab::Personalization => personal_tab(app, ui),
                            SettingsTab::About => about_tab(app, ui),
                        }
                    });
                });
            });
        });
    });
    if resp.should_close() && app.capturing_key.is_none() {
        app.settings_tab = None;
    }
}

/// The kit's .fsec: icon, title in the display face, a hairline under both.
fn section(ui: &mut egui::Ui, icon: &str, title: &str) {
    ui.add_space(22.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 9.0;
        let (ir, _) = ui.allocate_exact_size(vec2(19.0, 19.0), Sense::hover());
        crate::views::kit_icon(ui.painter(), icon, ir.center(), theme::MUTED());
        ui.label(d(title, 15.0, theme::TEXT()));
    });
    ui.add_space(9.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.5), Sense::hover());
    ui.painter().rect_filled(r, 0.0, theme::LINE());
    ui.add_space(4.0);
}

/// A settings row: title and description on the left, the control on the right.
/// The kit's .frow: title, helper text, control on the right, with a hairline
/// above each row after the first.
///
/// No border of its own. Rows sit inside an already outlined sheet, and a box
/// around each one repeats the mistake that made the segmented control look
/// wrong.
fn row(ui: &mut egui::Ui, title: &str, desc: &str, control_w: f32, control: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(13.0);
    ui.horizontal_top(|ui| {
        let left = ui.available_width() - control_w - 18.0;
        ui.vertical(|ui| {
            ui.set_width(left);
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.label(m(title, 13.5, theme::TEXT()));
            if !desc.is_empty() {
                ui.label(para(desc, 12.0, theme::MUTED()));
            }
        });
        ui.add_space(18.0);
        ui.vertical(|ui| {
            ui.set_width(control_w);
            // The sheet zeroes item_spacing.x for its own nav-and-content
            // split, and every row inherits it, so controls sitting side by
            // side ended up touching. Rows get normal spacing back.
            ui.spacing_mut().item_spacing.x = 8.0;
            control(ui);
        });
    });
    ui.add_space(14.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.5), Sense::hover());
    ui.painter().rect_filled(r, 0.0, theme::LINE());
}

fn models_tab(app: &mut DictateApp, ui: &mut egui::Ui) {
    egui::Frame::new()
        .fill(theme::SURFACE())
        .stroke(Stroke::new(1.0, theme::LINE()))
        .corner_radius(10)
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(m("What you need", 14.0, theme::TEXT()));
            crate::views::bullets(
                ui,
                &[
                    "Speech model (required): turns your voice into text. A downloaded Whisper model is all Dictate needs.",
                    "Text model (optional): removes filler words, fixes corrections and formats. Translate and Ask anything need it. Without one, Dictate pastes exactly what you said.",
                ],
                13.0,
                theme::MUTED(),
            );
        });
    let ctx = ui.ctx().clone();
    let health = app.health.lock().unwrap().clone();
    let mut changed = false;

    section(ui, crate::icon::MIC, "Speech to text");
    row(ui, "Engine", "Where your recordings are turned into text.", 300.0, |ui| {
        changed |= crate::views::kit_radio(ui, &mut app.s.speech_engine, SpeechEngine::WhisperLocal, "whisper.cpp on this Mac").changed();
        changed |= crate::views::kit_radio(ui, &mut app.s.speech_engine, SpeechEngine::OpenAiCompatible, "OpenAI-compatible server").changed();
    });
    match app.s.speech_engine {
        SpeechEngine::WhisperLocal => {
            row(ui, "whisper.cpp program", "The whisper-cli command (brew install whisper-cpp).", 300.0, |ui| {
                changed |= ui.add(egui::TextEdit::singleline(&mut app.s.whisper_bin).desired_width(300.0)).changed();
                match &health.whisper_bin {
                    Some(p) => crate::views::status(ui, crate::views::State::Ok, format!("Found at {}", p.display())),
                    None if health.checked => crate::views::status(ui, crate::views::State::Bad, "Not found"),
                    None => {
                        ui.label(t("Checking…", 12.5, theme::MUTED()));
                    }
                };
            });
            let local = engines::local_whisper_models();
            row(ui, "Model", "A ggml model file. Bigger models are more accurate but slower.", 300.0, |ui| {
                let current = engines::speech_model_label(&app.s);
                egui::ComboBox::from_id_salt("wmodel").width(300.0).selected_text(current).show_ui(ui, |ui| {
                    if local.is_empty() {
                        ui.label("No models downloaded yet");
                    }
                    for p in &local {
                        let s = p.to_string_lossy().into_owned();
                        let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
                        changed |= crate::views::combo_value(ui, &mut app.s.whisper_model, s, name).changed();
                    }
                });
                // Only needed when the file lives somewhere the list cannot
                // see, so it is offered rather than always present.
                let custom = !app.s.whisper_model.is_empty()
                    && !local.iter().any(|p| p.to_string_lossy() == app.s.whisper_model);
                if disclosed(ui, "wpath", "Use a file not in this list", custom) {
                    ui.add_space(4.0);
                    changed |= ui.add(egui::TextEdit::singleline(&mut app.s.whisper_model).desired_width(300.0)).changed();
                }
            });
            ui.label(m("Download a model", 14.0, theme::TEXT()));
            ui.add_space(4.0);
            for choice in engines::WHISPER_MODELS {
                let have = local.iter().any(|p| p.file_name().is_some_and(|f| f == choice.file));
                theme::soft(theme::SURFACE(), 12, 2.0, 2).corner_radius(8).inner_margin(Margin::symmetric(12, 8)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(m(format!("{}  ·  {}", choice.label, choice.size), 13.5, theme::TEXT()));
                            ui.label(t(choice.note, 12.5, theme::MUTED()));
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if let Some((f, p)) = &app.download {
                                if f == choice.file {
                                    let p = p.lock().unwrap();
                                    let frac = if p.total > 0 { p.done as f32 / p.total as f32 } else { 0.0 };
                                    crate::views::progress(ui, "Downloading", &format!("{:.0}%", frac * 100.0), frac, 180.0);
                                    return;
                                }
                            }
                            if have {
                                let path = crate::settings::models_dir().join(choice.file).to_string_lossy().into_owned();
                                if app.s.whisper_model == path {
                                    crate::views::status(ui, crate::views::State::Ok, "In use");
                                } else if crate::views::btn(ui, "Use").clicked() {
                                    app.s.whisper_model = path;
                                    changed = true;
                                }
                            } else if ui.add_enabled(app.download.is_none(), egui::Button::new("Download")).clicked() {
                                let p: engines::SharedProgress = Default::default();
                                engines::download_whisper(choice.file, p.clone(), ctx.clone());
                                app.download = Some((choice.file.to_string(), p));
                            }
                        });
                    });
                });
                ui.add_space(4.0);
            }
            ui.add_space(10.0);
        }
        SpeechEngine::OpenAiCompatible => {
            row(ui, "Server address", "Base URL ending in /v1 (OpenAI, Groq, a local faster-whisper server…).", 300.0, |ui| {
                changed |= ui.add(egui::TextEdit::singleline(&mut app.s.speech_url).desired_width(300.0)).changed();
            });
            row(ui, "API key", "Leave empty for servers that don't need one.", 300.0, |ui| {
                changed |= ui.add(egui::TextEdit::singleline(&mut app.s.speech_key).password(true).desired_width(300.0)).changed();
            });
            row(ui, "Model name", "For example whisper-1 or whisper-large-v3-turbo.", 300.0, |ui| {
                changed |= ui.add(egui::TextEdit::singleline(&mut app.s.speech_model).desired_width(300.0)).changed();
            });
        }
    }

    section(ui, crate::icon::WAND, "Text clean-up");
    row(ui, "Engine", "What tidies up, translates and answers.", 300.0, |ui| {
        changed |= crate::views::kit_radio(ui, &mut app.s.text_engine, TextEngine::Ollama, "Ollama on this Mac").changed();
        changed |= crate::views::kit_radio(ui, &mut app.s.text_engine, TextEngine::OpenAiCompatible, "OpenAI-compatible server").changed();
        changed |= crate::views::kit_radio(ui, &mut app.s.text_engine, TextEngine::Off, "Off — paste exactly what I said").changed();
    });
    match app.s.text_engine {
        TextEngine::Ollama => {
            row(ui, "Ollama address", "", 300.0, |ui| {
                changed |= ui.add(egui::TextEdit::singleline(&mut app.s.ollama_url).desired_width(300.0)).changed();
                match &health.ollama {
                    Some(Ok(list)) => {
                        crate::views::status(ui, crate::views::State::Ok, format!("Running · {} model{}", list.len(), if list.len() == 1 { "" } else { "s" }));
                    }
                    Some(Err(_)) => {
                        ui.horizontal(|ui| {
                            crate::views::status(ui, crate::views::State::Bad, "Not running");
                            if crate::views::btn(ui, "Start Ollama").clicked() {
                                match engines::start_ollama() {
                                    Ok(()) => app.notify("Starting Ollama…", false),
                                    Err(e) => app.notify(e, true),
                                }
                                let h = app.health.clone();
                                let c = ctx.clone();
                                let url = app.s.ollama_url.clone();
                                std::thread::spawn(move || {
                                    std::thread::sleep(std::time::Duration::from_secs(3));
                                    h.lock().unwrap().ollama = Some(engines::ollama_models(&url));
                                    c.request_repaint();
                                });
                            }
                        });
                    }
                    None => {
                        ui.label(t("Checking…", 12.5, theme::MUTED()));
                    }
                }
            });
            let list = health.ollama.clone().and_then(|r| r.ok()).unwrap_or_default();
            // One control, not two. This used to be a dropdown of installed
            // models with a text field under it, both writing the same value,
            // so it showed the answer twice and left it unclear which won.
            row(ui, "Model", "Small models (3-8B) are quick enough for dictation.", 300.0, |ui| {
                if list.is_empty() {
                    crate::views::status(ui, crate::views::State::Warn, "Nothing downloaded yet - pick one below");
                } else {
                    egui::ComboBox::from_id_salt("omodel")
                        .width(300.0)
                        .selected_text(app.s.ollama_model.clone())
                        .show_ui(ui, |ui| {
                            for name in &list {
                                changed |= crate::views::combo_value(ui, &mut app.s.ollama_model, name.clone(), name).changed();
                            }
                        });
                }
            });
            // Summarising runs once when a meeting ends, so it can afford a
            // model dictation could not: a 3B finds four actions in five but
            // only two decisions in four, and cannot tell an answered
            // question from an open one.
            row(
                ui,
                "Meeting summaries",
                "Written at the end of a meeting, so a slower, larger model is worth it here.",
                300.0,
                |ui| {
                    let same = "Same as above";
                    let shown = if app.s.meeting_model.trim().is_empty() { same.to_string() } else { app.s.meeting_model.clone() };
                    egui::ComboBox::from_id_salt("mmodel").width(300.0).selected_text(shown).show_ui(ui, |ui| {
                        let mut pick = app.s.meeting_model.clone();
                        changed |= crate::views::combo_value(ui, &mut pick, String::new(), same).changed();
                        for name in &list {
                            changed |= crate::views::combo_value(ui, &mut pick, name.clone(), name).changed();
                        }
                        app.s.meeting_model = pick;
                    });
                },
            );
            // Field and action first, suggestions under them and captioned.
            // The three names used to sit on top looking like buttons that
            // download, when all they do is fill the box.
            row(ui, "Download a model", "Any name from the Ollama library.", 300.0, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut app.pull_name).hint_text("model name").desired_width(186.0));
                    let busy = app.download.is_some();
                    let ready = !busy && !app.pull_name.trim().is_empty();
                    let btn = egui::Button::new(d("DOWNLOAD", 12.0, if ready { theme::PAPER() } else { theme::FAINT() }))
                        .fill(if ready { theme::INK() } else { theme::SURFACE() })
                        .stroke(Stroke::new(2.5, if ready { theme::PAPER() } else { theme::FAINT() }))
                        .corner_radius(11);
                    if ui.add_enabled(ready, btn).clicked() {
                        let p: engines::SharedProgress = Default::default();
                        engines::pull_ollama(&app.s.ollama_url, &app.pull_name, p.clone(), ctx.clone());
                        app.download = Some((format!("ollama:{}", app.pull_name.trim()), p));
                        app.s.ollama_model = app.pull_name.trim().to_string();
                        changed = true;
                    }
                });
                ui.add_space(9.0);
                let spaced: String = "SUGGESTIONS".chars().flat_map(|c| [c, ' ']).collect();
                ui.label(b(spaced.trim_end(), 9.0, theme::FAINT()));
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    for sug in ["llama3.2:3b", "qwen2.5:3b", "gemma3:4b"] {
                        let have = list.iter().any(|m| m == sug || m.trim_end_matches(":latest") == sug);
                        if have {
                            // Already here: ticked, and not offered again.
                            let label = format!("{}  {sug}", crate::icon::CHECK);
                            ui.add_enabled(
                                false,
                                egui::Button::new(t(label, 13.0, theme::FAINT()))
                                    .fill(theme::SURFACE())
                                    .stroke(Stroke::new(2.0, theme::FAINT())),
                            );
                        } else if crate::views::btn(ui, sug).clicked() {
                            app.pull_name = sug.into();
                        }
                    }
                });
                if let Some((f, p)) = &app.download {
                    if f.starts_with("ollama:") {
                        let p = p.lock().unwrap();
                        let frac = if p.total > 0 { p.done as f32 / p.total as f32 } else { 0.0 };
                        crate::views::progress(ui, &p.status, &format!("{:.0}%", frac * 100.0), frac, 300.0);
                    }
                }
            });
        }
        TextEngine::OpenAiCompatible => {
            row(ui, "Server address", "Base URL ending in /v1.", 300.0, |ui| {
                changed |= ui.add(egui::TextEdit::singleline(&mut app.s.text_url).desired_width(300.0)).changed();
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                    for (name, url) in [
                        ("LM Studio", "http://localhost:1234/v1"),
                        ("OpenAI", "https://api.openai.com/v1"),
                        ("OpenRouter", "https://openrouter.ai/api/v1"),
                        ("Groq", "https://api.groq.com/openai/v1"),
                    ] {
                        if crate::views::btn(ui, name).clicked() {
                            app.s.text_url = url.into();
                            changed = true;
                        }
                    }
                });
            });
            row(ui, "API key", "Leave empty for local servers.", 300.0, |ui| {
                changed |= ui.add(egui::TextEdit::singleline(&mut app.s.text_key).password(true).desired_width(300.0)).changed();
            });
            let list = health.remote_text.clone().and_then(|r| r.ok()).unwrap_or_default();
            row(ui, "Model name", "", 300.0, |ui| {
                let listed = list.iter().any(|n| *n == app.s.text_model);
                if list.is_empty() {
                    changed |= ui.add(egui::TextEdit::singleline(&mut app.s.text_model).desired_width(300.0)).changed();
                } else {
                    egui::ComboBox::from_id_salt("rmodel")
                        .width(300.0)
                        .selected_text(app.s.text_model.clone())
                        .show_ui(ui, |ui| {
                            for name in &list {
                                changed |= crate::views::combo_value(ui, &mut app.s.text_model, name.clone(), name).changed();
                            }
                        });
                    if disclosed(ui, "rmodel-name", "Type a name the server did not list", !listed) {
                        ui.add_space(4.0);
                        changed |= ui.add(egui::TextEdit::singleline(&mut app.s.text_model).desired_width(300.0)).changed();
                    }
                }
                ui.add_space(6.0);
                if crate::views::btn(ui, "Fetch model list").clicked() {
                    app.refresh_health(&ctx);
                }
                if let Some(Err(e)) = &health.remote_text {
                    ui.label(t(e, 12.0, theme::RED()));
                }
            });
        }
        TextEngine::Off => {}
    }
    row(ui, "Brave Search key", "Lets Talking Points search the words you select. Leave empty to turn search off.", 300.0, |ui| {
        changed |= ui.add(egui::TextEdit::singleline(&mut app.s.brave_key).password(true).desired_width(300.0)).changed();
    });
    if let Some(url) = crate::talking::text_server(&app.s).map(str::to_owned) {
        let mut probe = app.s.clone();
        probe.talking_points = true;
        let gate = crate::talking::availability(&probe, crate::talking::physical_memory());
        let needs_memory = gate == crate::talking::Availability::NeedsMemory;
        row(ui, "Talking Points", "Suggests what to say and keeps notes while a meeting runs.", 300.0, |ui| {
            ui.add_enabled_ui(!needs_memory || app.s.talking_points, |ui| {
                changed |= crate::views::kit_checkbox(ui, &mut app.s.talking_points).changed();
            });
            if needs_memory {
                ui.label(t("Needs 16 GB of memory to run the text model on this Mac", 12.0, theme::MUTED()));
            }
        });
        if !crate::talking::is_local(&url) {
            let host = crate::talking::host_of(&url);
            let mut on = !matches!(gate, crate::talking::Availability::NeedsCloudOptIn { .. });
            row(
                ui,
                &format!("Send meeting transcripts to {host}"),
                &format!("While Talking Points runs, the meeting transcript is sent to {host} every few seconds."),
                300.0,
                |ui| {
                    if crate::views::kit_checkbox(ui, &mut on).changed() {
                        crate::talking::set_cloud_opt_in(&mut app.s, on);
                        changed = true;
                    }
                },
            );
        }
    }

    section(ui, crate::icon::FLASK, "Try it");
    ui.label(para("Type something messy, the way people talk, and see what your text model makes of it.", 13.5, theme::MUTED()));
    ui.add(egui::TextEdit::multiline(&mut app.test_input).desired_width(ui.available_width()).desired_rows(2));
    let running = app.test_running.load(std::sync::atomic::Ordering::SeqCst);
    let btn = egui::Button::new(d(if running { "RUNNING…" } else { "RUN TEST" }, 12.0, theme::PAPER())).fill(theme::INK()).stroke(Stroke::new(2.5, theme::PAPER())).corner_radius(11);
    if ui.add_enabled(!running, btn).clicked() {
        app.run_test(&ctx);
    }
    if let Some(r) = app.test_output.lock().unwrap().clone() {
        theme::soft(theme::SURFACE(), 8, 2.0, 2).corner_radius(8).inner_margin(Margin::same(10)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            match r {
                Ok(text) => ui.label(t(text, 14.0, theme::TEXT())),
                Err(e) => ui.label(t(e, 13.5, theme::RED())),
            };
        });
    }

    if changed {
        app.changed();
        app.refresh_health(&ctx);
    }
}

fn general_tab(app: &mut DictateApp, ui: &mut egui::Ui) {
    section(ui, crate::icon::PALETTE, "Appearance");
    let mut want = app.s.appearance;
    let choices: Vec<(Appearance, &str)> = Appearance::ALL.iter().map(|a| (*a, a.label())).collect();
    crate::views::pill_tabs(ui, &mut want, &choices);
    if want != app.s.appearance {
        app.s.appearance = want;
        app.changed();
    }
    ui.add_space(14.0);

    section(ui, crate::icon::KEYBOARD, "Shortcuts");
    // One choice of two, which is what the segmented control is for. As a
    // pair of selectable labels the unchosen one looked like plain text and
    // gave no sign it could be clicked.
    let mut hold = app.s.hold_to_talk;
    crate::views::pill_tabs(
        ui,
        &mut hold,
        &[(false, "Press to start and stop"), (true, "Hold while speaking")],
    );
    if hold != app.s.hold_to_talk {
        app.s.hold_to_talk = hold;
        app.changed();
    }
    ui.add_space(14.0);
    let mut keys_changed = false;
    for trigger in Trigger::ALL {
        let desc = match trigger {
            // A meeting runs for an hour; nobody holds a key down for that.
            Trigger::Meeting => {
                "Press to start recording a meeting, press again to stop. Hold to talk does not apply.".to_string()
            }
            Trigger::Act(a) => {
                let mut d = if app.s.hold_to_talk {
                    format!("Hold to keep {}, let go to finish.", a.verb())
                } else {
                    format!("Press to start and stop {}.", a.verb())
                };
                if a == Action::Ask {
                    d.push_str(" Select text first to have it rewritten.");
                }
                d
            }
        };
        row(ui, trigger.label(), &desc, 204.0, |ui| {
            let mut remove = None;
            for (i, k) in app.s.trigger_keys(trigger).iter().enumerate() {
                egui::Frame::new()
                    .stroke(Stroke::new(2.0, theme::INK()))
                    .corner_radius(8)
                    .inner_margin(Margin::symmetric(6, 5))
                    .show(ui, |ui| {
                        ui.set_width(190.0);
                        ui.horizontal(|ui| {
                            kbd(ui, k);
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if icon_button(ui, "close", "Remove").clicked() {
                                    remove = Some(i);
                                }
                            });
                        });
                    });
            }
            if let Some(i) = remove {
                app.s.trigger_keys_mut(trigger).remove(i);
                keys_changed = true;
            }
            let label = if app.capturing_key == Some(trigger) { "Press keys…" } else { "Add another" };
            if crate::views::btn(ui, label).clicked() {
                app.capturing_key = Some(trigger);
            }
        });
    }
    for e in app.hotkey_errors.clone() {
        crate::views::status(ui, crate::views::State::Bad, format!("{e}"));
    }
    if keys_changed {
        app.changed();
        app.register_hotkeys();
    }

    section(ui, crate::icon::GLOBE, "Language");
    let langs = [
        "Spanish", "French", "German", "Portuguese", "Italian", "English", "Yoruba", "Igbo", "Hausa", "Chinese", "Japanese", "Korean", "Arabic", "Hindi",
    ];
    row(ui, "Translation target", "Language the Translate shortcut writes in.", 204.0, |ui| {
        let mut v = app.s.translate_to.clone();
        egui::ComboBox::from_id_salt("tr").width(204.0).selected_text(v.clone()).show_ui(ui, |ui| {
            for l in langs {
                crate::views::combo_value(ui, &mut v, l.to_string(), l);
            }
        });
        if v != app.s.translate_to {
            app.s.translate_to = v;
            app.changed();
        }
    });
    row(ui, "Spoken language", "What to listen for. Detecting it works well for most people.", 204.0, |ui| {
        if spoken_picker(ui, &mut app.s.language, 204.0, "spoken-general") {
            app.changed();
        }
    });

    section(ui, crate::icon::CLIPBOARD, "Output");
    let mut c = false;
    row(ui, "Paste into the app I'm using", "Otherwise the text is only copied.", 204.0, |ui| {
        c |= crate::views::kit_checkbox(ui, &mut app.s.auto_paste).changed();
    });
    row(ui, "Keep my clipboard", "Put back what was on the clipboard after pasting.", 204.0, |ui| {
        c |= crate::views::kit_checkbox(ui, &mut app.s.restore_clipboard).changed();
    });
    row(ui, "Show floating indicator", "A small pill at the bottom of the screen while listening.", 204.0, |ui| {
        c |= crate::views::kit_checkbox(ui, &mut app.s.show_indicator).changed();
    });
    if c {
        app.changed();
    }
    ui.label(para(
        "Pasting needs Accessibility access and recording needs Microphone access (System Settings › Privacy & Security).",
        12.5,
        theme::FAINT(),
    ));
}

fn audio_tab(app: &mut DictateApp, ui: &mut egui::Ui) {
    section(ui, crate::icon::MIC_VOCAL, "Microphone");
    let mut changed = false;
    row(ui, "Microphone", "Which microphone records your voice.", 260.0, |ui| {
        let default_label = match &app.default_mic {
            Some(n) => format!("System default ({n})"),
            None => "System default".to_string(),
        };
        let selected = if app.s.microphone.is_empty() { default_label.clone() } else { app.s.microphone.clone() };
        let mut mic = app.s.microphone.clone();
        egui::ComboBox::from_id_salt("mic").width(260.0).selected_text(selected).show_ui(ui, |ui| {
            crate::views::combo_value(ui, &mut mic, String::new(), default_label);
            for name in &app.mics {
                crate::views::combo_value(ui, &mut mic, name.clone(), name);
            }
        });
        if !mic.is_empty() && !app.mics.contains(&mic) {
            crate::views::status(ui, crate::views::State::Warn, "This microphone isn't connected; the system default is used.");
        }
        if mic != app.s.microphone {
            app.s.microphone = mic;
            changed = true;
        }
        if crate::views::btn(ui, "Refresh list").clicked() {
            app.refresh_mics();
        }
    });
    row(ui, "Input level", "Speak to check the bar moves. Aim for the middle when talking normally.", 260.0, |ui| {
        let level = app.mic_test.as_ref().map(|r| r.level()).unwrap_or(0.0);
        let (r, _) = ui.allocate_exact_size(vec2(260.0, 10.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(r, 5.0, theme::TRACK());
        let frac = (level * 2.0).clamp(0.0, 1.0);
        if frac > 0.0 {
            let mut fill = r;
            fill.set_width(r.width() * frac);
            let c = if frac > 0.95 { theme::RED() } else if frac > 0.08 { theme::GREEN() } else { theme::FAINT() };
            p.rect_filled(fill, 5.0, c);
        }
        ui.add_space(4.0);
        let testing = app.mic_test.is_some();
        let label = if testing { "Stop test" } else { "Test microphone" };
        let btn = egui::Button::new(d(label.to_uppercase(), 12.0, theme::PAPER())).fill(if testing { theme::RED() } else { theme::INK() }).stroke(Stroke::new(2.5, theme::PAPER())).corner_radius(11);
        if ui.add_enabled(app.recording.is_none(), btn).clicked() {
            app.toggle_mic_test();
        }
        if let Some(r) = &app.mic_test {
            if let Some(e) = r.error() {
                crate::views::status(ui, crate::views::State::Bad, format!("The microphone stopped: {e}"));
            } else if r.sample_count() == 0 && r.elapsed() > 1.5 {
                ui.label(para(format!("{} No sound is arriving from this microphone. Try another one or reconnect it.", crate::icon::WARN), 12.5, theme::RED()));
            } else if r.elapsed() > 1.5 && r.peak() == 0.0 {
                // Exact digital silence on a live device is what macOS sends when access is blocked.
                crate::views::status(ui, crate::views::State::Bad, "macOS is blocking the microphone (it only sends silence).");
                if crate::views::btn(ui, "Open microphone privacy settings").clicked() {
                    open_mic_privacy();
                }
            } else {
                ui.label(t(if frac < 0.03 { "Not hearing anything yet…".to_string() } else { format!("Hearing you {}", crate::icon::CHECK) }, 12.5, theme::MUTED()));
            }
            ui.label(t(r.info(), 11.5, theme::FAINT()));
        }
    });
    row(ui, "Input boost", "Turn up if your words come out quiet or are missed.", 260.0, |ui| {
        changed |= crate::views::slider(ui, &mut app.s.input_gain, 0.5..=4.0, 0.25, "\u{00d7}", 260.0).changed();
    });
    if changed {
        app.changed();
        // Restart a running test so it uses the new microphone and boost.
        if app.mic_test.is_some() {
            app.mic_test = None;
            app.toggle_mic_test();
        }
    }
    section(ui, crate::icon::LOCK, "Permission");
    row(
        ui,
        "Microphone access",
        "macOS asks once whether Open Dictate may use the microphone. If you said no, or the bar never moves, switch Open Dictate on in Privacy & Security › Microphone, then quit and reopen it.",
        260.0,
        |ui| {
            if crate::views::btn(ui, "Open microphone privacy settings").clicked() {
                open_mic_privacy();
            }
        },
    );
}

pub fn open_mic_privacy() {
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
        .spawn();
}

fn personal_tab(app: &mut DictateApp, ui: &mut egui::Ui) {
    section(ui, crate::icon::PALETTE, "Tone");
    // Four cards across. Each one costs its inner margin and its border on
    // both sides, and there are three gaps between them; leave that out and
    // the row demands more width than the pane has, which drags the whole
    // modal wider than every other tab.
    // The sheet zeroes item_spacing.x, so the gap is stated here rather than
    // inherited - reading it back gave zero and the cards tiled edge to edge.
    const GAP: f32 = 10.0;
    let n = Tone::ALL.len() as f32;
    let card_chrome = 12.0 * 2.0 + 2.5 * 2.0;
    let w = ((ui.available_width() - GAP * (n - 1.0)) / n - card_chrome).max(56.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        for tone in Tone::ALL {
            let active = app.s.tone == tone;
            let resp = egui::Frame::new()
                .fill(if active { theme::INK() } else { theme::SURFACE() })
                .stroke(Stroke::new(2.5, theme::INK()))
                .corner_radius(15)
                .shadow(egui::epaint::Shadow {
                    offset: if active { [0, 0] } else { [3, 4] },
                    blur: 0,
                    spread: 0,
                    color: theme::SOFT_SHADOW(),
                })
                .inner_margin(Margin { left: 12, right: 12, top: 11, bottom: 13 })
                .show(ui, |ui| {
                    // Frame::show inherits the parent layout, and the parent
                    // here is horizontal: without this the label and blurb sit
                    // side by side and each card demands its full text width,
                    // which ran the row off the edge of the sheet.
                    // Frame::show inherits the parent layout, and the parent
                    // here is horizontal: without this the label and blurb sit
                    // side by side and each card demands its full text width.
                    ui.vertical(|ui| {
                        ui.set_width(w);
                        ui.set_min_height(56.0);
                        let fg = if active { theme::ON_FILL() } else { theme::TEXT() };
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            // A marker, not a second target: the card is the control.
                            let (dr, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
                            let p = ui.painter();
                            p.circle_filled(dr.center(), 9.0, if active { theme::PAPER() } else { theme::SURFACE() });
                            p.circle_stroke(dr.center(), 8.0, Stroke::new(2.0, fg));
                            if active {
                                p.circle_filled(dr.center(), 4.0, theme::INK());
                            }
                            ui.label(m(tone.label(), 14.0, fg));
                        });
                        ui.label(para(tone.blurb(), 12.0, if active { theme::ON_FILL_DIM() } else { theme::MUTED() }));
                    });
                });
            if ui.interact(resp.response.rect, ui.id().with(tone.label()), Sense::click()).clicked() {
                app.s.tone = tone;
                app.changed();
            }
        }
    });
    section(ui, crate::icon::ERASER, "Clean-up");
    let mut c = false;
    row(ui, "Remove filler words", "Drops um, uh, like, you know.", 60.0, |ui| {
        c |= crate::views::kit_checkbox(ui, &mut app.s.remove_fillers).changed();
    });
    row(ui, "Follow my corrections", "\"Tuesday, no wait, Wednesday\" becomes \"Wednesday\".", 60.0, |ui| {
        c |= crate::views::kit_checkbox(ui, &mut app.s.fix_corrections).changed();
    });
    row(ui, "Auto-format", "Turns spoken lists into bullet points and splits paragraphs.", 60.0, |ui| {
        c |= crate::views::kit_checkbox(ui, &mut app.s.auto_format).changed();
    });
    section(ui, crate::icon::PENCIL_LINE, "Your own instructions");
    ui.label(t("Anything else your text model should always do, e.g. \"Use British spelling.\"", 13.5, theme::MUTED()));
    c |= ui.add(egui::TextEdit::multiline(&mut app.s.custom_instructions).desired_width(ui.available_width()).desired_rows(3)).changed();
    section(ui, crate::icon::TIMER, "Time saved");
    row(ui, "Your typing speed", "Used to work out the time saved on Home.", 220.0, |ui| {
        c |= crate::views::slider(ui, &mut app.s.typing_wpm, 15.0..=120.0, 1.0, " WPM", 220.0).changed();
    });
    if c {
        app.changed();
    }
    section(ui, crate::icon::EYE, "What your text model is told");
    theme::soft(theme::SURFACE(), 8, 2.0, 2).corner_radius(8).inner_margin(Margin::same(10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(t(engines::polish_prompt(&app.s, false), 12.5, theme::MUTED()));
    });
}

fn about_tab(app: &mut DictateApp, ui: &mut egui::Ui) {
    ui.label(t(format!("Open Dictate v{VERSION}"), 14.0, theme::TEXT()));
    ui.label(para(
        "Voice dictation that runs on the models you choose. Speak in any app; your words are transcribed, tidied up and pasted where your cursor is.",
        13.5,
        theme::MUTED(),
    ));
    section(ui, crate::icon::FOLDER, "Your data");
    let dir = crate::settings::data_dir();
    ui.label(t(dir.display().to_string(), 13.0, theme::MUTED()));
    if crate::views::btn(ui, "Open folder").clicked() {
        let _ = std::process::Command::new("open").arg(&dir).spawn();
    }
    section(ui, crate::icon::CHART, "Totals");
    ui.label(t(
        format!("{} dictations · {} words · {} in history", app.stats.sessions, app.stats.words, app.history.len()),
        13.5,
        theme::MUTED(),
    ));
}

pub fn capture_shortcut(app: &mut DictateApp, ctx: &egui::Context) {
    let Some(trigger) = app.capturing_key else { return };
    let mut done: Option<Option<String>> = None;
    ctx.input(|i| {
        for e in &i.events {
            if let egui::Event::Key { key, pressed: true, modifiers, .. } = e {
                if *key == egui::Key::Escape {
                    done = Some(None);
                    break;
                }
                let mut parts = Vec::new();
                if modifiers.ctrl {
                    parts.push("Ctrl");
                }
                if modifiers.alt {
                    parts.push("Alt");
                }
                if modifiers.shift {
                    parts.push("Shift");
                }
                if modifiers.mac_cmd {
                    parts.push("Cmd");
                }
                parts.push(key.name());
                done = Some(Some(parts.join("+")));
                break;
            }
        }
    });
    egui::Area::new(egui::Id::new("capture")).order(egui::Order::Tooltip).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
        egui::Frame::new()
            .fill(theme::SURFACE())
            .stroke(Stroke::new(2.5, theme::INK()))
            .stroke(Stroke::new(1.0, theme::ACCENT()))
            .corner_radius(12)
            .inner_margin(Margin::same(20))
            .show(ui, |ui| {
                ui.label(b(format!("Press the shortcut for {}", trigger.label()), 16.0, theme::TEXT()));
                ui.label(para("Function keys (F13–F19) or a combination like ⌥ Space work best. Esc cancels.", 13.0, theme::MUTED()));
            });
    });
    if let Some(r) = done {
        app.capturing_key = None;
        if let Some(k) = r {
            match k.parse::<global_hotkey::hotkey::HotKey>() {
                Ok(_) => {
                    if !app.s.trigger_keys(trigger).contains(&k) {
                        app.s.trigger_keys_mut(trigger).push(k);
                        app.changed();
                        app.register_hotkeys();
                    }
                }
                Err(_) => app.notify(format!("{k} can't be used as a shortcut"), true),
            }
        }
    }
}
