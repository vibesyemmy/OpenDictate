//! Drawing: sidebar, Home insights, History and Dictionary. The Settings window
//! lives in `settings_view`.

use crate::app::{DictateApp, Page, PendingDataAction, SettingsTab};
use crate::settings::{Action, DictWord, KeepHistory, Trigger};
use crate::stats;
use crate::theme::{self, b, d, m, para, t};
use eframe::egui::{self, Align, Align2, Color32, FontId, Layout, Margin, Rect, Sense, Stroke, StrokeKind, Vec2, pos2, vec2};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn draw(app: &mut DictateApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    egui::Panel::left("sidebar")
        .exact_size(216.0)
        .resizable(false)
        .show_separator_line(false)
        .frame(egui::Frame::new().fill(theme::BG_SHEER()))
        .show(ui, |ui| sidebar(app, ui, SidebarVariant::Attached));

    egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::BG_SHEER())).show(ui, |ui| {
        let full = ui.available_width();
        let width = (full - 80.0).clamp(560.0, 1080.0);
        let pad = ((full - width) / 2.0).max(0.0);
        // Home is one piece of reading, so the whole page moves together.
        // The three list pages pin their own headings and scroll only the
        // list underneath, which means they must NOT sit inside a scroll
        // area of their own - nested scrolling gives the heading nothing to
        // stay still against, and it scrolls away with everything else.
        if app.page == Page::Home {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                column(ui, pad, width, |ui| {
                    ui.add_space(36.0);
                    home(app, ui, width);
                    ui.add_space(40.0);
                });
            });
        } else {
            // The player belongs to the whole page; its controls line up with
            // the reading column while the background reaches both edges.
            if app.page == Page::Meetings && app.open_meeting.is_some() && app.meeting.is_none() {
                let player_height = if app.saved_audio.is_empty() { 78.0 } else { 140.0 };
                egui::Panel::bottom("meeting-audio-player").exact_size(player_height)
                    .frame(egui::Frame::new().fill(theme::PAPER()).inner_margin(Margin::symmetric(0, 12)))
                    .show(ui, |ui| column(ui, pad, width, |ui| meeting_player(ui, app)));
            }
            column(ui, pad, width, |ui| {
                ui.add_space(36.0);
                match app.page {
                    Page::History => history(app, ui),
                    Page::Meetings => meetings(app, ui, width),
                    Page::Dictionary => dictionary(app, ui),
                    Page::Home => unreachable!("handled above"),
                }
            });
        }
    });

    if app.settings_tab.is_some() {
        crate::settings_view::settings_window(app, &ctx);
    }
    crate::settings_view::capture_shortcut(app, &ctx);
    confirm_data_action(app, &ctx);
    meeting_intro(app, &ctx);
    crate::setup_guide::show(app, &ctx);
    toast(app, &ctx);
}

fn meeting_intro(app: &mut DictateApp, ctx: &egui::Context) {
    if !app.meeting_intro_open { return; }
    let response = egui::Modal::new(egui::Id::new("meeting-intro"))
        .frame(theme::ink_panel().inner_margin(Margin::same(22)))
        .show(ctx, |ui| crate::meeting_intro::draw(ui, &app.s));
    if let Some(choice) = response.inner {
        app.finish_meeting_intro(choice);
    } else if response.should_close() {
        app.finish_meeting_intro(crate::meeting_intro::Choice::NotNow);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConfirmChoice { Cancel, Confirm }

fn data_confirmation_content(ui: &mut egui::Ui, action: &PendingDataAction, count: usize) -> Option<ConfirmChoice> {
    let (title, body, confirm) = match action {
        PendingDataAction::DeleteMeeting { title, .. } => (
            "Delete meeting?",
            format!("“{title}” and its transcript, notes and saved audio will be removed from Open Dictate. This can’t be undone in the app."),
            "Delete meeting",
        ),
        PendingDataAction::DeleteHistoryEntry { .. } => (
            "Remove history entry?",
            format!("{count} matching {} will be removed. A backup of the text and any saved audio will be placed in macOS Trash. You can restore the most recent backup from History until Trash is emptied.", if count == 1 { "entry" } else { "entries" }),
            "Remove",
        ),
        PendingDataAction::ClearHistory => (
            "Clear all history?",
            format!("This removes {count} {} from History. A backup of the text and any saved audio will be placed in macOS Trash. You can restore the most recent backup from History until Trash is emptied.", if count == 1 { "entry" } else { "entries" }),
            "Clear history",
        ),
        PendingDataAction::ChangeRetention(keep) => (
            "Change history retention?",
            format!("Changing to {} removes {count} existing {} now and applies to future entries. Backups of removed text and saved audio go to macOS Trash. You can restore the most recent backup from History until Trash is emptied.", keep.label(), if count == 1 { "entry" } else { "entries" }),
            "Change retention",
        ),
        PendingDataAction::RestoreHistory => (
            "Restore history backup?",
            "The most recent backup in Trash will be merged with your current history. History retention will be set to Forever so restored entries are kept. The backup will remain in Trash.".into(),
            "Restore history",
        ),
        PendingDataAction::RegenerateSummary { .. } => (
            "Replace your edited summary?",
            "Summarising again replaces your edits with a new model-generated summary. The transcript and saved audio stay unchanged.".into(),
            "Replace summary",
        ),
        PendingDataAction::DiscardRecoveryAudio { count, .. } => (
            "Discard recovery audio?",
            format!("Move {count} untranscribed {} to Trash? Those portions will remain missing from the transcript. The audio stays in Trash until it is emptied.", if *count == 1 { "clip" } else { "clips" }),
            "Move to Trash",
        ),
    };
    ui.set_max_width(440.0);
    ui.label(b(title, 20.0, theme::TEXT()));
    ui.add_space(10.0);
    ui.label(para(body, 14.0, theme::MUTED()));
    ui.add_space(18.0);
    let mut choice = None;
    ui.horizontal(|ui| {
        let cancel = kit_button(ui, "Cancel", Btn::Secondary, 100.0);
        cancel.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "CANCEL"));
        if cancel.clicked() { choice = Some(ConfirmChoice::Cancel); }
        ui.add_space(8.0);
        let accept = kit_button(ui, confirm, Btn::Primary, 120.0);
        accept.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, confirm.to_uppercase()));
        if accept.clicked() { choice = Some(ConfirmChoice::Confirm); }
    });
    choice
}

fn confirm_data_action(app: &mut DictateApp, ctx: &egui::Context) {
    let Some(action) = app.pending_data_action.clone() else { return };
    let count = match &action {
        PendingDataAction::DeleteMeeting { .. } => 1,
        PendingDataAction::DeleteHistoryEntry { when } => app.history.iter().filter(|e| e.when == *when).count(),
        PendingDataAction::ClearHistory => app.history.len(),
        PendingDataAction::ChangeRetention(keep) => crate::data_safety::expired_count(&app.history, *keep, crate::settings::now_secs()),
        PendingDataAction::RestoreHistory => 0,
        PendingDataAction::RegenerateSummary { .. } => 0,
        PendingDataAction::DiscardRecoveryAudio { count, .. } => *count,
    };
    let response = egui::Modal::new(egui::Id::new("confirm-data-action"))
        .frame(theme::ink_panel().inner_margin(Margin::same(22)))
        .show(ctx, |ui| data_confirmation_content(ui, &action, count));
    if response.inner == Some(ConfirmChoice::Cancel) || response.should_close() {
        app.pending_data_action = None;
        return;
    }
    if response.inner != Some(ConfirmChoice::Confirm) { return; }
    app.pending_data_action = None;
    let result = match action {
        PendingDataAction::DeleteMeeting { id, .. } => {
            app.audio_player = None;
            match crate::meeting::delete(&id) {
                Ok(()) => {
                    if app.open_meeting.as_deref() == Some(id.as_str()) {
                        app.open_meeting = None;
                        app.open_segments.clear();
                        app.open_meta = None;
                    }
                    app.meetings_dirty = true;
                    Ok("Meeting deleted.")
                }
                Err(e) => Err(e),
            }
        }
        PendingDataAction::DeleteHistoryEntry { when } => app.delete_entry(when).map(|_| "History removed; backup is in Trash."),
        PendingDataAction::ClearHistory => app.clear_history().map(|_| "History cleared; backup is in Trash."),
        PendingDataAction::ChangeRetention(keep) => app.set_keep_history(keep).map(|_| "History retention updated."),
        PendingDataAction::RestoreHistory => app.restore_history().map(|_| "History restored. Retention is set to Forever."),
        PendingDataAction::RegenerateSummary { id } => { app.start_summary(id); return; }
        PendingDataAction::DiscardRecoveryAudio { id, .. } => {
            let result = crate::meeting::trash_failed_audio(&id);
            app.refresh_meetings();
            if app.open_meeting.as_deref() == Some(id.as_str()) {
                app.open_failed_count = crate::meeting::failed_clip_count(&id);
                app.open_meta = app.meeting_list.iter().find(|m| m.id == id).cloned();
            }
            match result {
                Ok(()) => Ok("Recovery audio moved to Trash."),
                Err(e) => Err(e),
            }
        }
    };
    match result {
        Ok(message) => app.notify(message, false),
        Err(error) => app.notify(error, true),
    }
}

#[cfg(test)]
mod data_confirmation_tests {
    use super::*;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn shortcut_hint_is_compact_next_to_a_button() {
        let h = egui_kittest::Harness::builder().with_size(vec2(500.0, 120.0))
            .with_context_setup(theme::install).build_ui_state(|ui, height| {
                ui.horizontal(|ui| {
                    kit_button_compact_on(ui, "Start dictating", Btn::Primary, 0.0, true);
                    *height = kbd_label(ui, "Option + Space").rect.height();
                });
            }, 0.0_f32);
        let height = *h.state();
        assert!(height <= 24.0, "shortcut hint was {height} points high");
    }

    #[test]
    fn meeting_search_matches_titles_and_subtitles_without_case_sensitivity() {
        assert!(meeting_matches("Project Review", "Payment options", "review"));
        assert!(meeting_matches("Project Review", "Payment options", " PAYMENT "));
        assert!(!meeting_matches("Project Review", "Payment options", "budget"));
        assert!(meeting_matches("Project Review", "", "   "));
    }

    #[test]
    fn meeting_search_sits_to_the_right_of_the_heading() {
        let h = egui_kittest::Harness::builder().with_size(vec2(700.0, 100.0))
            .with_context_setup(theme::install).build_ui_state(|ui, query| {
                meeting_header(ui, query, true);
            }, String::new());
        let heading = h.get_by_label("Meetings").rect();
        let search = h.get_by_label("Search meetings").rect();
        assert!(search.left() > heading.right());
        assert!((search.center().y - heading.center().y).abs() < 20.0);
    }

    #[test]
    fn meeting_tabs_switch_between_summary_and_transcript() {
        let mut h = egui_kittest::Harness::builder().with_size(vec2(400.0, 100.0))
            .with_context_setup(theme::install).build_ui_state(|ui, transcript| {
                pill_tabs(ui, transcript, &[(false, "Summary"), (true, "Transcript")]);
            }, false);
        h.get_by_label("Transcript").click(); h.run();
        assert!(*h.state());
        h.get_by_label("Summary").click(); h.run();
        assert!(!*h.state());
    }

    #[test]
    fn meeting_more_menu_requests_delete() {
        let mut h = egui_kittest::Harness::builder().with_size(vec2(400.0, 300.0))
            .with_context_setup(theme::install).build_ui_state(|ui, requested| {
                if meeting_more_actions(ui, "Planning", false) { *requested = true; }
            }, false);
        h.get_by_label("More actions for Planning").click(); h.run();
        h.get_by_label("Delete").click(); h.run();
        assert!(*h.state());
    }

