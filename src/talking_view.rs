use crate::talking::{Availability, Kind};
use crate::theme::{self, b, m, para, t};
use crate::{meeting, talking};
use eframe::egui::{self, FontFamily, FontId, Frame, Label, Margin, Sense, Stroke, text::LayoutJob, text::TextFormat, vec2};

const NO_KEY: &str = "Add a Brave Search key in Models to search";
const LABELS: [&str; 3] = ["YOU", "THEM", "BOTH"];

#[derive(Default)]
pub struct ViewState {
    pub panel_open: bool,
    pub seen_suggestions: Option<talking::Suggestions>,
    pub selection: Option<(String, egui::Pos2)>,
    transcript: String,
    transcribed: usize,
    galley: Option<(GalleyKey, std::sync::Arc<egui::Galley>)>,
    cursor: Option<egui::text_selection::CCursorRange>,
}

type GalleyKey = (usize, bool, f32);

impl ViewState {
    pub fn catch_up(&mut self, meeting: &meeting::Live) -> bool {
        let count = meeting.segments.len();
        if count == self.transcribed {
            return false;
        }
        if count < self.transcribed {
            self.transcript.clear();
            self.transcribed = 0;
        }
        if self.transcribed > 0 {
            self.transcript.push_str("\n\n");
        }
        self.transcript.push_str(&transcript_text(&meeting.segments[self.transcribed..]));
        self.transcribed = count;
        self.selection = None;
        true
    }
}

#[derive(Debug, PartialEq)]
pub enum ViewAction {
    Search(String),
    Copy(String),
    RemoveChip(String),
}

pub fn draw(ui: &mut egui::Ui, elapsed: f32, talking: &talking::Live, can_search: bool, vs: &mut ViewState) -> Option<ViewAction> {
    ui.painter().rect_filled(ui.max_rect(), 0.0, theme::PAPER());
    let pad = Frame::new().inner_margin(Margin::symmetric(14, 8));
    egui::Panel::top("tp-bar").frame(pad).show(ui, |ui| title_bar(ui, elapsed, talking));
    let removed = (!talking.chips.is_empty())
        .then(|| egui::Panel::bottom("tp-context").frame(pad.fill(theme::SURFACE())).show(ui, |ui| chips(ui, &talking.chips)).inner)
        .flatten();
    let picked = egui::Panel::left("tp-transcript")
        .resizable(true)
        .default_size(ui.available_width() * 0.52)
        .frame(Frame::new().inner_margin(Margin::same(14)))
        .show(ui, |ui| transcript(ui, can_search, vs))
        .inner;
    let copied = egui::CentralPanel::no_frame()
        .show(ui, |ui| {
            Frame::new()
                .inner_margin(Margin::same(14))
                .show(ui, |ui| egui::ScrollArea::vertical().id_salt("tp-notes").show(ui, |ui| notes(ui, talking, vs)).inner)
                .inner
        })
        .inner;
    removed.or(picked).or(copied)
}

fn title_bar(ui: &mut egui::Ui, elapsed: f32, talking: &talking::Live) {
    ui.horizontal(|ui| {
        let (dot, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
        ui.painter().circle_filled(dot.center(), 4.0, theme::RED());
        ui.label(m(format!("Talking Points · {}", crate::views::hhmmss(elapsed)), 13.0, theme::TEXT()));
        if let Some(at) = talking.updated_ms {
            let ago = (elapsed - at as f32 / 1000.0).max(0.0) as u64;
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(t(format!("updated {ago}s ago"), 11.5, theme::FAINT()));
            });
        }
    });
}

fn transcript_text(segments: &[meeting::Segment]) -> String {
    let lines: Vec<String> = segments
        .iter()
        .map(|s| {
            let who = s.voice.to_uppercase();
            if LABELS.contains(&who.as_str()) { format!("{who}  {}", s.text) } else { s.text.clone() }
        })
        .collect();
    lines.join("\n\n")
}

