#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable;
    use eframe::egui::vec2;

    #[test]
    fn first_use_explains_setup_and_does_not_show_example_statistics() {
        let data = HomeData::example(false);
        let h = egui_kittest::Harness::builder().with_size(vec2(900.0, 1000.0))
            .with_context_setup(crate::theme::install).build_ui(|ui| { draw(ui, &data); });
        h.get_by_label("Set up dictation to get started");
        h.get_by_label("Your insights will appear here");
        assert!(h.query_by_label("12,500").is_none());
        let setup = h.get_by_label("Open setup guide").rect();
        let heading = h.get_by_label("Set up dictation to get started").rect();
        assert!(setup.left() > heading.right(), "setup action should be at the far right");
        assert!((setup.center().y - heading.center().y).abs() < 50.0, "setup action should align with its message, not stretch the notice");
    }

    #[test]
    fn cards_have_equal_width_and_expose_actions() {
        assert_eq!(card_widths(800.0), (392.0, 392.0));
        let data = HomeData::example(true);
        let mut h = egui_kittest::Harness::builder().with_size(vec2(900.0, 1000.0))
            .with_context_setup(crate::theme::install).build_ui_state(|ui, action| {
                if let Some(a) = draw(ui, &data) { *action = Some(a); }
            }, None::<HomeAction>);
        h.get_by_label("Start meeting").click(); h.run();
        assert_eq!(*h.state(), Some(HomeAction::Meeting));
    }

    #[test]
    fn action_cards_fit_above_insights_at_a_narrow_width() {
        let data = HomeData::example(true);
        let h = egui_kittest::Harness::builder().with_size(vec2(620.0, 1000.0))
            .with_context_setup(crate::theme::install).build_ui(|ui| { draw(ui, &data); });
        let insights = h.get_by_label("Your insights").rect();
        for label in ["Start dictating", "Start meeting"] {
            let action = h.get_by_label(label).rect();
            assert!(action.bottom() < insights.top(), "{label} overlaps insights");
        }
    }

    #[test]
    fn an_active_meeting_is_never_stopped_by_the_home_card() {
        let mut data = HomeData::example(true); data.meeting_active = true;
        let mut h = egui_kittest::Harness::builder().with_size(vec2(900.0, 1000.0))
            .with_context_setup(crate::theme::install).build_ui_state(|ui, action| {
                if let Some(a) = draw(ui, &data) { *action = Some(a); }
            }, None::<HomeAction>);
        h.get_by_label("View meeting").click(); h.run();
        assert_eq!(*h.state(), Some(HomeAction::Meeting));
    }

    #[test]
    fn stopping_from_home_promises_copy_and_returns_the_dictation_action() {
        let mut data = HomeData::example(true); data.dictating = true;
        let mut h = egui_kittest::Harness::builder().with_size(vec2(900.0, 1000.0))
            .with_context_setup(crate::theme::install).build_ui_state(|ui, action| {
                if let Some(a) = draw(ui, &data) { *action = Some(a); }
            }, None::<HomeAction>);
        h.get_by_label_contains("Stop here to copy");
        h.get_by_label("Stop & copy").click(); h.run();
        assert_eq!(*h.state(), Some(HomeAction::Dictate));
    }

    #[test]
    fn recovery_notice_stays_compact_and_opens_the_meeting() {
        let mut data = HomeData::example(true);
        data.attention = Some("Some speech needs retry.".into());
        let mut h = egui_kittest::Harness::builder().with_size(vec2(900.0, 1000.0))
            .with_context_setup(crate::theme::install).build_ui_state(|ui, action| {
                if let Some(a) = draw(ui, &data) { *action = Some(a); }
            }, None::<HomeAction>);
        let message = h.get_by_label("A meeting needs attention").rect();
        let review = h.get_by_label("Review meeting").rect();
        assert!((message.center().y - review.center().y).abs() < 50.0);
        h.get_by_label("Review meeting").click(); h.run();
        assert_eq!(*h.state(), Some(HomeAction::ReviewMeeting));
    }

    #[test]
    fn remote_speech_is_not_shown_as_configured_without_a_server() {
        assert_eq!(speech_state(true, true, true, ""), SpeechState::NeedsSetup);
        assert_eq!(speech_state(true, true, true, "http://localhost:1234"), SpeechState::Configured);
        assert_eq!(speech_state(false, false, false, ""), SpeechState::Checking);
    }

    #[test]
    fn speech_status_is_specific_and_setup_status_opens_models() {
        let data = HomeData::example(true);
        let h = egui_kittest::Harness::builder().with_size(vec2(900.0, 1000.0))
            .with_context_setup(crate::theme::install).build_ui(|ui| { draw(ui, &data); });
        h.get_by_label("Speech model ready");
        assert!(h.query_by_label("Speech · Ready").is_none());
        let data = HomeData::example(false);
        let mut h = egui_kittest::Harness::builder().with_size(vec2(900.0, 1000.0))
            .with_context_setup(crate::theme::install).build_ui_state(|ui, action| {
                if let Some(a) = draw(ui, &data) { *action = Some(a); }
            }, None::<HomeAction>);
        h.get_by_label("Speech setup needed").click(); h.run();
        assert_eq!(*h.state(), Some(HomeAction::Models));
    }
}