    #[test]
    fn scrolling_lists_reserve_space_for_the_scrollbar() {
        let mut h = egui_kittest::Harness::builder().with_size(vec2(600.0, 150.0))
            .with_context_setup(theme::install).build_ui_state(|ui, bounds: &mut (f32, f32)| {
                reserve_scrollbar_gutter(ui);
                let outer_right = ui.available_rect_before_wrap().right();
                let area = egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                    for _ in 0..20 { ui.label("Example row"); }
                });
                *bounds = (outer_right, area.inner_rect.right());
            }, (0.0, 0.0));
        h.run(); h.run();
        let (outer, inner) = *h.state();
        assert!(outer - inner >= 15.0, "scrollbar overlaps the content: outer={outer}, inner={inner}");
    }

    #[test]
    fn capture_panel_reports_each_channel_without_claiming_a_speaker() {
        let sample = crate::tap::CaptureSnapshot { frames_ms: 10_000, microphone_ms: 9_900, system_ms: 0, has_mic: true, separated: true };
        let h = egui_kittest::Harness::builder().with_size(vec2(700.0, 200.0))
            .with_context_setup(theme::install)
            .build_ui(|ui| capture_status_detail(ui, &sample, 20.0, 10_100));
        h.get_by_label("Microphone · Sound detected");
        h.get_by_label("System audio · No sound right now");
        h.get_by_label_contains("Speak or play call audio");
        h.get_by_label("Sound levels show activity, not who is speaking.");
    }

    #[test]
    fn retention_dialog_shows_affected_count_and_cancel_keeps_the_choice_unconfirmed() {
        let mut h = egui_kittest::Harness::builder().with_size(vec2(600.0, 300.0))
            .with_context_setup(theme::install)
            .build_ui_state(|ui, choice| {
                if let Some(result) = data_confirmation_content(ui, &PendingDataAction::ChangeRetention(KeepHistory::Days7), 3) {
                    *choice = Some(result);
                }
            }, None::<ConfirmChoice>);
        h.get_by_label("Change history retention?");
        h.get_by_label_contains("removes 3 existing entries now");
        h.get_by_label("CANCEL").click();
        h.run();
        assert!(h.state() == &Some(ConfirmChoice::Cancel));
    }

    #[test]
    fn meeting_dialog_explains_what_is_removed() {
        let h = egui_kittest::Harness::builder().with_size(vec2(600.0, 300.0))
            .with_context_setup(theme::install)
            .build_ui(|ui| {
                data_confirmation_content(ui, &PendingDataAction::DeleteMeeting { id: "id".into(), title: "Project review".into() }, 1);
            });
        h.get_by_label("Delete meeting?");
        h.get_by_label_contains("Project review");
        h.get_by_label_contains("can’t be undone in the app");
        h.get_by_label("DELETE MEETING");
    }

    #[test]
    fn recovery_audio_discard_confirmation_explains_missing_speech_and_trash() {
        let h = egui_kittest::Harness::builder().with_size(vec2(600.0, 300.0))
            .with_context_setup(theme::install)
            .build_ui(|ui| { data_confirmation_content(ui, &PendingDataAction::DiscardRecoveryAudio { id: "meeting".into(), count: 2 }, 2); });
        h.get_by_label("Discard recovery audio?");
        h.get_by_label_contains("portions will remain missing");
        h.get_by_label_contains("stays in Trash");
        h.get_by_label("CANCEL");
    }

    #[test]
    fn confirmation_button_returns_an_explicit_confirm() {
        let mut h = egui_kittest::Harness::builder().with_size(vec2(600.0, 300.0))
            .with_context_setup(theme::install)
            .build_ui_state(|ui, choice| {
                if let Some(result) = data_confirmation_content(ui, &PendingDataAction::ClearHistory, 2) {
                    *choice = Some(result);
                }
            }, None::<ConfirmChoice>);
        h.get_by_label_contains("removes 2 entries from History");
        h.get_by_label("CLEAR HISTORY").click();
        h.run();
        assert!(h.state() == &Some(ConfirmChoice::Confirm));
    }

    #[test]
    fn restore_dialog_explains_retention_change() {
        let h = egui_kittest::Harness::builder().with_size(vec2(600.0, 300.0))
            .with_context_setup(theme::install)
            .build_ui(|ui| { data_confirmation_content(ui, &PendingDataAction::RestoreHistory, 0); });
        h.get_by_label_contains("retention will be set to Forever");
        h.get_by_label("RESTORE HISTORY");
    }
}

/// The centred reading column every page sits in.
///
/// Full height, so a page that scrolls only part of itself has something to
/// measure its scroll area against.
fn column(ui: &mut egui::Ui, pad: f32, width: f32, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal_top(|ui| {
        ui.add_space(pad);
        ui.vertical(|ui| {
            ui.set_width(width);
            add(ui);
        });
    });
}

// ---------------------------------------------------------------------------
// Sidebar

/// The kit's NOTICE, warning and error variants.
///

#[derive(Clone, Copy)]
#[allow(dead_code)]
enum SidebarVariant { Floating, Attached }

fn sidebar(app: &mut DictateApp, ui: &mut egui::Ui, variant: SidebarVariant) {
    let bounds = ui.max_rect();
    let rect = match variant {
        SidebarVariant::Floating => bounds.shrink2(vec2(17.0, 20.0)),
        SidebarVariant::Attached => bounds,
    };
    let p = ui.painter();
    match variant {
        SidebarVariant::Floating => {
            p.rect_filled(rect.translate(vec2(5.0, 6.0)), 18.0, theme::INK());
            p.rect_filled(rect, 18.0, theme::SURFACE());
            p.rect_stroke(rect, 18.0, Stroke::new(3.0, theme::INK()), egui::StrokeKind::Inside);
        }
        SidebarVariant::Attached => {
            p.rect_filled(rect, 0.0, theme::SIDEBAR());
            p.line_segment([rect.right_top(), rect.right_bottom()], Stroke::new(2.0, theme::INK()));
        }
    }

    let inner = rect.shrink2(vec2(10.0, 12.0));
    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)));
    let ui = &mut ui;

    // .sp-head
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        logo(ui);
        ui.add_space(4.0);
        ui.label(d("OPEN DICTATE", 15.0, theme::TEXT()));
    });
    ui.add_space(4.0);

    sp_cap(ui, "PAGES");
    for (page, icon, label) in [
        (Page::Home, "home", "Home"),
        (Page::History, "clock", "History"),
        (Page::Meetings, "mic", "Meetings"),
        (Page::Dictionary, "book", "Dictionary"),
    ] {
        if sp_item(ui, icon, label, app.page == page).clicked() {
            app.page = page;
            if page == Page::Meetings {
                app.meetings_dirty = true;
            }
        }
    }

    // .sp-foot, laid out upwards from the bottom of the panel.
    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
        if sp_item(ui, "gear", "Settings", app.settings_tab.is_some()).clicked() {
            app.settings_tab = Some(SettingsTab::General);
        }
        ui.add_space(8.0);
        let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.5), Sense::hover());
        ui.painter().rect_filled(line, 0.0, theme::LINE());
    });
}

/// Small line icons drawn with the painter (the original uses monochrome outline icons).
pub fn draw_icon(p: &egui::Painter, name: &str, c: egui::Pos2, color: Color32) {
    // Lucide glyphs, keyed by the names the call sites already use, so adding
    // an icon here is a one line change and nothing else moves.
    let glyph = match name {
        "home" => crate::icon::HOUSE,
        "clock" => crate::icon::HISTORY,
        "book" => crate::icon::BOOK,
        "mic" => crate::icon::MIC,
        "gear" => crate::icon::SETTINGS,
        "help" | "info" => crate::icon::HELP,
        "lock" => crate::icon::LOCK,
        "spark" => crate::icon::SPARKLES,
        "close" => crate::icon::X,
        // Anything unknown still shows itself, which is easier to spot than a blank.
        other => other,
    };
    p.text(c, Align2::CENTER_CENTER, glyph, FontId::proportional(17.0), color);
}

/// The kit's .sp-logo: a 30px ink-bordered square in yellow, with its own
/// small hard shadow, holding the three bars mark.
fn logo(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(r.translate(vec2(2.0, 3.0)), 9.0, theme::INK());
    p.rect_filled(r, 9.0, theme::AMBER());
    p.rect_stroke(r, 9.0, Stroke::new(2.5, theme::INK()), egui::StrokeKind::Inside);
    let c = r.center();
    for (i, h) in [8.0, 14.0, 10.0].iter().enumerate() {
        let x = c.x - 5.0 + i as f32 * 5.0;
        p.rect_filled(Rect::from_center_size(pos2(x, c.y), vec2(3.0, *h)), 1.5, theme::ON_YELLOW());
    }
}

/// The kit's own icons, ported from its SVG sprite: a 24 unit grid, 2.3
/// stroke, round caps and joins, drawn at 19px as .sp-item does.
///
/// The sprite holds twelve icons and has neither a clock nor a book, so those
/// two are drawn to the same recipe rather than left in another family.
pub fn kit_icon(p: &egui::Painter, name: &str, c: egui::Pos2, color: Color32) {
    kit_icon_at(p, name, c, color, 19.0);
}

/// The same glyphs at a size of your choosing. The sprite is a 24 grid with a
/// 2.3 stroke, and both scale together, so a small one stays in proportion.
pub fn kit_icon_at(p: &egui::Painter, name: &str, c: egui::Pos2, color: Color32, size: f32) {
    let k = size / 24.0;
    let at = |x: f32, y: f32| pos2(c.x + (x - 12.0) * k, c.y + (y - 12.0) * k);
    let st = Stroke::new(2.3 * k, color);
    let line = |a: (f32, f32), b: (f32, f32)| egui::Shape::line_segment([at(a.0, a.1), at(b.0, b.1)], st);
    let poly = |pts: &[(f32, f32)]| {
        egui::Shape::line(pts.iter().map(|&(x, y)| at(x, y)).collect::<Vec<_>>(), st)
    };
    let ring = |r: f32| egui::Shape::circle_stroke(at(12.0, 12.0), r * k, st);
    match name {
        // i-home
        "home" => {
            p.add(poly(&[(4.0, 11.2), (12.0, 4.5), (20.0, 11.2), (20.0, 20.5), (4.0, 20.5), (4.0, 11.2)]));
            p.add(poly(&[(9.0, 20.5), (9.0, 15.0), (15.0, 15.0), (15.0, 20.5)]));
        }
        // i-gear: a ring with eight spokes
        "gear" => {
            p.add(ring(3.2));
            for i in 0..8 {
                let a = std::f32::consts::TAU * i as f32 / 8.0;
                let (sx, sy) = (12.0 + a.cos() * 5.9, 12.0 + a.sin() * 5.9);
                let (ex, ey) = (12.0 + a.cos() * 8.6, 12.0 + a.sin() * 8.6);
                p.add(line((sx, sy), (ex, ey)));
            }
        }
        // i-bell
        "bell" => {
            p.add(poly(&[(6.0, 16.5), (6.0, 11.0)]));
            p.add(poly(&[(18.0, 11.0), (18.0, 16.5), (19.5, 18.0), (4.5, 18.0), (6.0, 16.5)]));
            p.add(egui::Shape::line(
                (0..=16).map(|i| {
                    let a = std::f32::consts::PI * (1.0 + i as f32 / 16.0);
                    at(12.0 + a.cos() * 6.0, 11.0 + a.sin() * 6.0)
                }).collect::<Vec<_>>(),
                st,
            ));
            p.add(line((10.0, 20.5), (14.0, 20.5)));
        }
        // i-star
        "star" => {
            p.add(poly(&[
                (12.0, 3.8), (14.5, 9.1), (20.2, 9.8), (16.0, 13.7), (17.1, 19.4),
                (12.0, 16.6), (6.9, 19.4), (8.0, 13.7), (3.8, 9.8), (9.5, 9.1), (12.0, 3.8),
            ]));
        }
        // not in the sprite: the kit has no tick, and a star means favourite,
        // not confirmed. Drawn to the same recipe as clock and book.
        "check" => {
            p.add(poly(&[(5.0, 12.4), (10.0, 17.4), (19.2, 6.9)]));
        }
        // not in the sprite either: the kit marks "opens something" with the
        // .inspect button, which has no room here.
        "chev" => {
            p.add(poly(&[(9.5, 5.5), (16.0, 12.0), (9.5, 18.5)]));
        }
        // two sheets, one behind the other
        "copy" => {
            p.add(poly(&[(16.0, 8.0), (16.0, 4.5), (4.5, 4.5), (4.5, 16.0), (8.0, 16.0)]));
            p.add(egui::Shape::rect_stroke(
                egui::Rect::from_min_max(at(8.0, 8.0), at(19.5, 19.5)),
                2.0 * k,
                st,
                egui::StrokeKind::Middle,
            ));
        }
        // not in the sprite: a capsule over its cradle
        "mic" => {
            p.add(egui::Shape::rect_stroke(
                egui::Rect::from_min_max(at(9.2, 3.0), at(14.8, 14.3)),
                2.8 * k,
                st,
                egui::StrokeKind::Middle,
            ));
            p.add(egui::Shape::line(
                (0..=14)
                    .map(|i| {
                        let a = std::f32::consts::PI * i as f32 / 14.0;
                        at(12.0 + a.cos() * 6.2, 11.2 + a.sin() * 6.2)
                    })
                    .collect::<Vec<_>>(),
                st,
            ));
            p.add(line((12.0, 17.4), (12.0, 21.0)));
        }
        // the overflow dots, filled rather than stroked
        "more" => {
            for x in [6.0, 12.0, 18.0] {
                p.add(egui::Shape::circle_filled(at(x, 12.0), 1.7 * k, color));
            }
        }
        // i-lens
        "lens" => {
            p.add(egui::Shape::circle_stroke(at(10.0, 10.0), 6.2 * k, Stroke::new(2.4 * k, color)));
            p.add(line((14.6, 14.6), (20.5, 20.5)));
        }
        "chev-down" => { p.add(poly(&[(6.5, 9.0), (12.0, 14.5), (17.5, 9.0)])); }
        "chev-up" => { p.add(poly(&[(6.5, 14.5), (12.0, 9.0), (17.5, 14.5)])); }
        // i-x
        "x" => {
            p.add(line((7.0, 7.0), (17.0, 17.0)));
            p.add(line((17.0, 7.0), (7.0, 17.0)));
        }
        // not in the sprite, drawn to match it
        "clock" => {
            p.add(ring(7.8));
            p.add(line((12.0, 12.0), (12.0, 7.2)));
            p.add(line((12.0, 12.0), (15.6, 13.6)));
        }
        "book" => {
            p.add(poly(&[(12.0, 7.0), (5.0, 5.2), (3.6, 5.2), (3.6, 17.6), (5.0, 17.6), (12.0, 19.4)]));
            p.add(poly(&[(12.0, 7.0), (19.0, 5.2), (20.4, 5.2), (20.4, 17.6), (19.0, 17.6), (12.0, 19.4)]));
            p.add(line((12.0, 7.0), (12.0, 19.4)));
        }
        other => draw_icon(p, other, c, color),
    }
}