fn transcript_job(text: &str, wrap: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    let body = TextFormat {
        font_id: FontId::new(13.5, FontFamily::Proportional),
        color: theme::TEXT(),
        line_height: Some(20.0),
        ..Default::default()
    };
    for line in text.split_inclusive('\n') {
        let rest = match line.split_once("  ") {
            Some((who, rest)) if LABELS.contains(&who) => {
                let color = match who {
                    "YOU" => theme::GREEN(),
                    "THEM" => theme::ACCENT(),
                    _ => theme::FAINT(),
                };
                job.append(who, 0.0, TextFormat { font_id: FontId::new(11.0, theme::bold()), color, ..body.clone() });
                job.append("  ", 0.0, body.clone());
                rest
            }
            _ => line,
        };
        job.append(rest, 0.0, body.clone());
    }
    job
}

fn transcript(ui: &mut egui::Ui, can_search: bool, vs: &mut ViewState) -> Option<ViewAction> {
    let text = std::mem::take(&mut vs.transcript);
    let action = transcript_with(ui, &text, can_search, vs);
    vs.transcript = text;
    action
}

fn transcript_with(ui: &mut egui::Ui, text: &str, can_search: bool, vs: &mut ViewState) -> Option<ViewAction> {
    let mut view: &str = text;
    let (count, cache) = (vs.transcribed, &mut vs.galley);
    let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
        let key = (count, theme::is_light(), wrap);
        if let Some((k, galley)) = cache.as_ref()
            && *k == key
        {
            return galley.clone();
        }
        let galley = ui.fonts_mut(|f| f.layout_job(transcript_job(buf.as_str(), wrap)));
        *cache = Some((key, galley.clone()));
        galley
    };
    let out = egui::ScrollArea::vertical()
        .id_salt("tp-transcript-scroll")
        .stick_to_bottom(true)
        .show(ui, |ui| {
            egui::TextEdit::multiline(&mut view)
                .frame(Frame::NONE)
                .desired_width(f32::INFINITY)
                .layouter(&mut layouter)
                .show(ui)
        })
        .inner;
    if out.response.has_focus() && out.cursor_range != vs.cursor {
        vs.cursor = out.cursor_range;
        vs.selection = out.cursor_range.and_then(|r| {
            let picked = r.slice_str(text).trim();
            let at = out.galley_pos + out.galley.pos_from_cursor(r.primary).left_bottom().to_vec2();
            (!picked.is_empty()).then(|| (picked.to_owned(), at))
        });
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        vs.selection = None;
    }
    let (sel, at) = vs.selection.clone()?;
    let mut open = true;
    let mut action = None;
    egui::Popup::from_response(&out.response)
        .open_bool(&mut open)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .at_position(at)
        .show(|ui| {
            let shown: String = if sel.chars().count() > 32 { sel.chars().take(31).chain(['…']).collect() } else { sel.clone() };
            if ui.add_enabled(can_search, egui::Button::new(format!("Search “{shown}”"))).clicked() {
                action = Some(ViewAction::Search(sel.clone()));
            }
            if !can_search {
                ui.label(t(NO_KEY, 11.5, theme::FAINT()));
            }
            if ui.button("Copy").clicked() {
                action = Some(ViewAction::Copy(sel.clone()));
            }
        });
    if !open || action.is_some() {
        vs.selection = None;
    }
    action
}

fn off_reason(talking: &talking::Live) -> Option<Vec<String>> {
    Some(match talking.availability.as_ref()? {
        Availability::Ready => vec![talking.status.clone()?],
        Availability::NeedsMemory => vec!["Needs 16 GB of memory to run the text model on this Mac".into()],
        Availability::NeedsCloudOptIn { host } => vec![
            format!("Turn on “Send meeting transcripts to {host}” in Models to use Talking Points."),
            format!("While Talking Points runs, the meeting transcript is sent to {host} every few seconds."),
        ],
        Availability::NoTextModel => vec!["Choose a text model in Models to use Talking Points.".into()],
        Availability::Disabled => vec!["Talking Points is turned off in Models.".into()],
    })
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(b(text, 12.0, theme::FAINT()));
    ui.add_space(4.0);
}