use crate::{stats::Stats, theme::{self, b, d, para, t}, views};
use chrono::{Datelike, Duration, Local};
use eframe::egui::{self, Align, Align2, FontId, Layout, Margin, Rect, Sense, Stroke, pos2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpeechState { Checking, Ready, Configured, NeedsSetup }

pub fn speech_state(remote: bool, ready: bool, checked: bool, address: &str) -> SpeechState {
    if remote {
        if ready && !address.trim().is_empty() { SpeechState::Configured } else { SpeechState::NeedsSetup }
    } else if ready { SpeechState::Ready }
    else if !checked { SpeechState::Checking }
    else { SpeechState::NeedsSetup }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HomeAction { Dictate, Meeting, Setup, Models, ReviewMeeting }

#[derive(Clone)]
pub struct HomeData {
    pub speech: SpeechState,
    pub remote_speech: bool,
    pub dictation_key: Option<String>,
    pub meeting_key: Option<String>,
    pub dictating: bool,
    pub busy: bool,
    pub meeting_active: bool,
    pub attention: Option<String>,
    pub stats: Stats,
    pub typing_wpm: f32,
}

impl HomeData {
    #[cfg(test)]
    fn example(ready: bool) -> Self {
        Self { speech: if ready { SpeechState::Ready } else { SpeechState::NeedsSetup }, remote_speech: false,
            dictation_key: Some("Option + Space".into()), meeting_key: Some("F16".into()), dictating: false,
            busy: false, meeting_active: false, attention: None, stats: Stats::default(), typing_wpm: 40.0 }
    }
}

fn card_widths(width: f32) -> (f32, f32) {
    let each = ((width - 16.0) / 2.0).max(0.0);
    (each, each)
}

fn button(ui: &mut egui::Ui, label: &str, primary: bool, enabled: bool, hint: Option<&str>) -> egui::Response {
    views::kit_button_with_hint(ui, label, if primary { views::Btn::Primary } else { views::Btn::Secondary }, 120.0, enabled, hint)
}

fn setup_notice(ui: &mut egui::Ui, remote_speech: bool) -> bool {
    let mut clicked = false;
    theme::soft(theme::Y1(), 9, 1.5, 0).stroke(Stroke::new(1.5, theme::WARN())).inner_margin(Margin::same(13)).show(ui, |ui| {
        let available = ui.available_width();
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.vertical(|ui| {
                ui.set_width((available - 180.0).max(160.0));
                ui.label(b("Set up dictation to get started", 14.0, theme::ON_YELLOW()));
                ui.label(para(if remote_speech { "Configure a speech server and model, and check your microphone." } else { "Choose a speech model and check your microphone." }, 12.5, theme::ON_YELLOW_DIM()));
            });
            ui.allocate_ui_with_layout(vec2(180.0, 48.0), Layout::right_to_left(Align::Center), |ui| {
                let response = views::kit_button_compact_on(ui, "Open setup guide", views::Btn::Secondary, 0.0, true);
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Open setup guide"));
                clicked = response.clicked();
            });
        });
    });
    clicked
}