/// The kit's checkbox (.box) and radio (.dot).
///
/// Ticking one presses it onto its own shadow, which is how .inspect behaves
/// when it turns on, so the control looks pushed in rather than merely filled.
fn kit_toggle(ui: &mut egui::Ui, on: bool, round: bool) -> egui::Response {
    let (slot, resp) = ui.allocate_exact_size(vec2(25.0, 25.0), Sense::click());
    let b = Rect::from_min_size(slot.min, vec2(22.0, 22.0));
    let radius = if round { 11.0 } else { 7.0 };
    let p = ui.painter();
    if on {
        let b = b.translate(vec2(2.0, 3.0));
        p.rect_filled(b, radius, theme::INK());
        let c = b.center();
        if round {
            p.circle_filled(c, 4.5, theme::PAPER());
        } else {
            p.add(egui::Shape::line(
                vec![pos2(c.x - 4.6, c.y - 0.2), pos2(c.x - 1.4, c.y + 3.2), pos2(c.x + 4.8, c.y - 3.8)],
                Stroke::new(2.6, theme::PAPER()),
            ));
        }
    } else {
        p.rect_filled(b.translate(vec2(2.0, 3.0)), radius, theme::INK());
        p.rect_filled(b, radius, if resp.hovered() { theme::PAPER() } else { theme::SURFACE() });
        p.rect_stroke(b, radius, Stroke::new(2.5, theme::INK()), egui::StrokeKind::Inside);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn kit_checkbox(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let mut r = kit_toggle(ui, *on, false);
    if r.clicked() {
        *on = !*on;
        r.mark_changed();
    }
    r
}

pub fn kit_radio<T: PartialEq + Copy>(ui: &mut egui::Ui, current: &mut T, value: T, label: &str) -> egui::Response {
    ui.horizontal(|ui| {
        let on = *current == value;
        let mut r = kit_toggle(ui, on, true);
        ui.add_space(3.0);
        let lr = ui.label(m(label, 13.0, theme::TEXT())).interact(Sense::click());
        if lr.clicked() || r.clicked() {
            *current = value;
            r.mark_changed();
        }
        r
    })
    .inner
}

/// A list whose wrapped lines line up under the text, not under the bullet.
///
/// egui has no hanging indent: a bullet typed into the string is just another
/// character, so the second line returns to the margin and the list stops
/// looking like one. The marker gets its own column instead.
pub fn bullets(ui: &mut egui::Ui, items: &[&str], size: f32, color: Color32) {
    for item in items {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let (dot, _) = ui.allocate_exact_size(vec2(15.0, size * 1.5), Sense::hover());
            ui.painter().circle_filled(pos2(dot.left() + 5.0, dot.top() + size * 0.78), 2.2, color);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                ui.label(para(*item, size, color));
            });
        });
        ui.add_space(4.0);
    }
}

/// The kit's .status: a state in a word, tinted by the kit's three colours.
#[derive(Clone, Copy, PartialEq)]
pub enum State {
    Ok,
    Warn,
    Bad,
}

pub fn status(ui: &mut egui::Ui, state: State, text: impl Into<String>) {
    let (tint, glyph) = match state {
        State::Ok => (theme::GREEN(), "check"),
        State::Warn => (theme::WARN(), "bell"),
        State::Bad => (theme::RED(), "x"),
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 7.0;
        let (ir, _) = ui.allocate_exact_size(vec2(15.0, 15.0), Sense::hover());
        kit_icon(ui.painter(), glyph, ir.center(), tint);
        ui.add(egui::Label::new(m(text, 12.0, tint)).wrap());
    });
}

/// The kit's .progress: a caption, a figure, and a bar you can read.
pub fn progress(ui: &mut egui::Ui, caption: &str, figure: &str, frac: f32, w: f32) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(w, 12.0), Sense::hover());
            let p = ui.painter();
            let spaced: String = caption.to_uppercase().chars().flat_map(|c| [c, ' ']).collect();
            p.text(pos2(r.left(), r.center().y), Align2::LEFT_CENTER, spaced.trim_end(), FontId::new(9.0, theme::bold()), theme::FAINT());
            p.text(pos2(r.right(), r.center().y), Align2::RIGHT_CENTER, figure, FontId::new(12.0, theme::display()), theme::TEXT());
        });
        let (r, _) = ui.allocate_exact_size(vec2(w, 11.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(Rect::from_min_size(r.min, vec2(r.width() * frac.clamp(0.0, 1.0), r.height())), 5.0, theme::INK());
        p.rect_stroke(r, 5.0, Stroke::new(2.5, theme::INK()), egui::StrokeKind::Inside);
    });
}

/// The kit's .link: ink, underlined, thicker when pointed at.
pub fn link(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let font = FontId::new(12.5, theme::medium());
    let g = ui.painter().layout_no_wrap(text.to_string(), font.clone(), theme::TEXT());
    let (r, resp) = ui.allocate_exact_size(vec2(g.rect.width(), g.rect.height() + 6.0), Sense::click());
    let p = ui.painter();
    p.text(pos2(r.left(), r.top()), Align2::LEFT_TOP, text, font, theme::TEXT());
    let t = if resp.hovered() { 3.0 } else { 2.0 };
    let y = r.top() + g.rect.height() + 1.0;
    p.line_segment([pos2(r.left(), y), pos2(r.right(), y)], Stroke::new(t, theme::TEXT()));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A row in a dropdown.
///
/// egui takes a selected row's fill from visuals.selection.bg_fill but its
/// text from the global override, so filling it with ink left the label
/// invisible. This paints both, the way everything else chosen in the kit
/// does: ink underneath, paper on top.
pub fn combo_item(ui: &mut egui::Ui, selected: bool, label: &str) -> egui::Response {
    let font = FontId::new(13.0, theme::medium());
    let g = ui.painter().layout_no_wrap(label.to_string(), font.clone(), theme::TEXT());
    let h = (g.rect.height() + 13.0).max(30.0);
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(r, 9.0, theme::INK());
    } else if resp.hovered() {
        p.rect_filled(r, 9.0, theme::PAPER());
    }
    p.text(
        pos2(r.left() + 10.0, r.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        if selected { theme::PAPER() } else { theme::TEXT() },
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// `selectable_value` for a dropdown, drawn by combo_item.
pub fn combo_value<T: PartialEq + Clone>(ui: &mut egui::Ui, current: &mut T, value: T, label: impl AsRef<str>) -> egui::Response {
    let mut r = combo_item(ui, *current == value, label.as_ref());
    if r.clicked() {
        *current = value;
        r.mark_changed();
    }
    r
}

/// The kit's .slider: an ink-edged track with an ink thumb riding it, and
/// the value read out beside it in the display face.
///
/// Painted rather than egui's Slider, whose rail takes colours from the
/// widget visuals and came out invisible against the sheet.
pub fn slider(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    step: f32,
    suffix: &str,
    w: f32,
) -> egui::Response {
    let read_w = 74.0;
    let (slot, mut resp) = ui.allocate_exact_size(vec2(w, 30.0), Sense::click_and_drag());
    let track = Rect::from_min_size(
        pos2(slot.left() + 10.0, slot.center().y - 5.5),
        vec2((slot.width() - read_w - 30.0).max(40.0), 11.0),
    );
    let (lo, hi) = (*range.start(), *range.end());
    if let Some(pos) = resp.interact_pointer_pos() {
        let t = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
        let raw = lo + t * (hi - lo);
        let snapped = if step > 0.0 { (raw / step).round() * step } else { raw };
        if (snapped - *value).abs() > f32::EPSILON {
            *value = snapped;
            resp.mark_changed();
        }
    }
    let t = ((*value - lo) / (hi - lo)).clamp(0.0, 1.0);
    let p = ui.painter();
    // .slider: 11px track, radius 7, 2.5px ink edge. The thumb is 19px at the
    // same radius, filled with ink and bordered in ink - the kit's border is
    // the same colour as the fill, so it reads as one solid rounded square.
    // A paper ring here turned it into an outlined circle instead.
    p.rect_filled(track, 7.0, theme::SURFACE());
    p.rect_stroke(track, 7.0, Stroke::new(2.5, theme::INK()), egui::StrokeKind::Inside);
    let thumb = Rect::from_center_size(pos2(track.left() + t * track.width(), track.center().y), vec2(19.0, 19.0));
    p.rect_filled(thumb.translate(vec2(2.0, 3.0)), 7.0, theme::INK());
    p.rect_filled(thumb, 7.0, theme::INK());
    let shown = if step >= 1.0 {
        format!("{:.0}{suffix}", *value)
    } else {
        format!("{:.2}{suffix}", *value)
    };
    p.text(pos2(slot.right(), slot.center().y), Align2::RIGHT_CENTER, shown, FontId::new(13.0, theme::display()), theme::TEXT());
    resp.on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
}

/// The kit's .btn: its small secondary button, for the actions that hang off
/// a control rather than carry a screen.
///
/// Surface with an ink edge, inverting to ink and paper when pointed at.
/// Painted because the inversion needs the label to change colour with the
/// fill, and egui takes button text from the global override.
pub fn btn(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let font = FontId::new(13.5, theme::bold());
    let g = ui.painter().layout_no_wrap(label.to_string(), font.clone(), theme::INK());
    let (r, resp) = ui.allocate_exact_size(vec2(g.rect.width() + 26.0, g.rect.height() + 15.0), Sense::click());
    let hot = resp.hovered();
    let p = ui.painter();
    p.rect_filled(r, 11.0, if hot { theme::INK() } else { theme::SURFACE() });
    p.rect_stroke(
        r,
        11.0,
        Stroke::new(2.5, if hot { theme::PAPER() } else { theme::INK() }),
        egui::StrokeKind::Inside,
    );
    p.text(r.center(), Align2::CENTER_CENTER, label, font, if hot { theme::PAPER() } else { theme::INK() });
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The kit's .sp-cap: a group heading, tiny and widely tracked.
fn sp_cap(ui: &mut egui::Ui, text: &str) {
    ui.add_space(8.0);
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 13.0), Sense::hover());
    let spaced: String = text.chars().flat_map(|c| [c, ' ']).collect();
    ui.painter().text(
        pos2(r.left() + 10.0, r.center().y),
        Align2::LEFT_CENTER,
        spaced.trim_end(),
        FontId::new(9.0, theme::bold()),
        theme::FAINT(),
    );
    ui.add_space(3.0);
}

/// The kit's .sp-item. The chosen row is filled with ink like the primary
/// button, and carries a yellow shadow rather than an ink one.
pub fn nav_item(ui: &mut egui::Ui, icon: &str, label: &str, active: bool) -> egui::Response {
    sp_item(ui, icon, label, active)
}

fn sp_item(ui: &mut egui::Ui, icon: &str, label: &str, active: bool) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
    let p = ui.painter();
    if active {
        p.rect_filled(r.translate(vec2(3.0, 4.0)), 14.0, theme::AMBER());
        p.rect_filled(r, 14.0, theme::INK());
    } else if resp.hovered() {
        p.rect_filled(r, 14.0, theme::PAPER());
        p.rect_stroke(r, 14.0, Stroke::new(2.5, theme::INK()), egui::StrokeKind::Inside);
    }
    let fg = if active { theme::PAPER() } else { theme::TEXT() };
    kit_icon(p, icon, pos2(r.left() + 20.0, r.center().y), fg);
    p.text(pos2(r.left() + 40.0, r.center().y), Align2::LEFT_CENTER, label, FontId::new(13.0, theme::medium()), fg);
    ui.add_space(4.0);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn status_line(ui: &mut egui::Ui, dot: Color32, what: &str, label: &str) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(8.0, 14.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 3.5, dot);
        ui.label(b(what, 12.5, theme::TEXT()));
        ui.add(egui::Label::new(t(label, 12.5, theme::MUTED())).truncate());
    });
}