fn notes(ui: &mut egui::Ui, talking: &talking::Live, vs: &mut ViewState) -> Option<ViewAction> {
    if let Some(lines) = off_reason(talking) {
        for line in lines {
            ui.label(para(line, 13.5, theme::MUTED()));
        }
        return None;
    }
    heading(ui, "So far");
    let summary = if talking.summary.is_empty() { "Nothing yet." } else { &talking.summary };
    ui.label(para(summary, 13.5, theme::TEXT()));
    ui.add_space(12.0);
    heading(ui, "Action points");
    for tracked in &talking.actions {
        ui.horizontal_wrapped(|ui| {
            ui.label(para(format!("{}: {}", tracked.action.who, tracked.action.what), 13.5, theme::TEXT()));
            if tracked.new {
                ui.label(b("NEW", 10.0, theme::ACCENT()));
            }
        });
    }
    ui.add_space(12.0);
    let suggestions = talking.suggestions.as_ref().filter(|s| !s.items.is_empty())?;
    if !vs.panel_open {
        if vs.seen_suggestions.as_ref() == Some(suggestions) {
            return None;
        }
        let note = theme::soft(theme::ACCENT(), 10, 0.0, 0).inner_margin(Margin::symmetric(10, 8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                theme::soft(theme::ON_FILL(), 9, 0.0, 0).inner_margin(Margin::symmetric(7, 1)).show(ui, |ui| {
                    ui.add(Label::new(b(suggestions.items.len().to_string(), 11.0, theme::ACCENT())).selectable(false));
                });
                ui.vertical(|ui| {
                    ui.add(Label::new(m("Suggestions", 13.0, theme::ON_FILL())).selectable(false));
                    ui.add(Label::new(t(&suggestions.reason, 12.0, theme::ON_FILL_DIM())).selectable(false));
                });
            });
        });
        let name = format!("{} suggestions: {}", suggestions.items.len(), suggestions.reason);
        if named(ui, note.response.interact(Sense::click()), &name).clicked() {
            vs.panel_open = true;
        }
        return None;
    }
    let mut action = None;
    theme::soft(theme::SURFACE(), 10, 1.5, 0).inner_margin(Margin::same(10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(m("Suggestions", 12.5, theme::ACCENT()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if cross(ui, "Close suggestions").clicked() {
                    vs.panel_open = false;
                    vs.seen_suggestions = Some(suggestions.clone());
                }
            });
        });
        for s in &suggestions.items {
            ui.separator();
            ui.horizontal_top(|ui| {
                let (kind, what) = if s.kind == Kind::Reply { ("REPLY", "reply") } else { ("ASK", "question") };
                ui.add_sized(vec2(44.0, 16.0), Label::new(b(kind, 10.0, theme::FAINT())));
                ui.allocate_ui(vec2(ui.available_width() - 40.0, 0.0), |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(Label::new(para(&s.text, 13.0, theme::TEXT())).wrap());
                });
                if link(ui, "copy", &format!("Copy {what}: {}", s.text)).clicked() {
                    action = Some(ViewAction::Copy(s.text.clone()));
                }
            });
        }
    });
    action
}

fn chips(ui: &mut egui::Ui, chips: &[talking::Chip]) -> Option<ViewAction> {
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        ui.label(t("Context:", 11.5, theme::MUTED()));
        for chip in chips {
            Frame::new()
                .fill(theme::PAPER())
                .stroke(Stroke::new(1.0, theme::LINE()))
                .corner_radius(255)
                .inner_margin(Margin::symmetric(8, 2))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(t(&chip.query, 11.5, theme::TEXT()));
                        if cross(ui, &format!("Remove {}", chip.query)).clicked() {
                            action = Some(ViewAction::RemoveChip(chip.query.clone()));
                        }
                    });
                });
        }
    });
    action
}

fn cross(ui: &mut egui::Ui, name: &str) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::click());
    let color = if resp.hovered() { theme::TEXT() } else { theme::FAINT() };
    crate::views::kit_icon_at(ui.painter(), "x", r.center(), color, 12.0);
    named(ui, resp, name).on_hover_text(name)
}

fn link(ui: &mut egui::Ui, text: &str, name: &str) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), FontId::new(11.5, theme::medium()), theme::ACCENT());
    let (r, resp) = ui.allocate_exact_size(galley.size(), Sense::click());
    ui.painter().galley(r.min, galley, theme::ACCENT());
    named(ui, resp, name)
}