pub fn draw(ui: &mut egui::Ui, data: &HomeData) -> Option<HomeAction> {
    let width = ui.available_width();
    let mut action = None;
    ui.horizontal(|ui| {
        ui.label(d("Home", 32.0, theme::TEXT()));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (label, dot) = match data.speech {
                SpeechState::Checking => ("Checking speech model…", theme::MUTED()),
                SpeechState::Ready => ("Speech model ready", theme::GREEN()),
                SpeechState::Configured => ("Speech server configured", theme::GREEN()),
                SpeechState::NeedsSetup => ("Speech setup needed", theme::WARN()),
            };
            if data.speech == SpeechState::NeedsSetup {
                let r = ui.add(egui::Label::new(t(label, 12.5, theme::MUTED()).underline()).sense(Sense::click()));
                r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, label));
                if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() { action = Some(HomeAction::Models); }
            } else {
                ui.label(t(label, 12.5, theme::MUTED()));
            }
            let (r, _) = ui.allocate_exact_size(vec2(8.0, 14.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 3.5, dot);
        });
    });
    ui.add_space(18.0);
    if data.speech == SpeechState::NeedsSetup {
        if setup_notice(ui, data.remote_speech) { action = Some(HomeAction::Setup); }
        ui.add_space(14.0);
    }
    let (first, second) = card_widths(width);
    let can_start = matches!(data.speech, SpeechState::Ready | SpeechState::Configured);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 16.0;
        ui.allocate_ui_with_layout(vec2(first, 172.0), Layout::top_down(Align::Min), |ui| { theme::ink(theme::AMBER(), 13, 2.5, 4)
            .shadow(egui::epaint::Shadow::NONE)
            .stroke(Stroke::new(2.5, theme::ON_YELLOW())).inner_margin(Margin::same(18)).show(ui, |ui| {
                ui.set_width((first - 36.0).max(0.0)); ui.set_min_height(130.0);
                ui.label(d("Dictate anywhere", 22.0, theme::ON_YELLOW()));
                ui.label(para(if data.dictating { "Stop here to copy, or stop with your shortcut in another app to paste." } else { "Use the shortcut in any app to turn speech into text." }, 13.0, theme::ON_YELLOW_DIM()));
                ui.add_space(14.0);
                ui.horizontal_wrapped(|ui| {
                    let label = if data.dictating { "Stop & copy" } else if data.busy { "Transcribing…" } else { "Start dictating" };
                    let enabled = data.dictating || (can_start && !data.busy && !data.meeting_active);
                    let hint = if !data.dictating && !data.busy { data.dictation_key.as_deref() } else { None };
                    let r = button(ui, label, true, enabled, hint);
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
                    if r.clicked() { action = Some(HomeAction::Dictate); }
                });
            }); });
        ui.allocate_ui_with_layout(vec2(second, 172.0), Layout::top_down(Align::Min), |ui| { theme::ink_card().inner_margin(Margin::same(18)).show(ui, |ui| {
            ui.set_width((second - 36.0).max(0.0)); ui.set_min_height(130.0);
            ui.label(d("Record a meeting", 22.0, theme::TEXT()));
            ui.label(para("Capture the conversation and create a transcript.", 13.0, theme::MUTED()));
            ui.add_space(14.0);
            ui.horizontal_wrapped(|ui| {
                let label = if data.meeting_active { "View meeting" } else { "Start meeting" };
                let enabled = data.meeting_active || (can_start && !data.dictating && !data.busy);
                let hint = if !data.meeting_active { data.meeting_key.as_deref() } else { None };
                let r = button(ui, label, false, enabled, hint);
                r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
                if r.clicked() { action = Some(HomeAction::Meeting); }
            });
        }); });
    });
    ui.add_space(18.0);
    if let Some(reason) = &data.attention {
        theme::soft(theme::Y1(), 9, 1.5, 0).stroke(Stroke::new(1.5, theme::WARN())).inner_margin(Margin::same(13)).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                let avail = ui.available_width();
                ui.vertical(|ui| { ui.set_width((avail - 170.0).max(140.0)); ui.label(b("A meeting needs attention", 14.0, theme::ON_YELLOW())); ui.label(para(reason, 12.5, theme::ON_YELLOW_DIM())); });
                ui.allocate_ui_with_layout(vec2(170.0, 48.0), Layout::right_to_left(Align::Center), |ui| {
                    let r = views::kit_button_compact_on(ui, "Review meeting", views::Btn::Secondary, 0.0, true);
                    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Review meeting"));
                    if r.clicked() { action = Some(HomeAction::ReviewMeeting); }
                });
            });
        });
        ui.add_space(16.0);
    }
    ui.label(d("Your insights", 20.0, theme::TEXT()));
    ui.add_space(8.0);
    theme::ink_card().inner_margin(Margin::same(18)).show(ui, |ui| {
        ui.set_width((width - 36.0).max(0.0));
        if data.stats.words == 0 && data.stats.days.is_empty() {
            ui.label(b("Your insights will appear here", 14.0, theme::TEXT()));
            ui.label(para("After your first dictation, see your words, estimated time saved and activity.", 13.0, theme::MUTED()));
        } else {
            let minutes = data.stats.minutes_saved(data.typing_wpm).round() as u64;
            let duration = if minutes >= 60 { format!("{} hr {} min", minutes / 60, minutes % 60) } else { format!("{minutes} min") };
            let metrics = [(format_words(data.stats.words), "Words dictated"), (duration, "Estimated time saved"), (format!("{} days", data.stats.active_days()), "Active days")];
            ui.horizontal_top(|ui| {
                let col = ui.available_width() / 3.0;
                ui.spacing_mut().item_spacing.x = 0.0;
                for (value, label) in metrics { ui.vertical(|ui| { ui.set_width(col); ui.label(d(value, 24.0, theme::TEXT())); ui.label(t(label, 12.5, theme::MUTED())); }); }
            });
            ui.add_space(10.0);
            ui.label(para("Time saved compares dictation time with your configured typing speed.", 12.0, theme::MUTED()));
            ui.add_space(10.0); ui.separator(); ui.add_space(10.0);
            heatmap(ui, &data.stats);
        }
    });
    action
}