#[derive(Clone, Copy, PartialEq)]
pub enum Btn {
    Primary,
    Secondary,
}

/// The kit's .b-primary and .b-secondary.
///
/// The press is the point: hovering shifts the button 3px and shrinks its
/// shadow to 2/3, pressing shifts it the full 5/6 and the shadow disappears,
/// so the button ends up sitting where its shadow was. Without that travel
/// these are just outlined rectangles.
///
/// Rotation is the one part left out - the kit tilts these by about a degree
/// and egui cannot rotate a laid out widget.
pub fn kit_button(ui: &mut egui::Ui, label: &str, kind: Btn, min_w: f32) -> egui::Response {
    kit_button_on(ui, label, kind, min_w, true)
}

/// The same button with the kit's .is-disabled: 0.38 opacity, no shadow,
/// nothing to press. Only the fill and the edge fade - the label is already
/// paper, which is what fading the whole thing onto paper would leave.
pub fn kit_button_on(ui: &mut egui::Ui, label: &str, kind: Btn, min_w: f32, on: bool) -> egui::Response {
    kit_button_sized(ui, label, kind, min_w, on, false, None)
}

/// Compact size of the same Paper & Ink primary and secondary buttons.
pub fn kit_button_compact_on(ui: &mut egui::Ui, label: &str, kind: Btn, min_w: f32, on: bool) -> egui::Response {
    kit_button_sized(ui, label, kind, min_w, on, true, None)
}

/// Compact Paper & Ink action with a non-interactive key hint inside the button.
pub fn kit_button_with_hint(ui: &mut egui::Ui, label: &str, kind: Btn, min_w: f32, on: bool, hint: Option<&str>) -> egui::Response {
    kit_button_sized(ui, label, kind, min_w, on, true, hint)
}

fn kit_button_sized(ui: &mut egui::Ui, label: &str, kind: Btn, min_w: f32, on: bool, compact: bool, hint: Option<&str>) -> egui::Response {
    let size = if compact { 13.0 } else { 15.0 };
    let height = if compact { 36.0 } else { 44.0 };
    let slot_height = height + 6.0;
    let radius = if compact { 11.0 } else { 16.0 };
    let font = FontId::new(size, theme::display());
    let text = label.to_uppercase();
    let pad = if compact { 14.0 } else if kind == Btn::Primary { 32.0 } else { 22.0 };
    let measured = ui.painter().layout_no_wrap(text.clone(), font.clone(), theme::INK()).rect.width();
    let hint_font = FontId::new(10.0, theme::bold());
    let hint_galley = hint.map(|key| ui.painter().layout_no_wrap(key.to_owned(), hint_font, theme::INK()));
    let hint_width = hint_galley.as_ref().map_or(0.0, |g| g.rect.width() + 12.0);
    let hint_gap = if hint_galley.is_some() { 10.0 } else { 0.0 };
    let w = (measured + pad * 2.0 + hint_gap + hint_width).max(min_w);
    let sense = if on { Sense::click() } else { Sense::hover() };
    let (slot, resp) = ui.allocate_exact_size(vec2(w + 5.0, slot_height), sense);
    let base = Rect::from_min_size(slot.min, vec2(w, height));
    let (shift, lift) = if !on {
        (vec2(0.0, 0.0), 0.0)
    } else if resp.is_pointer_button_down_on() {
        (vec2(5.0, 6.0), 0.0)
    } else if resp.hovered() {
        (vec2(3.0, 3.0), 2.0)
    } else {
        (vec2(0.0, 0.0), 5.0)
    };
    let r = base.translate(shift);
    // Primary is ink already; secondary turns into it on hover.
    let inked = kind == Btn::Primary || (on && resp.hovered());
    let (fill, fg, edge) = if inked {
        (theme::INK(), theme::PAPER(), theme::PAPER())
    } else {
        (theme::SURFACE(), theme::INK(), theme::INK())
    };
    let (fill, edge) = if on { (fill, edge) } else { (fill.gamma_multiply(0.38), edge.gamma_multiply(0.38)) };
    let p = ui.painter();
    if lift > 0.0 {
        p.rect_filled(r.translate(vec2(lift, lift + 1.0)), radius, theme::INK());
    }
    p.rect_filled(r, radius, fill);
    p.rect_stroke(r, radius, Stroke::new(3.0, edge), egui::StrokeKind::Inside);
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &text,
        0.0,
        egui::TextFormat { font_id: font, color: fg, extra_letter_spacing: size * 0.045, ..Default::default() },
    );
    let g = p.layout_job(job);
    let content_width = g.rect.width() + hint_gap + hint_width;
    let text_x = r.center().x - content_width / 2.0;
    let text_width = g.rect.width();
    p.galley(pos2(text_x, r.center().y - g.rect.height() / 2.0), g, fg);
    if hint_galley.is_some() {
        let key_rect = Rect::from_center_size(pos2(text_x + text_width + hint_gap + hint_width / 2.0, r.center().y), vec2(hint_width, 22.0));
        p.rect_stroke(key_rect, 5.0, Stroke::new(1.5, fg), StrokeKind::Inside);
        p.text(key_rect.center(), Align2::CENTER_CENTER, hint.unwrap_or_default(), FontId::new(10.0, theme::bold()), fg);
    }
    if on { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

/// The kit's .inspect: a square icon button that presses onto its shadow and
/// fills with ink while it is held.
pub fn icon_button(ui: &mut egui::Ui, icon: &str, tip: &str) -> egui::Response {
    let (slot, resp) = ui.allocate_exact_size(vec2(32.0, 33.0), Sense::click());
    let base = Rect::from_min_size(slot.min, vec2(29.0, 29.0));
    let down = resp.is_pointer_button_down_on();
    let (shift, lift) = if down {
        (vec2(3.0, 4.0), 0.0)
    } else if resp.hovered() {
        (vec2(1.0, 1.0), 2.0)
    } else {
        (vec2(0.0, 0.0), 3.0)
    };
    let r = base.translate(shift);
    let p = ui.painter();
    if lift > 0.0 {
        p.rect_filled(r.translate(vec2(lift, lift + 1.0)), 9.0, theme::INK());
    }
    p.rect_filled(r, 9.0, if down { theme::INK() } else { theme::SURFACE() });
    p.rect_stroke(r, 9.0, Stroke::new(2.5, theme::INK()), egui::StrokeKind::Inside);
    kit_icon(p, icon, r.center(), if down { theme::PAPER() } else { theme::INK() });
    resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand)
}

// ---------------------------------------------------------------------------
// Home

fn home(app: &mut DictateApp, ui: &mut egui::Ui, _width: f32) {
    use crate::home_view::{HomeAction, HomeData};
    if app.meetings_dirty { app.refresh_meetings(); }
    let (speech_ready, _) = app.speech_ready();
    let checked = app.health.lock().map(|h| h.checked).unwrap_or(false);
    let remote_speech = app.s.speech_engine == crate::settings::SpeechEngine::OpenAiCompatible;
    let speech = crate::home_view::speech_state(remote_speech, speech_ready, checked, &app.s.speech_url);
    let shortcut = |keys: &[String]| -> Option<String> {
        let k = keys.iter().find(|k| !k.starts_with('F')).or_else(|| keys.first())?;
        Some(k.split('+').map(|part| match part.trim().to_lowercase().as_str() {
            "alt" | "option" => "Option".to_string(), "cmd" | "meta" | "super" => "Command".to_string(),
            "ctrl" | "control" => "Control".to_string(), "shift" => "Shift".to_string(), _ => part.trim().to_string(),
        }).collect::<Vec<_>>().join(" + "))
    };
    let attention = app.home_attention.as_ref().filter(|_| app.meeting.is_none()).map(|(_, failed_audio)| if *failed_audio {
        "Some speech could not be transcribed. The audio was kept for retry.".to_string()
    } else { "The meeting summary could not be generated. You can review and retry it.".to_string() });
    let data = HomeData { speech, remote_speech, dictation_key: shortcut(&app.s.dictate_keys),
        meeting_key: shortcut(&app.s.meeting_keys), dictating: app.recording.is_some(), busy: app.busy.is_some(),
        meeting_active: app.meeting.is_some(), attention, stats: app.stats.clone(), typing_wpm: app.s.typing_wpm };
    match crate::home_view::draw(ui, &data) {
        Some(HomeAction::Dictate) => if app.recording.is_some() { app.stop_recording_from_home(ui.ctx()); } else { app.start_recording(Action::Dictate); },
        Some(HomeAction::Meeting) => if app.meeting.is_some() { app.page = Page::Meetings; } else { app.toggle_meeting(); },
        Some(HomeAction::Setup) => { app.settings_tab = None; app.setup_open = true; app.refresh_health(ui.ctx()); },
        Some(HomeAction::Models) => { app.settings_tab = Some(SettingsTab::Models); app.refresh_health(ui.ctx()); },
        Some(HomeAction::ReviewMeeting) => if let Some((id, _)) = app.home_attention.clone() { app.page = Page::Meetings; app.open_meeting(id); },
        None => {}
    }
}

pub fn kbd(ui: &mut egui::Ui, k: &str) {
    kbd_label(ui, &pretty_key(k));
}

/// Paper & Ink shortcut keycap for already-formatted, readable shortcut text.
pub fn kbd_label(ui: &mut egui::Ui, label: &str) -> egui::Response {
    let width = ui.fonts_mut(|fonts| fonts.layout_no_wrap(label.to_owned(), egui::FontId::new(10.0, theme::bold()), theme::TEXT()).size().x) + 14.0;
    ui.allocate_ui_with_layout(vec2(width, 22.0), Layout::top_down(Align::Min), |ui| {
        theme::soft(theme::SURFACE(), 14, 2.0, 0)
            .shadow(egui::epaint::Shadow::NONE)
            .corner_radius(5)
            .inner_margin(Margin::symmetric(5, 2))
            .show(ui, |ui| {
                ui.spacing_mut().interact_size.y = 0.0;
                ui.label(b(label, 10.0, theme::TEXT()));
            }).response
    }).inner
}

pub fn pretty_key(k: &str) -> String {
    k.split('+')
        .map(|p| match p.trim().to_lowercase().as_str() {
            "alt" | "option" => "⌥".to_string(),
            "shift" => "⇧".to_string(),
            "ctrl" | "control" => "⌃".to_string(),
            "cmd" | "command" | "super" | "meta" => "⌘".to_string(),
            _ => p.trim().to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// History

/// The kit's segmented control: one track carrying one outline, with the
/// chosen segment filled with ink like the primary button.
///
/// The segments have no border of their own. They used to be egui Buttons,
/// which pick up the global ink stroke, so every segment was outlined inside
/// an already outlined track and the whole thing read as clutter.
pub fn pill_tabs<T: PartialEq + Copy>(ui: &mut egui::Ui, current: &mut T, items: &[(T, &str)]) {
    const H: f32 = 38.0;
    const PAD: f32 = 16.0;
    const GAP: f32 = 4.0;
    let font = FontId::new(12.0, theme::display());
    let widths: Vec<f32> = items
        .iter()
        .map(|(_, l)| ui.painter().layout_no_wrap(l.to_uppercase(), font.clone(), theme::INK()).rect.width() + PAD * 2.0)
        .collect();
    let inner: f32 = widths.iter().sum::<f32>() + GAP * items.len().saturating_sub(1) as f32;
    let (slot, _) = ui.allocate_exact_size(vec2(inner + 8.0 + 3.0, H + 4.0), Sense::hover());
    let track = Rect::from_min_size(slot.min, vec2(inner + 8.0, H));
    let p = ui.painter();
    p.rect_filled(track.translate(vec2(3.0, 4.0)), H / 2.0, theme::INK());
    p.rect_filled(track, H / 2.0, theme::SURFACE());
    p.rect_stroke(track, H / 2.0, Stroke::new(2.5, theme::INK()), egui::StrokeKind::Inside);

    let mut x = track.left() + 4.0;
    for ((v, label), w) in items.iter().zip(widths) {
        let seg = Rect::from_min_size(pos2(x, track.top() + 4.0), vec2(w, H - 8.0));
        let resp = ui.interact(seg, ui.id().with(("segment", *label)), Sense::click());
        let on = *current == *v;
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, on, *label));
        let p = ui.painter();
        if on {
            p.rect_filled(seg, (H - 8.0) / 2.0, theme::INK());
        } else if resp.hovered() {
            p.rect_filled(seg, (H - 8.0) / 2.0, theme::PAPER());
        }
        p.text(
            seg.center(),
            Align2::CENTER_CENTER,
            label.to_uppercase(),
            font.clone(),
            if on { theme::PAPER() } else { theme::TEXT() },
        );
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            *current = *v;
        }
        x += w + GAP;
    }
}