fn named(ui: &egui::Ui, resp: egui::Response, name: &str) -> egui::Response {
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    if resp.has_focus() {
        ui.painter().rect_stroke(resp.rect.expand(3.0), 6.0, Stroke::new(2.0, theme::INK()), egui::StrokeKind::Outside);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meeting::Segment;
    use crate::talking::{Availability, Chip, Kind, Suggestion, Suggestions};
    use egui_kittest::Harness;
    use egui_kittest::kittest::{NodeT, Queryable};
    use std::sync::Mutex;

    struct Fixture {
        meeting: meeting::Live,
        talking: Mutex<talking::Live>,
        vs: ViewState,
        can_search: bool,
        actions: Vec<ViewAction>,
    }

    fn segment(at_ms: u64, voice: &str, text: &str) -> Segment {
        Segment { at_ms, secs: 3.0, text: text.into(), voice: voice.into() }
    }

    fn meeting() -> meeting::Live {
        meeting::Live {
            elapsed: 1394.0,
            segments: vec![
                segment(1_380_000, "them", "We'd meter it through Stripe, like Linear does."),
                segment(1_385_000, "you", "And the existing annual customers?"),
                segment(1_389_000, "them", "Could you get us a rough cost estimate by Friday?"),
            ],
            ..Default::default()
        }
    }

    fn ready() -> talking::Live {
        talking::Live {
            summary: "Usage-based pricing for Q3; annual-contract migration unresolved.".into(),
            actions: vec![talking::Tracked {
                action: talking::Action { who: "Raj".into(), what: "list renewing contracts".into() },
                new: false,
            }],
            availability: Some(Availability::Ready),
            updated_ms: Some(1_390_000),
            ..Default::default()
        }
    }

    fn suggestions() -> Suggestions {
        Suggestions {
            reason: "You were asked for an estimate by Friday".into(),
            items: vec![
                Suggestion { kind: Kind::Reply, text: "Yes, Friday works.".into() },
                Suggestion { kind: Kind::Reply, text: "Can I send a range Thursday?".into() },
                Suggestion { kind: Kind::Ask, text: "Should it include the annual customers?".into() },
            ],
        }
    }

    fn harness(talking: talking::Live, vs: ViewState) -> Harness<'static, Fixture> {
        harness_with(talking, vs, false)
    }

    fn harness_with(talking: talking::Live, vs: ViewState, can_search: bool) -> Harness<'static, Fixture> {
        let fixture = Fixture { meeting: meeting(), talking: Mutex::new(talking), vs, can_search, actions: Vec::new() };
        Harness::builder()
            .with_size(egui::vec2(640.0, 460.0))
            .with_context_setup(crate::theme::install)
            .build_ui_state(
                |ui, f: &mut Fixture| {
                    f.vs.catch_up(&f.meeting);
                    let t = f.talking.lock().unwrap().clone();
                    if let Some(a) = draw(ui, f.meeting.elapsed, &t, f.can_search, &mut f.vs) {
                        f.actions.push(a);
                    }
                },
                fixture,
            )
    }

    #[test]
    fn the_title_bar_shows_elapsed_time_and_how_long_ago_the_notes_changed() {
        let h = harness(ready(), ViewState::default());
        h.get_by_label_contains("23:14");
        h.get_by_label("updated 4s ago");
    }

    #[test]
    fn the_transcript_labels_each_line_with_who_spoke() {
        let h = harness(ready(), ViewState::default());
        let text = h.get_by_role(egui::accesskit::Role::MultilineTextInput).value().unwrap_or_default();
        assert!(text.contains("THEM  We'd meter it through Stripe"), "{text}");
        assert!(text.contains("YOU  And the existing annual customers?"), "{text}");
    }

    #[test]
    fn shows_a_notification_with_count_and_reason_when_suggestions_arrive() {
        let mut h = harness(ready(), ViewState::default());
        h.get_by_label("So far");
        h.get_by_label("Action points");
        assert!(h.query_by_label("You were asked for an estimate by Friday").is_none());

        h.state_mut().talking.lock().unwrap().suggestions = Some(suggestions());
        h.run();

        h.get_by_label("3");
        h.get_by_label("You were asked for an estimate by Friday");
    }

    #[test]
    fn clicking_the_notification_opens_reply_and_ask_items() {
        let mut h = harness(talking::Live { suggestions: Some(suggestions()), ..ready() }, ViewState::default());
        h.get_by_label("Suggestions").click();
        h.run();

        assert!(h.state().vs.panel_open);
        assert_eq!(h.get_all_by_label("REPLY").count(), 2);
        assert_eq!(h.get_all_by_label("ASK").count(), 1);
        h.get_by_label("Should it include the annual customers?");

        h.get_by_label("Copy question: Should it include the annual customers?").click();
        h.run();
        assert_eq!(h.state().actions, vec![ViewAction::Copy("Should it include the annual customers?".into())]);

        h.get_by_label("Close suggestions").click();
        h.run();
        assert!(!h.state().vs.panel_open);
        assert!(h.query_by_label("REPLY").is_none());
    }

    #[test]
    fn the_panel_stays_open_when_the_next_update_has_no_suggestions() {
        let mut h = harness(talking::Live { suggestions: Some(suggestions()), ..ready() }, ViewState::default());
        h.get_by_label("Suggestions").click();
        h.run();

        let segments = h.state().meeting.segments.clone();
        let reply = |_: talking::Job, _: &str, _: &str| Ok(r#"{"reason":"","items":[]}"#.to_string());
        talking::step(&h.state().talking, talking::Job::Suggest, &segments, 1_393_000, &reply, &|_| {});
        h.run();

        assert!(h.state().vs.panel_open);
        h.get_by_label("Yes, Friday works.");
        h.get_by_label("Should it include the annual customers?");
        h.get_by_label("updated 1s ago");
    }

    #[test]
    fn closing_the_panel_hides_the_notification_until_new_suggestions_arrive() {
        let mut h = harness(talking::Live { suggestions: Some(suggestions()), ..ready() }, ViewState::default());
        h.get_by_label("Suggestions").click();
        h.run();
        h.get_by_label("Close suggestions").click();
        h.run();
        assert!(h.query_by_label("Suggestions").is_none());
        assert!(h.query_by_label("You were asked for an estimate by Friday").is_none());

        let newer = Suggestions { reason: "They asked who owns the migration".into(), ..suggestions() };
        h.state_mut().talking.lock().unwrap().suggestions = Some(newer);
        h.run();
        h.get_by_label("They asked who owns the migration");
    }

    fn pick(h: &mut Harness<'static, Fixture>, text: &str) {
        h.state_mut().vs.selection = Some((text.into(), egui::pos2(20.0, 80.0)));
        h.run();
    }

    fn select_all_of_the_transcript(h: &mut Harness<'static, Fixture>) {
        h.get_by_role(egui::accesskit::Role::MultilineTextInput).focus();
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
        h.run();
    }

    fn press(h: &mut Harness<'static, Fixture>, pos: egui::Pos2, pressed: bool) {
        h.event(egui::Event::PointerMoved(pos));
        h.step();
        h.event(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
        if pressed { h.step() } else { h.run(); }
    }

    #[test]
    fn selecting_transcript_text_offers_search_and_copy_and_the_menu_stays_put_while_copy_is_clicked() {
        let mut h = harness(ready(), ViewState::default());
        select_all_of_the_transcript(&mut h);

        assert!(h.get_by_label_contains("Search “").accesskit_node().is_disabled());
        let copy = h.get_by_label("Copy").rect();
        h.get_by_label("Copy").focus();
        h.run();
        assert_eq!(h.get_by_label("Copy").rect(), copy);
        press(&mut h, copy.center(), true);
        assert_eq!(h.get_by_label("Copy").rect(), copy);
        press(&mut h, copy.center(), false);

        let all = transcript_text(&h.state().meeting.segments);
        assert_eq!(h.state().actions, vec![ViewAction::Copy(all)]);
        assert!(h.query_by_label("Copy").is_none());
    }

    #[test]
    fn escape_closes_the_selection_menu() {
        let mut h = harness(ready(), ViewState::default());
        select_all_of_the_transcript(&mut h);
        h.get_by_label("Copy");
        h.key_press(egui::Key::Escape);
        h.run();
        assert!(h.query_by_label("Copy").is_none());
    }

    #[test]
    fn the_transcript_is_rebuilt_only_when_segments_change() {
        let mut m = meeting();
        let mut vs = ViewState::default();
        assert!(vs.catch_up(&m));
        assert!(!vs.catch_up(&m));
        m.elapsed += 5.0;
        assert!(!vs.catch_up(&m));

        m.segments.push(segment(1_396_000, "you", "Friday works."));
        assert!(vs.catch_up(&m));
        assert_eq!(vs.transcript, transcript_text(&m.segments));
    }

    #[test]
    fn new_segments_close_the_selection_menu() {
        let mut h = harness(ready(), ViewState::default());
        select_all_of_the_transcript(&mut h);
        h.get_by_label("Copy");

        h.state_mut().meeting.segments.push(segment(1_396_000, "you", "Friday works."));
        h.run();
        assert!(h.query_by_label("Copy").is_none());
    }

    #[test]
    fn search_is_offered_and_returns_the_selection_when_search_is_available() {
        let mut h = harness_with(ready(), ViewState::default(), true);
        pick(&mut h, "Stripe");
        assert!(h.query_by_label("Add a Brave Search key in Models to search").is_none());

        h.get_by_label("Search “Stripe”").click();
        h.run();
        assert_eq!(h.state().actions, vec![ViewAction::Search("Stripe".into())]);
    }

    #[test]
    fn the_notification_and_copy_buttons_are_named_for_assistive_tech() {
        let mut h = harness(talking::Live { suggestions: Some(suggestions()), ..ready() }, ViewState::default());
        let note = h.get_by_label("3 suggestions: You were asked for an estimate by Friday");
        assert_eq!(note.accesskit_node().role(), egui::accesskit::Role::Button);
        note.click();
        h.run();

        h.get_by_label("Copy reply: Yes, Friday works.").click();
        h.run();
        h.get_by_label("Copy question: Should it include the annual customers?");
        assert_eq!(h.state().actions, vec![ViewAction::Copy("Yes, Friday works.".into())]);
    }

    #[test]
    fn search_is_disabled_with_a_reason_when_there_is_no_brave_key() {
        let mut h = harness(ready(), ViewState::default());
        pick(&mut h, "Stripe");
        let search = h.get_by_label("Search “Stripe”");
        assert!(search.accesskit_node().is_disabled());
        h.get_by_label("Add a Brave Search key in Models to search");

        h.get_by_label("Copy").click();
        h.run();
        assert_eq!(h.state().actions, vec![ViewAction::Copy("Stripe".into())]);
    }

    #[test]
    fn shows_why_talking_points_are_off_when_memory_is_too_low() {
        let h = harness(talking::Live { availability: Some(Availability::NeedsMemory), ..ready() }, ViewState::default());
        h.get_by_label("Needs 16 GB of memory to run the text model on this Mac");
        assert!(h.query_by_label("So far").is_none());
    }

    #[test]
    fn asks_for_the_cloud_opt_in_in_models_when_it_is_missing() {
        let host = "api.openai.com".to_string();
        let h = harness(talking::Live { availability: Some(Availability::NeedsCloudOptIn { host }), ..ready() }, ViewState::default());
        h.get_by_label_contains("Send meeting transcripts to api.openai.com");
        h.get_by_label("While Talking Points runs, the meeting transcript is sent to api.openai.com every few seconds.");
        assert!(h.query_by_label("So far").is_none());
    }

    #[test]
    fn asks_for_a_text_model_when_there_is_none() {
        let h = harness(talking::Live { availability: Some(Availability::NoTextModel), ..ready() }, ViewState::default());
        h.get_by_label_contains("text model in Models");
        assert!(h.query_by_label("So far").is_none());
    }

    #[test]
    fn says_the_text_model_is_unreachable_in_place_of_the_notes() {
        let unreachable = "Can't reach the text model. Check it in Models.";
        let h = harness(talking::Live { status: Some(unreachable.into()), ..ready() }, ViewState::default());
        h.get_by_label(unreachable);
        assert!(h.query_by_label("So far").is_none());
    }

    #[test]
    fn removing_a_chip_returns_it_from_the_view() {
        let chips = vec![
            Chip { query: "Stripe metered billing".into(), text: String::new() },
            Chip { query: "Linear pricing".into(), text: String::new() },
        ];
        let mut h = harness(talking::Live { chips, ..ready() }, ViewState::default());
        h.get_by_label("Stripe metered billing");
        h.get_by_label("Remove Linear pricing").click();
        h.run();
        assert_eq!(h.state().actions, vec![ViewAction::RemoveChip("Linear pricing".into())]);
    }
}