fn format_words(count: u64) -> String {
    let raw = count.to_string();
    let mut out = String::new();
    for (i, ch) in raw.chars().enumerate() { if i > 0 && (raw.len() - i) % 3 == 0 { out.push(','); } out.push(ch); }
    out
}

fn heatmap(ui: &mut egui::Ui, stats: &Stats) {
    let width = ui.available_width();
    let label_w = 24.0;
    let cell = 16.0;
    let avail = (width - label_w).max(cell * 4.0);
    let weeks = ((avail + 6.0) / (cell + 6.0)).floor().max(4.0) as i64;
    let gap = (avail - weeks as f32 * cell) / (weeks as f32 - 1.0);
    let today = Local::now().date_naive();
    let sunday = today - Duration::days(today.weekday().num_days_from_sunday() as i64);
    let start = sunday - Duration::weeks(weeks - 1);
    let first = stats.first_day().unwrap_or(today);
    let busiest = stats.busiest_day().max(1);
    let (r, response) = ui.allocate_exact_size(vec2(width, 7.0 * 22.0 + 16.0), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, format!("Dictation activity from {} to {}, {} active days. Darker squares mean more dictations.", start.format("%d %b %Y"), today.format("%d %b %Y"), stats.active_days())));
    let p = ui.painter();
    for (i, day) in ["S", "M", "T", "W", "T", "F", "S"].iter().enumerate() {
        p.text(pos2(r.left() + 5.0, r.top() + i as f32 * 22.0 + cell / 2.0), Align2::CENTER_CENTER, day, FontId::proportional(10.0), theme::MUTED());
    }
    let mut last_month = 0;
    for wk in 0..weeks {
        let x = r.left() + label_w + wk as f32 * (cell + gap);
        for day in 0..7 {
            let date = start + Duration::days(wk * 7 + day);
            let rect = Rect::from_min_size(pos2(x, r.top() + day as f32 * 22.0), vec2(cell, cell));
            if date > today { p.rect_stroke(rect, 3.0, Stroke::new(1.0, theme::LINE()), egui::StrokeKind::Inside); }
            else if date < first {
                p.rect_filled(rect, 3.0, theme::TRACK());
                for k in [4.0, 9.0, 14.0] { p.line_segment([pos2(rect.left()+k-4.0,rect.bottom()-1.0),pos2(rect.left()+k+1.0,rect.top()+1.0)], Stroke::new(1.0,theme::LINE())); }
            } else {
                let n = stats.count_on(date);
                p.rect_filled(rect, 3.0, if n == 0 { theme::TRACK() } else { theme::HEAT()[((n as f32 / busiest as f32) * 3.99) as usize] });
            }
        }
        let day = start + Duration::days(wk * 7);
        if day.month() != last_month { if wk > 0 { p.text(pos2(x + cell / 2.0, r.bottom() - 4.0), Align2::CENTER_CENTER, day.format("%b").to_string(), FontId::proportional(10.0), theme::MUTED()); } last_month = day.month(); }
    }
    ui.horizontal(|ui| { ui.label(t("Less", 11.0, theme::MUTED())); for c in theme::HEAT() { let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover()); ui.painter().rect_filled(r, 2.0, c); } ui.label(t("More", 11.0, theme::MUTED())); });
}