enum RowAction {
    Retry,
    Delete,
    Details,
    SaveEdit,
}

fn reserve_scrollbar_gutter(ui: &mut egui::Ui) {
    ui.style_mut().spacing.scroll.floating_allocated_width = 16.0;
}

fn history(app: &mut DictateApp, ui: &mut egui::Ui) {
    // Keep the floating scroll handle in its own gutter beside the rows.
    reserve_scrollbar_gutter(ui);
    ui.horizontal(|ui| {
        ui.label(b("History", 38.0, theme::TEXT()));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let more = icon_button(ui, "more", "More");
            egui::Popup::menu(&more).show(|ui| {
                if ui.button("Copy all as text").clicked() {
                    let all = app.history.iter().rev().filter(|e| e.error.is_none()).map(|e| e.text.clone()).collect::<Vec<_>>().join("\n\n");
                    ui.ctx().copy_text(all);
                    ui.close();
                }
                if crate::data_safety::latest_backup().is_some() && ui.button("Restore last history backup").clicked() {
                    app.pending_data_action = Some(PendingDataAction::RestoreHistory);
                    ui.close();
                }
                if ui.button(t("Clear all history", 14.0, theme::RED())).clicked() {
                    if !app.history.is_empty() { app.pending_data_action = Some(PendingDataAction::ClearHistory); }
                    ui.close();
                }
            });
            ui.label(t(format!("{} Stored only on this Mac", crate::icon::LOCK), 14.0, theme::MUTED()));
        });
    });
    ui.add_space(10.0);
    theme::soft(theme::PANEL(), 10, 2.0, 2).inner_margin(Margin::symmetric(12, 12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(m(format!("{} Keep history", crate::icon::ARCHIVE), 14.0, theme::TEXT()));
                ui.label(t("Choose how long to keep your history on this device.", 14.0, theme::MUTED()));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let mut k = app.s.keep_history;
                egui::ComboBox::from_id_salt("keep").width(140.0).selected_text(m(k.label(), 14.0, theme::TEXT())).show_ui(ui, |ui| {
                    for o in KeepHistory::ALL {
                        combo_value(ui, &mut k, o, o.label());
                    }
                });
                if k != app.s.keep_history {
                    if crate::data_safety::shortens_history(app.s.keep_history, k) {
                        app.pending_data_action = Some(PendingDataAction::ChangeRetention(k));
                    } else if let Err(e) = app.set_keep_history(k) {
                        app.notify(e, true);
                    }
                }
            });
        });
        ui.add_space(6.0);
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
        ui.painter().rect_filled(r, 0.0, theme::LINE());
        ui.add_space(6.0);
        ui.label(m(format!("{} Your data stays yours", crate::icon::LOCK), 14.0, theme::TEXT()));
        ui.label(para(
            "History is stored on this Mac. Your selected speech and text providers may process audio or text on another server; see Settings › Privacy & data.",
            14.0,
            theme::MUTED(),
        ));
    });
    ui.add_space(14.0);
    if crate::data_safety::latest_backup().is_some() {
        ui.horizontal(|ui| {
            ui.label(t("A history backup is in Trash.", 13.0, theme::MUTED()));
            let restore = btn(ui, "Restore last backup");
            restore.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Restore last backup"));
            if restore.clicked() {
                app.pending_data_action = Some(PendingDataAction::RestoreHistory);
            }
        });
        ui.add_space(14.0);
    }
    pill_tabs(
        ui,
        &mut app.hist_filter,
        &[(None, "All"), (Some(Action::Dictate), "Dictations"), (Some(Action::Translate), "Translations"), (Some(Action::Ask), "Ask anything")],
    );
    ui.add_space(14.0);

    let items: Vec<_> = app.history.iter().rev().filter(|e| app.hist_filter.is_none_or(|f| f == e.action)).cloned().collect();

    let ctx = ui.ctx().clone();
    let mut action: Option<(RowAction, u64)> = None;
    // Everything above stays put; only the entries move. The filter you
    // picked and the retention you set are what you steer the list with, so
    // they are no use scrolled off the top of it.
    egui::ScrollArea::vertical().auto_shrink(false).id_salt("history_list").show(ui, |ui| {
        if items.is_empty() {
            ui.add_space(30.0);
            let key = app.s.dictate_keys.first().map(|k| pretty_key(k)).unwrap_or_else(|| "your shortcut".into());
            ui.vertical_centered(|ui| {
                ui.label(m("Nothing here yet", 18.0, theme::TEXT()));
                ui.label(t(format!("Press {key} anywhere, speak, and your words show up here."), 14.0, theme::MUTED()));
            });
            return;
        }
        let mut i = 0;
        while i < items.len() {
            let day = stats::local_date(items[i].when);
            ui.label(t(stats::day_heading(day), 13.0, theme::MUTED()));
            ui.add_space(2.0);
            theme::soft(theme::SURFACE(), 12, 2.0, 2).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let mut first = true;
                while i < items.len() && stats::local_date(items[i].when) == day {
                    let e = &items[i];
                    if !first {
                        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                        ui.painter().rect_filled(r, 0.0, theme::TRACK());
                    }
                    first = false;
                    if let Some(a) = history_row(app, ui, e) {
                        action = Some((a, e.when));
                    }
                    i += 1;
                }
            });
            ui.add_space(12.0);
        }
        ui.add_space(28.0);
    });
    match action {
        Some((RowAction::Retry, w)) => app.retry(&ctx, w),
        Some((RowAction::Delete, w)) => app.pending_data_action = Some(PendingDataAction::DeleteHistoryEntry { when: w }),
        Some((RowAction::Details, w)) => app.expanded = if app.expanded == Some(w) { None } else { Some(w) },
        Some((RowAction::SaveEdit, w)) => {
            if let Some((_, text)) = app.editing.take() {
                app.save_edit(w, text);
            }
        }
        None => {}
    }
}

fn history_row(app: &mut DictateApp, ui: &mut egui::Ui, e: &crate::settings::Entry) -> Option<RowAction> {
    let mut out = None;
    egui::Frame::new().inner_margin(Margin { left: 12, right: 12, top: 10, bottom: 10 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_top(|ui| {
            ui.add_sized(vec2(58.0, 18.0), egui::Label::new(t(stats::clock(e.when), 12.5, theme::MUTED())));
            let right_w = 150.0;
            let text_w = ui.available_width() - right_w;
            ui.vertical(|ui| {
                ui.set_width(text_w);
                if let Some(err) = &e.error {
                    ui.label(t(format!("{err}  {}", crate::icon::INFO), 14.0, theme::MUTED()))
                        .on_hover_text(format!("{} · {:.1}s recording", e.action.label(), e.seconds));
                } else if let Some((_, text)) = app.editing.as_mut().filter(|(w, _)| *w == e.when) {
                    ui.add(egui::TextEdit::multiline(text).desired_width(text_w).desired_rows(3));
                    ui.horizontal(|ui| {
                        if btn(ui, "Save").clicked() {
                            out = Some(RowAction::SaveEdit);
                        }
                        if btn(ui, "Cancel").clicked() {
                            app.editing = None;
                        }
                    });
                } else {
                    match e.action {
                        Action::Dictate => {}
                        Action::Translate => {
                            ui.label(t("Translated", 11.5, theme::HEAT()[3]));
                        }
                        Action::Ask => {
                            ui.label(t("Ask anything", 11.5, theme::HEAT()[3]));
                        }
                    }
                    ui.label(para(&e.text, 14.0, theme::TEXT()));
                    if app.expanded == Some(e.when) {
                        ui.add_space(4.0);
                        if !e.selection.is_empty() {
                            ui.label(t(format!("Selected text: {}", e.selection), 12.5, theme::MUTED()));
                        }
                        ui.label(t(format!("Heard: {}", e.raw), 12.5, theme::MUTED()));
                        ui.label(t(format!("{:.1}s · speech: {} · text: {}", e.seconds, e.speech_model, e.text_model), 12.0, theme::FAINT()));
                    }
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                let more = icon_button(ui, "more", "More");
                egui::Popup::menu(&more).show(|ui| {
                    if e.error.is_none() {
                        if ui.button("Copy").clicked() {
                            ui.ctx().copy_text(e.text.clone());
                            ui.close();
                        }
                        if ui.button("Copy what you said").clicked() {
                            ui.ctx().copy_text(e.raw.clone());
                            ui.close();
                        }
                        if ui.button("Correct text…").clicked() {
                            app.editing = Some((e.when, e.text.clone()));
                            ui.close();
                        }
                        if ui.button(if app.expanded == Some(e.when) { "Hide details" } else { "Show details" }).clicked() {
                            out = Some(RowAction::Details);
                            ui.close();
                        }
                    }
                    if ui.button(t("Delete", 14.0, theme::RED())).clicked() {
                        out = Some(RowAction::Delete);
                        ui.close();
                    }
                });
                if e.error.is_some() && e.audio.is_some() {
                    // The last button with a fill of its own. It needs a
                    // disabled state, which the kit's button now has.
                    let label = format!("{} Retry", crate::icon::ROTATE);
                    if kit_button_on(ui, &label, Btn::Secondary, 0.0, app.busy.is_none()).clicked() {
                        out = Some(RowAction::Retry);
                    }
                } else if e.error.is_none() && icon_button(ui, "copy", "Copy").clicked() {
                    ui.ctx().copy_text(e.text.clone());
                    app.notify("Copied", false);
                }
            });
        });
    });
    out
}

// ---------------------------------------------------------------------------
// Meetings

pub fn hhmmss(secs: f32) -> String {
    let s = secs.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

fn reviewed_items(ui: &mut egui::Ui, app: &mut DictateApp, section: &str, items: &[String], empty: &str) {
    ui.add_space(14.0);
    ui.label(m(section, 13.0, theme::MUTED()));
    ui.add_space(4.0);
    if items.is_empty() { ui.label(t(empty, 13.0, theme::FAINT())); return; }
    for (index, item) in items.iter().enumerate() {
        ui.push_id((section, index), |ui| {
            let mut source_action = None;
            let row = ui.horizontal_top(|ui| {
                let width = (ui.available_width() - 40.0).max(40.0);
                ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(width);
                    ui.add(egui::Label::new(para(format!("• {item}"), 14.0, theme::TEXT())).wrap());
                });
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    let same = app.review_source.as_ref().is_some_and(|(s, i)| s == section && *i == index);
                    let (_, source) = ui.allocate_exact_size(vec2(30.0, 26.0), Sense::click());
                    source.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{} possible source for {item}", if same { "Hide" } else { "Find" })));
                    source_action = Some((source, same));
                });
            });
            if let Some((source, same)) = source_action {
                let hovered = ui.input(|input| input.pointer.hover_pos())
                    .is_some_and(|pos| row.response.rect.contains(pos) && ui.clip_rect().contains(pos));
                if hovered || source.has_focus() {
                    kit_icon_at(ui.painter(), if same { "chev-up" } else { "chev-down" }, source.rect.center(), theme::TEXT(), 18.0);
                }
                if source.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(if same { "Hide possible source" } else { "Find possible source" }).clicked() {
                    app.review_source = (!same).then(|| (section.to_string(), index));
                }
            }
            if app.review_source.as_ref().is_some_and(|(s, i)| s == section && *i == index) {
                if let Some(i) = crate::meeting::possible_source(item, &app.open_segments) {
                    let seg = app.open_segments[i].clone();
                    ui.label(t(format!("Possible match · {} · Check the context; this may not support the claim.", hhmmss(seg.at_ms as f32 / 1000.0)), 12.0, theme::MUTED()));
                    ui.label(para(&seg.text, 13.0, theme::TEXT()));
                    if let Some(clip) = app.saved_audio.iter().find(|c| c.at_ms == seg.at_ms).cloned() {
                        let label = if app.audio_player.is_some() { "Stop audio" } else { "Play this clip" };
                        if btn(ui, label).clicked() {
                            if app.audio_player.is_some() { app.audio_player = None; }
                            else {
                                match crate::playback::Player::start_at(app.saved_audio.clone(), clip.at_ms) {
                                    Ok(player) => { app.audio_player = Some(player); app.meeting_follow_playback = true; app.meeting_last_playing_segment = None; },
                                    Err(error) => app.notify(error, true),
                                }
                            }
                        }
                    } else {
                        ui.label(t("Audio for this passage was not saved.", 12.0, theme::FAINT()));
                    }
                } else {
                    ui.label(t("No close transcript match found. Check the transcript below.", 12.0, theme::MUTED()));
                }
            }
            ui.add_space(6.0);
        });
    }
}

fn edit_summary_items(ui: &mut egui::Ui, label: &str, items: &mut Vec<String>) {
    ui.add_space(12.0);
    ui.label(m(label, 13.0, theme::MUTED()));
    let mut remove = None;
    for (index, item) in items.iter_mut().enumerate() {
        ui.push_id((label, index), |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::multiline(item).desired_width((ui.available_width() - 100.0).max(150.0)).desired_rows(2));
                if btn(ui, "Remove").clicked() { remove = Some(index); }
            });
        });
    }
    if let Some(index) = remove { items.remove(index); }
    if btn(ui, &format!("Add {label}")).clicked() { items.push(String::new()); }
}

fn capture_status(ui: &mut egui::Ui, live: &crate::meeting::Live) {
    ui.label(m("Audio capture", 13.0, theme::MUTED()));
    let Some(monitor) = &live.capture else {
        status(ui, State::Warn, "Audio capture paused while reconnecting the microphone.");
        return;
    };
    capture_status_detail(ui, &monitor.snapshot(), live.elapsed, crate::tap::clock_ms());
}

fn capture_status_detail(ui: &mut egui::Ui, sample: &crate::tap::CaptureSnapshot, elapsed: f32, now: u64) {
    use crate::meeting::{capture_activity, CaptureActivity};
    let is_separated = sample.separated || !sample.has_mic;
    let mic = capture_activity(sample.frames_ms, sample.microphone_ms, now, sample.has_mic, is_separated);
    let system = capture_activity(sample.frames_ms, sample.system_ms, now, true, is_separated);
    let label = |state| match state {
        CaptureActivity::Starting => "Starting…",
        CaptureActivity::Sound => "Sound detected",
        CaptureActivity::Quiet => "No sound right now",
        CaptureActivity::Stalled => "No audio data arriving",
        CaptureActivity::Unavailable => "Unavailable",
        CaptureActivity::Mixed => "Sources cannot be separated",
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(t(format!("Microphone · {}", label(mic)), 13.0, theme::TEXT()));
        ui.add_space(16.0);
        ui.label(t(format!("System audio · {}", label(system)), 13.0, theme::TEXT()));
    });
    if mic == CaptureActivity::Unavailable {
        ui.label(para("Microphone unavailable. Check the selected input and microphone permission in Settings › Audio.", 12.5, theme::MUTED()));
    } else if mic == CaptureActivity::Stalled || system == CaptureActivity::Stalled || (sample.frames_ms == 0 && elapsed > 5.0) {
        ui.label(para("No audio data is reaching the recorder. Check microphone and system audio permissions in Settings › Audio.", 12.5, theme::MUTED()));
    } else if mic == CaptureActivity::Mixed || system == CaptureActivity::Mixed {
        ui.label(para("The audio sources cannot be separated. Speaker labels may be mixed.", 12.5, theme::MUTED()));
    } else if elapsed > 10.0 && (mic == CaptureActivity::Quiet || system == CaptureActivity::Quiet) {
        ui.label(para("No sound right now can be normal. Speak or play call audio to check each source.", 12.5, theme::MUTED()));
    }
    ui.label(t("Sound levels show activity, not who is speaking.", 12.0, theme::FAINT()));
}

fn meeting_matches(title: &str, subtitle: &str, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty() || title.to_lowercase().contains(&query) || subtitle.to_lowercase().contains(&query)
}

fn play_meeting_from(app: &mut DictateApp, at_ms: u64) {
    if let Some(player) = app.audio_player.as_mut() {
        player.seek(at_ms);
        if let Err(error) = player.resume() { app.notify(error, true); }
    } else {
        match crate::playback::Player::start_at(app.saved_audio.clone(), at_ms) {
            Ok(player) => app.audio_player = Some(player),
            Err(error) => app.notify(error, true),
        }
    }
    app.meeting_follow_playback = true;
    app.meeting_last_playing_segment = None;
}

fn meeting_player(ui: &mut egui::Ui, app: &mut DictateApp) {
    ui.label(b("Meeting audio", 14.0, theme::TEXT()));
    let ranges = match &app.saved_audio_ranges {
        Ok(ranges) => ranges.clone(),
        Err(error) => { ui.label(para(error, 12.0, theme::RED())); return; }
    };
    let Some(&(first, _)) = ranges.first() else {
        ui.label(t("No audio was saved for this meeting.", 12.5, theme::MUTED()));
        return;
    };
    let last = ranges.iter().map(|range| range.1).max().unwrap_or(first).max(first + 1);
    let mut start = None;
    let mut seek = None;
    let mut error = None;
    ui.horizontal(|ui| {
        let is_playing = app.audio_player.as_ref().is_some_and(|p| p.playing());
        let label = if is_playing { "Pause" } else { "Play" };
        if kit_button_compact_on(ui, label, Btn::Secondary, 75.0, true).clicked() {
            if let Some(player) = app.audio_player.as_mut() {
                if is_playing { player.pause(); } else if let Err(e) = player.resume() { error = Some(e); }
            } else { start = Some(first); }
        }
        let position = app.audio_player.as_ref().map_or(first, |p| p.position_ms()).clamp(first, last);
        ui.label(t(hhmmss(position as f32 / 1000.0), 12.0, theme::MUTED()));
        let mut value = position as f64 / 1000.0;
        let slider_width = (ui.available_width() - 65.0).max(50.0);
        let response = ui.scope(|ui| {
            ui.spacing_mut().slider_width = slider_width;
            ui.add(egui::Slider::new(&mut value, first as f64 / 1000.0..=last as f64 / 1000.0).show_value(false))
        }).inner;
        response.widget_info(|| egui::WidgetInfo::slider(true, value, "Meeting audio position"));
        if response.changed() { seek = Some((value * 1000.0).round() as u64); }
        ui.label(t(hhmmss(last as f32 / 1000.0), 12.0, theme::MUTED()));
    });
    if let Some(e) = error { app.notify(e, true); }
    if let Some(at) = start { play_meeting_from(app, at); }
    if let Some(at) = seek {
        if let Some(player) = app.audio_player.as_mut() { player.seek(at); }
        else { play_meeting_from(app, at); }
        app.meeting_follow_playback = true;
        app.meeting_last_playing_segment = None;
    }
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width().max(1.0), 4.0), Sense::hover());
    ui.painter().rect_filled(bar, 2.0, theme::TRACK());
    for &(start, end) in &ranges {
        let scale = bar.width() / (last - first) as f32;
        let piece = Rect::from_min_max(pos2(bar.left() + (start - first) as f32 * scale, bar.top()), pos2(bar.left() + (end - first) as f32 * scale, bar.bottom()));
        ui.painter().rect_filled(piece, 2.0, theme::ACCENT());
    }
    let note = if app.audio_player.as_ref().is_some_and(|p| !p.ready()) { "Preparing audio…".into() }
        else if app.audio_player.as_ref().is_some_and(|p| p.playing() && p.position_ms() < last && !crate::playback::time_has_audio(&ranges, p.position_ms())) { "No saved audio at this point. Playback will continue to the next clip.".into() }
        else { format!("Saved clips · {}–{} of meeting · Gaps may contain missing audio.", hhmmss(first as f32 / 1000.0), hhmmss(last as f32 / 1000.0)) };
    ui.label(t(note, 12.0, theme::MUTED()));
}

fn meeting_more_actions(ui: &mut egui::Ui, title: &str, working: bool) -> bool {
    let more = icon_button(ui, "more", "More actions");
    more.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("More actions for {title}")));
    let mut delete = false;
    egui::Popup::menu(&more).show(|ui| {
        if ui.add_enabled(!working, egui::Button::new(t("Delete", 14.0, theme::RED()))).clicked() {
            delete = true;
            ui.close();
        }
    });
    delete
}

fn meeting_header(ui: &mut egui::Ui, query: &mut String, show_search: bool) {
    ui.horizontal(|ui| {
        ui.label(b("Meetings", 38.0, theme::TEXT()));
        if show_search {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let width = ui.available_width().min(300.0);
                let previous = query.clone();
                let response = ui.add(egui::TextEdit::singleline(query).hint_text("Search meetings").desired_width(width));
                response.widget_info(|| {
                    let mut info = egui::WidgetInfo::text_edit(true, &previous, &*query, "Search meetings");
                    info.label = Some("Search meetings".to_string());
                    info
                });
            });
        }
    });
}

fn meetings(app: &mut DictateApp, ui: &mut egui::Ui, _w: f32) {
    reserve_scrollbar_gutter(ui);
    let detail = app.open_meeting.is_some() && app.meeting.is_none();
    if !detail {
        meeting_header(ui, &mut app.meeting_search, app.meeting.is_none());
        ui.add_space(16.0);
    }

    // Keep the meeting context and section switcher visible while the
    // summary or transcript scrolls underneath.
    if detail {
        if let (Some(id), Some(meta)) = (app.open_meeting.clone(), app.open_meta.clone()) {
            let mut delete = false;
            ui.horizontal(|ui| {
                let back = link(ui, "←  All meetings");
                back.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Back to all meetings"));
                if back.clicked() {
                    app.audio_player = None;
                    app.open_meeting = None;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let working = app.retrying_meeting.as_deref() == Some(id.as_str()) || app.title_retrying.as_deref() == Some(id.as_str()) || app.summarising.as_deref() == Some(id.as_str());
                    let more = icon_button(ui, "more", "Meeting actions");
                    more.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Meeting actions"));
                    egui::Popup::menu(&more).show(|ui| {
                        if ui.add_enabled(!working && app.editing_title.is_none(), egui::Button::new("Rename")).clicked() { app.editing_title = Some(meta.title.clone()); ui.close(); }
                        if ui.add_enabled(!working, egui::Button::new(t("Delete", 14.0, theme::RED()))).clicked() { delete = true; ui.close(); }
                    });
                });
            });
            ui.add_space(12.0);
            if let Some(mut title) = app.editing_title.take() {
                let mut save = false;
                let mut cancel = false;
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut title).desired_width(340.0));
                    save = btn(ui, "Save title").clicked();
                    cancel = btn(ui, "Cancel").clicked();
                });
                if save {
                    match crate::meeting::rename(&id, &title) {
                        Ok(()) => { app.refresh_meetings(); app.open_meta = app.meeting_list.iter().find(|m| m.id == id).cloned(); app.notify("Title saved", false); }
                        Err(error) => { app.editing_title = Some(title); app.notify(error, true); }
                    }
                } else if !cancel { app.editing_title = Some(title); }
            } else {
                ui.add(egui::Label::new(b(&meta.title, 28.0, theme::TEXT())).wrap());
            }
            if let Some(error) = &meta.title_error {
                ui.label(para(format!("Title not generated: {error}"), 12.5, theme::MUTED()));
                if app.title_retrying.as_deref() == Some(id.as_str()) { ui.label(t("Retrying title…", 12.5, theme::MUTED())); }
                else if app.open_summary.is_some() && app.editing_title.is_none() && btn(ui, "Retry title").clicked() { app.retry_meeting_title(id.clone()); }
            }
            if !meta.subtitle.is_empty() {
                ui.add_space(4.0);
                ui.add(egui::Label::new(para(&meta.subtitle, 14.0, theme::MUTED())).wrap());
            }
            ui.add_space(5.0);
            let length = meta.ended.map(|e| hhmmss((e - meta.started) as f32)).unwrap_or_else(|| "unfinished".into());
            ui.label(t(format!("{} · {} · {length}", crate::stats::stamp(meta.started), meta.source), 13.0, theme::MUTED()));
            ui.add_space(16.0);

            if delete {
                app.pending_data_action = Some(PendingDataAction::DeleteMeeting { id: id.clone(), title: meta.title.clone() });
            }
            pill_tabs(ui, &mut app.meeting_show_transcript, &[(false, "Summary"), (true, "Transcript")]);
            ui.add_space(14.0);
        }
        if app.open_meeting.is_none() { return; }
    }

    // The heading stays; everything under it - a live transcript, an opened
    // meeting, or the list - is what moves. A transcript runs to hundreds of
    // lines, and scrolling it used to take the page's name with it.
    // Each tab retains its own place instead of opening halfway down when
    // switching from a long summary to a long transcript (or vice versa).
    let scroll_id = ("meetings_body", app.open_meeting.clone(), detail && app.meeting_show_transcript);
    egui::ScrollArea::vertical().auto_shrink(false).id_salt(scroll_id).show(ui, |ui| {
        // The scroll area is narrower when its gutter is visible. All cards,
        // transcript lines and list rows must fit that actual content width.
        let w = ui.available_width();
        // While one is running, it is the only thing worth looking at.
        if let Some(session) = &app.meeting {
            let (elapsed, segments, warning) = session
                .live
                .lock()
                .map(|l| (l.elapsed, l.segments.clone(), l.warning.clone()))
                .unwrap_or((0.0, Vec::new(), None));
            let mut stop = false;
            let mut audio_settings = false;
            theme::ink_card().inner_margin(Margin::same(18)).show(ui, |ui| {
                ui.set_width(w - 36.0);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(12.0, 18.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 5.0, theme::RED());
                    ui.label(b(format!("Recording · {}", hhmmss(elapsed)), 19.0, theme::TEXT()));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        stop = btn(ui, "Stop").clicked();
                    });
                });
                ui.label(t(&session.source, 13.0, theme::MUTED()));
                ui.add_space(10.0);
                if let Ok(live) = session.live.lock() { capture_status(ui, &live); }
                audio_settings = link(ui, "Audio settings").clicked();
                if let Some(warn) = &warning {
                    ui.add_space(6.0);
                    status(ui, State::Warn, warn);
                }
            });
            ui.add_space(14.0);
            if segments.is_empty() {
                ui.label(para(
                    "The transcript appears here about half a minute behind the room.",
                    14.0,
                    theme::FAINT(),
                ));
            }
            for seg in segments.iter().rev() {
                transcript_line(app, ui, seg, w, false, false, false, false, &mut None);
            }
            if stop {
                app.toggle_meeting();
            }
            if audio_settings { app.settings_tab = Some(SettingsTab::Audio); }
            return;
        }

        // One stored meeting, opened.
        if let Some(id) = app.open_meeting.clone() {
            let Some(meta) = app.open_meta.clone() else {
                app.open_meeting = None;
                return;
            };
            if let Some(error) = &meta.summary_error {
                theme::ink_card().inner_margin(Margin::same(16)).show(ui, |ui| {
                    ui.set_width(w - 32.0);
                    ui.label(b("Summary could not be generated", 16.0, theme::TEXT()));
                    ui.label(para(error, 13.0, theme::MUTED()));
                    if app.open_segments.is_empty() {
                        ui.label(t(if app.open_failed_count > 0 { "Recover the speech below before retrying the summary." } else { "There is no transcript available to summarise." }, 12.5, theme::MUTED()));
                    } else {
                        ui.label(t("Your transcript is still saved. Check the text model in Models, then try again.", 12.5, theme::MUTED()));
                    }
                    if !app.open_segments.is_empty() && app.summarising.as_deref() != Some(id.as_str()) && btn(ui, "Retry summary").clicked() {
                        if app.open_summary.as_ref().is_some_and(|s| s.user_edited) { app.pending_data_action = Some(PendingDataAction::RegenerateSummary { id: id.clone() }); }
                        else { app.start_summary(id.clone()); }
                    }
                });
                ui.add_space(14.0);
            }

            if app.open_failed_count > 0 {
                theme::ink_card().inner_margin(Margin::same(16)).show(ui, |ui| {
                    ui.set_width(w - 32.0);
                    ui.label(b("Some speech needs transcription", 16.0, theme::TEXT()));
                    let noun = if app.open_failed_count == 1 { "speech clip could" } else { "speech clips could" };
                    ui.label(para(format!("{} {noun} not be transcribed. The audio stays on this Mac until recovery succeeds or you move it to Trash.", app.open_failed_count), 13.0, theme::MUTED()));
                    ui.label(para("Retry uses your current speech provider. Review the recovered transcript and update any summary.", 12.5, theme::MUTED()));
                    let retrying = app.retrying_meeting.as_deref() == Some(id.as_str());
                    if retrying { ui.label(t("Recovering speech…", 13.0, theme::MUTED())); }
                    else {
                        ui.horizontal(|ui| {
                            if btn(ui, "Retry transcription").clicked() { app.retry_meeting_audio(id.clone()); }
                            ui.add_enabled_ui(app.summarising.as_deref() != Some(id.as_str()) && app.title_retrying.as_deref() != Some(id.as_str()), |ui| {
                                if btn(ui, "Discard audio…").clicked() { app.pending_data_action = Some(PendingDataAction::DiscardRecoveryAudio { id: id.clone(), count: app.open_failed_count }); }
                            });
                        });
                    }
                });
                ui.add_space(14.0);
            }

            if meta.discarded_failed_clips > 0 {
                ui.label(para(format!("{} untranscribed {} moved to Trash. The transcript may be incomplete.", meta.discarded_failed_clips, if meta.discarded_failed_clips == 1 { "clip was" } else { "clips were" }), 12.5, theme::MUTED()));
                ui.add_space(10.0);
            }

            if !app.meeting_show_transcript {
            // What it came to, before what was said.
            if app.summarising.as_deref() == Some(id.as_str()) {
                let step = app.summary_progress.lock().map(|p| p.clone()).unwrap_or_default();
                theme::ink_card().inner_margin(Margin::same(16)).show(ui, |ui| {
                    ui.set_width(w - 32.0);
                    status(ui, State::Warn, if step.is_empty() { "Summarising…".into() } else { step });
                });
                ui.add_space(14.0);
            } else if let Some(sum) = app.open_summary.clone() {
                theme::ink_card().inner_margin(Margin::same(18)).show(ui, |ui| {
                    ui.set_width((w - 36.0).min(700.0));
                    ui.horizontal(|ui| {
                        ui.label(b(if app.editing_summary.is_some() { "Edit summary" } else { "Summary" }, 19.0, theme::TEXT()));
                        if app.editing_summary.is_none() {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let edit = btn(ui, "Edit");
                                edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Edit summary"));
                                if edit.clicked() { app.editing_summary = Some(sum.clone()); }
                            });
                        }
                    });
                    ui.label(t("Generated suggestions can be wrong. Check the transcript before relying on them.", 12.5, theme::MUTED()));
                    if let Some(mut draft) = app.editing_summary.take() {
                        ui.add_space(8.0);
                        ui.label(m("Overview", 13.0, theme::MUTED()));
                        ui.add(egui::TextEdit::multiline(&mut draft.overview).desired_width(ui.available_width()).desired_rows(4));
                        edit_summary_items(ui, "Decisions", &mut draft.decisions);
                        edit_summary_items(ui, "Actions", &mut draft.actions);
                        edit_summary_items(ui, "Questions", &mut draft.questions);
                        edit_summary_items(ui, "Key points", &mut draft.points);
                        ui.add_space(12.0);
                        let mut save = false;
                        let mut cancel = false;
                        ui.horizontal(|ui| { save = btn(ui, "Save edits").clicked(); cancel = btn(ui, "Cancel").clicked(); });
                        if save {
                            for items in [&mut draft.decisions, &mut draft.actions, &mut draft.questions, &mut draft.points] {
                                items.retain(|s| !s.trim().is_empty());
                            }
                            draft.user_edited = true;
                            match crate::meeting::save_reviewed_summary(&id, &draft) {
                                Ok(()) => { app.open_summary = Some(draft); app.review_source = None; app.notify("Summary saved", false); }
                                Err(error) => { app.editing_summary = Some(draft); app.notify(error, true); }
                            }
                        } else if !cancel { app.editing_summary = Some(draft); }
                    } else {
                        ui.add_space(6.0);
                        ui.label(para(&sum.overview, 14.0, theme::TEXT()));
                        ui.label(t("Review the overview against the transcript.", 12.0, theme::FAINT()));
                            for (title, items, empty) in [
                                ("Decided", &sum.decisions, "Nothing was settled."),
                                ("To do", &sum.actions, "Nobody took anything on."),
                                ("Still open", &sum.questions, "Nothing was left hanging."),
                            ] { if !items.is_empty() { reviewed_items(ui, app, title, items, empty); } }
                        if !sum.points.is_empty() { reviewed_items(ui, app, "Key points", &sum.points, ""); }
                        ui.add_space(12.0);
                        ui.label(t(if sum.user_edited { format!("Generated by {} · edited", sum.model) } else { format!("Generated by {}", sum.model) }, 12.0, theme::FAINT()));
                    }
                });
                ui.add_space(8.0);
                // A summary written from text that has since been corrected
                // is quietly wrong, and the person who corrected it is the
                // one who would never know.
                ui.horizontal(|ui| {
                    if app.editing_summary.is_none() && btn(ui, "Summarise again").clicked() {
                        if sum.user_edited {
                            app.pending_data_action = Some(PendingDataAction::RegenerateSummary { id: id.clone() });
                        } else { app.start_summary(id.clone()); }
                    }
                    if crate::meeting::summary_is_stale(&id) {
                        status(ui, State::Warn, "The transcript was corrected after this was written");
                    }
                });
                ui.add_space(14.0);
            } else if !app.open_segments.is_empty() {
                ui.label(b("No summary yet", 17.0, theme::TEXT()));
                ui.label(para("Generate a summary from the transcript.", 13.0, theme::MUTED()));
                ui.add_space(8.0);
                if btn(ui, "Summarise").clicked() {
                    app.start_summary(id.clone());
                }
                ui.add_space(14.0);
            } else {
                ui.label(para("No summary is available for this meeting.", 14.0, theme::MUTED()));
            }

            }

            if app.meeting_show_transcript {
            if !app.saved_audio.is_empty() {
                ui.label(t("Select a timestamp to play its saved audio. Use Edit to correct the transcript.", 12.0, theme::MUTED()));
                ui.add_space(6.0);
            }
            let manually_scrolling = ui.ctx().input(|input| {
                let clip = ui.clip_rect();
                let wheel = input.smooth_scroll_delta.y != 0.0 && input.pointer.hover_pos().is_some_and(|pos| clip.contains(pos));
                let drag_scrollbar = input.pointer.primary_down() && input.pointer.hover_pos().is_some_and(|pos| pos.x >= clip.right() - 4.0 && pos.x <= clip.right() + 20.0 && (clip.top()..=clip.bottom()).contains(&pos.y));
                let key = app.editing_line.is_none() && [egui::Key::PageUp, egui::Key::PageDown, egui::Key::Home, egui::Key::End].into_iter().any(|key| input.key_pressed(key));
                wheel || drag_scrollbar || key
            });
            if manually_scrolling { app.meeting_follow_playback = false; }
            if !app.meeting_follow_playback && app.audio_player.as_ref().is_some_and(|player| player.playing()) {
                if link(ui, "Follow playback").clicked() { app.meeting_follow_playback = true; app.meeting_last_playing_segment = None; }
                ui.add_space(8.0);
            }
            let segs = app.open_segments.clone();
            let active = app.audio_player.as_ref().filter(|p| p.ready()).and_then(|player| crate::playback::active_segment(&segs, &player.ranges, player.position_ms()));
            let follow = app.meeting_follow_playback && active != app.meeting_last_playing_segment && app.editing_line.is_none();
            app.meeting_last_playing_segment = active;
            if segs.is_empty() {
                let why = if app.open_failed_count > 0 {
                    "The transcript is incomplete because speech could not be transcribed. Retry the saved audio above.".to_string()
                } else if meta.discarded_failed_clips > 0 {
                    "Some speech could not be transcribed, and its recovery audio was moved to Trash.".to_string()
                } else if meta.peak < 0.001 {
                    "Nothing reached the recorder at all. The meeting may have been on a device Open Dictate could not tap.".to_string()
                } else {
                    format!(
                        "Nothing was loud enough to transcribe. The loudest moment was {:.3} out of 1.0, which is about the level of a quiet room.",
                        meta.peak
                    )
                };
                ui.label(para(why, 14.0, theme::MUTED()));
                ui.add_space(6.0);
                ui.label(t("Levels for every chunk are in levels.log beside this transcript.", 12.5, theme::FAINT()));
            }
            let mut correction = None;
            let mut seek = None;
            for (index, seg) in segs.iter().enumerate() {
                let can_edit = app.retrying_meeting.as_deref() != Some(id.as_str());
                let has_audio = app.saved_audio.iter().any(|clip| clip.at_ms == seg.at_ms);
                if transcript_line(app, ui, seg, w, can_edit, active == Some(index), has_audio, follow && active == Some(index), &mut seek) {
                    correction = app.editing_line.take();
                }
            }
            if let Some(at) = seek { play_meeting_from(app, at); }
            if let Some((at, text)) = correction {
                let before = app.open_segments.iter().find(|segment| segment.at_ms == at).map(|segment| segment.text.clone()).unwrap_or_default();
                match crate::meeting::edit_segment(&id, at, text.trim()) {
                    Ok(()) => {
                        app.open_segments = crate::meeting::transcript(&id);
                        let learned = app.learn_transcript_correction(&before, text.trim());
                        app.notify(if learned > 0 { format!("Line corrected · learned {learned} {} in Dictionary", if learned == 1 { "term" } else { "terms" }) } else { "Line corrected".into() }, false);
                    }
                    Err(e) => app.notify(e, true),
                }
            }
            }
            return;
        }

        // The list, read when it changed rather than every frame.
        if app.meetings_dirty {
            app.refresh_meetings();
        }
        let all = app.meeting_list.clone();
        if all.is_empty() {
            theme::ink_card().inner_margin(Margin::same(18)).show(ui, |ui| {
                ui.set_width(w - 36.0);
                ui.label(b("No meetings yet", 19.0, theme::TEXT()));
                ui.add_space(4.0);
                ui.label(para("Start a meeting from Home. Your meetings and transcripts will appear here.", 14.0, theme::MUTED()));
                if link(ui, "Go to Home").clicked() { app.page = Page::Home; }
            });
            return;
        }
        let filtered: Vec<_> = all.into_iter().filter(|m| meeting_matches(&m.title, &m.subtitle, &app.meeting_search)).collect();
        if filtered.is_empty() {
            ui.label(para("No meetings match your search.", 14.0, theme::MUTED()));
        }
        for m in filtered {
            let working = app.retrying_meeting.as_deref() == Some(m.id.as_str()) || app.title_retrying.as_deref() == Some(m.id.as_str()) || app.summarising.as_deref() == Some(m.id.as_str());
            let mut delete = false;
            let row = ui.push_id(&m.id, |ui| ui.allocate_ui_with_layout(vec2(w, 76.0), Layout::top_down(Align::Min), |ui| {
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(vec2((w - 165.0).max(100.0), 25.0), Layout::left_to_right(Align::Center), |ui| {
                        ui.add(egui::Label::new(b(&m.title, 16.0, theme::TEXT())).truncate());
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        delete = meeting_more_actions(ui, &m.title, working);
                        let length = m.ended.map(|e| hhmmss((e - m.started) as f32)).unwrap_or_else(|| "Unfinished".into());
                        ui.label(t(length, 13.0, theme::MUTED()));
                    });
                });
                if !m.subtitle.is_empty() { ui.add(egui::Label::new(t(&m.subtitle, 13.0, theme::MUTED())).truncate()); }
                ui.label(t(crate::stats::stamp(m.started), 12.5, theme::FAINT()));
            })).inner;
            let mut open_rect = row.response.rect;
            open_rect.max.x -= 45.0;
            let click = ui.interact(open_rect, ui.id().with(&m.id), Sense::click());
            click.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Open meeting: {}", m.title)));
            if click.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                app.open_meeting(m.id.clone());
            }
            if delete { app.pending_data_action = Some(PendingDataAction::DeleteMeeting { id: m.id, title: m.title }); }
            ui.separator();
            ui.add_space(8.0);
        }
        ui.add_space(28.0);
    });
}

/// One line of a transcript, correctable when the meeting is a stored one.
///
/// Returns true when Save was pressed, leaving the edited text in
/// `app.editing_line` for the caller to write: this is inside a loop over a
/// clone of the transcript, and the write reloads it.
///
/// A live meeting is not editable. Its lines are still arriving, and the
/// file is being appended to underneath them.
fn transcript_line(
    app: &mut DictateApp,
    ui: &mut egui::Ui,
    seg: &crate::meeting::Segment,
    w: f32,
    editable: bool,
    highlighted: bool,
    can_seek: bool,
    follow: bool,
    seek: &mut Option<u64>,
) -> bool {
    let mut saved = false;
    let mut edit_action = None;
    let row = egui::Frame::new().fill(if highlighted { theme::HOVER() } else { Color32::TRANSPARENT }).corner_radius(6).inner_margin(Margin::same(6)).show(ui, |ui| { ui.horizontal_top(|ui| {
        ui.add_space(2.0);
        let (r, stamp_response) = ui.allocate_exact_size(vec2(60.0, 20.0), if can_seek { Sense::click() } else { Sense::hover() });
        if can_seek {
            stamp_response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Play from {}", hhmmss(seg.at_ms as f32 / 1000.0))));
            if stamp_response.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Play from this passage").clicked() { *seek = Some(seg.at_ms); }
        }
        let p = ui.painter();
        p.text(
            pos2(r.left(), r.top()),
            Align2::LEFT_TOP,
            hhmmss(seg.at_ms as f32 / 1000.0),
            FontId::new(12.0, theme::medium()),
            if can_seek { theme::ACCENT() } else { theme::FAINT() },
        );
        // Who was talking, from which side of the call was louder.
        let (who, colour) = match seg.voice.as_str() {
            "you" => ("You", theme::GREEN()),
            "them" => ("Other voices", theme::ACCENT()),
            "both" => ("Both", theme::FAINT()),
            "mixed" => ("Mixed", theme::FAINT()),
            _ => ("", theme::FAINT()),
        };
        ui.allocate_ui_with_layout(vec2(90.0, 20.0), Layout::left_to_right(Align::Center), |ui| {
            ui.set_min_width(90.0);
            if !who.is_empty() { ui.label(t(who, 11.5, colour)); }
        });
        let editing = app.editing_line.as_ref().is_some_and(|(at, _)| *at == seg.at_ms);
        if editing {
            ui.vertical(|ui| {
                let width = ui.available_width();
                if let Some((_, text)) = app.editing_line.as_mut() {
                    let r = ui.add(egui::TextEdit::multiline(text).desired_width(width).desired_rows(2));
                    // Enter saves, since a transcript line is one line and
                    // nobody wants to reach for a button to fix a name.
                    // Shift-Enter still breaks the line.
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift) {
                        saved = true;
                    }
                }
                ui.horizontal(|ui| {
                    if btn(ui, "Save").clicked() {
                        saved = true;
                    }
                    if btn(ui, "Cancel").clicked() {
                        app.editing_line = None;
                    }
                    ui.label(t("Enter saves · Esc cancels", 12.0, theme::FAINT()));
                });
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    app.editing_line = None;
                }
            });
        } else {
            ui.vertical(|ui| {
                ui.set_width((ui.available_width() - if editable { 55.0 } else { 0.0 }).max(80.0));
                ui.add(egui::Label::new(para(&seg.text, 14.0, theme::TEXT())).wrap());
            });
            if editable {
                // Keep a stable, focusable target even while the label is
                // hidden, so keyboard and assistive-technology users can edit.
                let (_, edit) = ui.allocate_exact_size(vec2(36.0, 24.0), Sense::click());
                edit.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Edit transcript passage at {}", hhmmss(seg.at_ms as f32 / 1000.0))));
                edit_action = Some(edit);
            }
        }
    }); });
    if let Some(edit) = edit_action {
        let hovered_row = ui.input(|input| input.pointer.hover_pos())
            .is_some_and(|pos| row.response.rect.contains(pos) && ui.clip_rect().contains(pos));
        if hovered_row || edit.has_focus() {
            let font = FontId::new(12.5, theme::medium());
            let g = ui.painter().layout_no_wrap("Edit".to_owned(), font.clone(), theme::TEXT());
            let origin = edit.rect.left_top();
            ui.painter().text(origin, Align2::LEFT_TOP, "Edit", font, theme::TEXT());
            let y = origin.y + g.rect.height() + 1.0;
            ui.painter().line_segment([pos2(origin.x, y), pos2(origin.x + g.rect.width(), y)], Stroke::new(if edit.hovered() || edit.has_focus() { 3.0 } else { 2.0 }, theme::TEXT()));
        }
        if edit.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Correct this transcript passage").clicked() {
            app.editing_line = Some((seg.at_ms, seg.text.clone()));
        }
    }
    if follow { row.response.scroll_to_me(None); }
    ui.add_space(10.0);
    let _ = w;
    saved
}

// ---------------------------------------------------------------------------
// Dictionary

fn dictionary(app: &mut DictateApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(b("Dictionary", 38.0, theme::TEXT()));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let btn = egui::Button::new(d("NEW WORD", 12.0, theme::PAPER()))
                .fill(theme::INK())
                .stroke(Stroke::new(2.5, theme::PAPER()))
                .corner_radius(11)
                .min_size(vec2(0.0, 32.0));
            if ui.add(btn).clicked() {
                app.new_word = Some(String::new());
            }
        });
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let (auto_tab, manual_tab) = (
            format!("{}  Auto-added", crate::icon::SPARKLES),
            format!("{}  Manually-added", crate::icon::PENCIL),
        );
        pill_tabs(ui, &mut app.dict_filter, &[(0, "All"), (1, &auto_tab), (2, &manual_tab)]);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if icon_button(ui, "lens", "Search words").clicked() {
                app.dict_search = if app.dict_search.is_some() { None } else { Some(String::new()) };
            }
            if let Some(q) = &mut app.dict_search {
                ui.add(egui::TextEdit::singleline(q).hint_text("Search words").desired_width(220.0));
            }
        });
    });
    ui.add_space(10.0);

    if let Some(w) = &mut app.new_word {
        let mut add = false;
        let mut cancel = false;
        theme::soft(theme::PANEL(), 10, 2.0, 2).inner_margin(Margin::same(12)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(t("Add a name, term or spelling you want written exactly like this.", 13.5, theme::MUTED()));
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(w).hint_text("e.g. Kubernetes, Adaeze, feat/idle-timeout").desired_width(360.0));
                if !r.has_focus() && !r.lost_focus() && w.is_empty() {
                    r.request_focus();
                }
                add = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                add |= ui.add(egui::Button::new(d("ADD", 12.0, theme::PAPER())).fill(theme::INK()).stroke(Stroke::new(2.5, theme::PAPER())).corner_radius(11)).clicked();
                cancel = btn(ui, "Cancel").clicked();
            });
        });
        ui.add_space(10.0);
        if add {
            let word = w.trim().to_string();
            if !word.is_empty() && !app.s.dictionary.iter().any(|d| d.word == word) {
                app.s.dictionary.push(DictWord { word, auto: false });
                app.changed();
            }
            app.new_word = None;
        } else if cancel {
            app.new_word = None;
        }
    }

    let q = app.dict_search.clone().unwrap_or_default().to_lowercase();
    let words: Vec<(usize, DictWord)> = app
        .s
        .dictionary
        .iter()
        .cloned()
        .enumerate()
        .rev()
        .filter(|(_, d)| match app.dict_filter {
            1 => d.auto,
            2 => !d.auto,
            _ => true,
        })
        .filter(|(_, d)| q.is_empty() || d.word.to_lowercase().contains(&q))
        .collect();
    let mut remove = None;
    // The heading, the filter, the search box and the add form all stay put:
    // they are how you narrow the list, and searching a long dictionary used
    // to push the very box you were typing in off the top of the page.
    egui::ScrollArea::vertical().auto_shrink(false).id_salt("dictionary_words").show(ui, |ui| {
        if words.is_empty() {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                ui.label(m("No words yet", 18.0, theme::TEXT()));
                ui.label(para("Add names and terms here, or correct a dictation in History and new words are learned automatically.", 14.0, theme::MUTED()));
            });
            return;
        }
        let cols = 3;
        let gap = 8.0;
        let w = (ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32;
        for row in words.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (idx, d) in row {
                    let (r, resp) = ui.allocate_exact_size(vec2(w, 38.0), Sense::hover());
                    let p = ui.painter();
                    p.rect_filled(r, 6.0, theme::SURFACE());
                    p.rect_stroke(r, 6.0, Stroke::new(1.0, theme::LINE()), StrokeKind::Inside);
                    let (icon, c) = if d.auto { (crate::icon::SPARKLES, theme::GREEN()) } else { (crate::icon::PENCIL, theme::FAINT()) };
                    p.text(pos2(r.left() + 18.0, r.center().y), Align2::CENTER_CENTER, icon, FontId::proportional(11.0), c);
                    p.text(pos2(r.left() + 32.0, r.center().y), Align2::LEFT_CENTER, &d.word, FontId::new(14.0, theme::medium()), theme::TEXT());
                    if resp.hovered() {
                        let x = Rect::from_center_size(pos2(r.right() - 18.0, r.center().y), Vec2::splat(20.0));
                        let xr = ui.interact(x, ui.id().with(("del", *idx)), Sense::click());
                        draw_icon(ui.painter(), "close", x.center(), if xr.hovered() { theme::TEXT() } else { theme::MUTED() });
                        if xr.clicked() {
                            remove = Some(*idx);
                        }
                    }
                }
            });
            ui.add_space(4.0);
        }
        ui.add_space(28.0);
    });
    if let Some(i) = remove {
        app.s.dictionary.remove(i);
        app.changed();
    }
}

fn toast(app: &mut DictateApp, ctx: &egui::Context) {
    let Some(toast) = &app.toast else { return };
    egui::Area::new(egui::Id::new("toast")).order(egui::Order::Tooltip).anchor(Align2::CENTER_BOTTOM, vec2(108.0, -28.0)).show(ctx, |ui| {
        egui::Frame::new()
            .fill(if toast.error { theme::RED() } else { theme::SURFACE() })
            .stroke(Stroke::new(2.5, theme::INK()))
            .corner_radius(10)
            .inner_margin(Margin::symmetric(16, 10))
            .show(ui, |ui| {
                ui.set_max_width(520.0);
                ui.label(t(&toast.text, 14.0, theme::TEXT()));
            });
    });
    ctx.request_repaint_after(std::time::Duration::from_millis(500));
}
