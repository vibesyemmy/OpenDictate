//! Drawing: sidebar, Home insights, History and Dictionary. The Settings window
//! lives in `settings_view`.

use crate::app::{DictateApp, Page, SettingsTab};
use crate::settings::{Action, DictWord, KeepHistory, Trigger};
use crate::stats;
use crate::theme::{self, b, d, m, para, t};
use chrono::{Datelike, Duration, Local};
use eframe::egui::{self, Align, Align2, Color32, FontId, Layout, Margin, Rect, Sense, Stroke, StrokeKind, Vec2, pos2, vec2};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn draw(app: &mut DictateApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    egui::Panel::left("sidebar")
        .exact_size(216.0)
        .resizable(false)
        .show_separator_line(false)
        .frame(egui::Frame::new().fill(theme::BG_SHEER()).inner_margin(Margin { left: 10, right: 8, top: 10, bottom: 10 }))
        .show(ui, |ui| sidebar(app, ui));

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
    toast(app, &ctx);
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
/// The variant recolours the whole box so the tone reads before the words do:
/// yellow when something is merely missing, red when nothing can run. It takes
/// the side panel's hard offset shadow rather than the soft one, because a
/// notice is meant to stop you.
///
/// One deviation: the kit shadows these in --on-yellow and --on-red, both of
/// which are paper. On a dark page paper is the page, so the shadow would be
/// invisible; it uses ink here, as the side panel does.
fn setup_notice(app: &mut DictateApp, ui: &mut egui::Ui, w: f32) {
    let (speech_ok, _) = app.speech_ready();
    let (text_ok, text) = app.text_ready();
    // On a cold start the text model is still being looked for. Saying
    // something is missing before we know would be a lie that fixes itself.
    let still_looking = text.starts_with("Checking");
    if speech_ok && (text_ok || still_looking || app.s.hide_text_model_notice) {
        return;
    }
    // The model names mean nothing to someone who never picked one. Say what
    // works, what does not, and what it is for; the detail lives in Models.
    let (fill, head, body, action) = if speech_ok {
        (
            theme::Y1(),
            "ONE OPTIONAL EXTRA",
            "Dictation works. If you also want \"um\" and \"uh\" taken out and small slips tidied up, that needs one more download. Without it your words are pasted exactly as you said them.",
            "SET IT UP",
        )
    } else {
        (
            theme::RED(),
            "DICTATION ISN'T READY YET",
            "Your shortcut will not do anything yet. To turn what you say into text, Open Dictate needs a speech model downloaded onto this Mac.",
            "SET UP DICTATION",
        )
    };
    // Yellow is one value in both themes, red is not, so what goes on top
    // of them cannot be one token either.
    let fg = if speech_ok { theme::ON_YELLOW() } else { theme::ON_FILL() };
    egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(2.5, fg))
        .corner_radius(16)
        .shadow(egui::epaint::Shadow { offset: [5, 6], blur: 0, spread: 0, color: theme::INK() })
        .inner_margin(Margin { left: 14, right: 14, top: 11, bottom: 12 })
        .show(ui, |ui| {
            ui.set_width(w - 33.0);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    // Reserve for the actions: both are the width of the
                    // widest label, so two cost twice one plus the gap.
                    let widest = if speech_ok { "NO THANKS" } else { action };
                    let one = ui.painter()
                        .layout_no_wrap(widest.to_string(), FontId::new(11.0, theme::display()), fg)
                        .rect
                        .width()
                        + 38.0;
                    let actions = if speech_ok { one * 2.0 + 6.0 } else { one } + 16.0;
                    ui.set_width(w - 33.0 - actions);
                    ui.spacing_mut().item_spacing.y = 5.0;
                    ui.horizontal(|ui| {
                        // Allocated rather than painted at the cursor, so the
                        // horizontal layout centres glyph and caption together.
                        let (ir, _) = ui.allocate_exact_size(vec2(19.0, 19.0), Sense::hover());
                        kit_icon(ui.painter(), if speech_ok { "bell" } else { "x" }, ir.center(), fg);
                        ui.add_space(3.0);
                        ui.label(b(head, 10.0, fg));
                    });
                    ui.label(para(body, 13.0, fg));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // The kit gives .notice-btns buttons flex: 1 1 0, so they
                    // share one width. Measure every label that will be shown
                    // and size them all to the widest.
                    let font = FontId::new(11.0, theme::display());
                    let labels: &[&str] = if speech_ok { &[action, "NO THANKS"] } else { &[action] };
                    let bw = labels
                        .iter()
                        .map(|l| ui.painter().layout_no_wrap(l.to_string(), font.clone(), fg).rect.width())
                        .fold(0.0_f32, f32::max)
                        + 38.0;
                    // Painted rather than egui Buttons: giving a Button an
                    // explicit fill and stroke freezes it, so it never showed
                    // a hover. The kit inverts these on hover.
                    let notice_btn = |ui: &mut egui::Ui, label: &str| {
                        let (br, bresp) = ui.allocate_exact_size(vec2(bw, 30.0), Sense::click());
                        let hot = bresp.hovered();
                        let p = ui.painter();
                        p.rect_filled(br, 8.0, if hot { fg } else { fill });
                        p.rect_stroke(br, 8.0, Stroke::new(2.0, fg), egui::StrokeKind::Inside);
                        p.text(br.center(), Align2::CENTER_CENTER, label, font.clone(), if hot { fill } else { fg });
                        bresp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
                    };
                    if notice_btn(ui, action) {
                        app.settings_tab = Some(SettingsTab::Models);
                        app.refresh_health(ui.ctx());
                    }
                    // Declining is only offered for the optional download.
                    // Hiding the blocking one would leave a shortcut that
                    // silently does nothing and no sign of why.
                    if speech_ok {
                        ui.add_space(6.0);
                        if notice_btn(ui, "NO THANKS") {
                            app.s.hide_text_model_notice = true;
                            app.changed();
                        }
                    }
                });
            });
        });
}

fn sidebar(app: &mut DictateApp, ui: &mut egui::Ui) {
    // The kit's .sidepanel: a 3px ink outline over a hard ink shadow. Inset so
    // the shadow has somewhere to fall inside the pane.
    let rect = ui.max_rect().shrink2(vec2(7.0, 10.0));
    // Solid, like the cards. A see-through panel put the backdrop between
    // the outline and the exposed sliver of the shadow, so the two read as a
    // doubled border; filled, they meet and the shadow is one rounded mass
    // again. Only the strip the panel sits in lets the backdrop through.
    let p = ui.painter();
    p.rect_filled(rect.translate(vec2(5.0, 6.0)), 18.0, theme::INK());
    p.rect_filled(rect, 18.0, theme::SURFACE());
    p.rect_stroke(rect, 18.0, Stroke::new(3.0, theme::INK()), egui::StrokeKind::Inside);

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
        ui.label(m(text, 12.0, tint));
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
    let font = FontId::new(15.0, theme::display());
    let text = label.to_uppercase();
    let pad = if kind == Btn::Primary { 32.0 } else { 22.0 };
    let measured = ui.painter().layout_no_wrap(text.clone(), font.clone(), theme::INK()).rect.width();
    let w = (measured + pad * 2.0).max(min_w);
    let sense = if on { Sense::click() } else { Sense::hover() };
    let (slot, resp) = ui.allocate_exact_size(vec2(w + 5.0, 50.0), sense);
    let base = Rect::from_min_size(slot.min, vec2(w, 44.0));
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
        p.rect_filled(r.translate(vec2(lift, lift + 1.0)), 16.0, theme::INK());
    }
    p.rect_filled(r, 16.0, fill);
    p.rect_stroke(r, 16.0, Stroke::new(3.0, edge), egui::StrokeKind::Inside);
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &text,
        0.0,
        egui::TextFormat { font_id: font, color: fg, extra_letter_spacing: 15.0 * 0.045, ..Default::default() },
    );
    let g = p.layout_job(job);
    p.galley(pos2(r.center().x - g.rect.width() / 2.0, r.center().y - g.rect.height() / 2.0), g, fg);
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

fn home(app: &mut DictateApp, ui: &mut egui::Ui, width: f32) {
    let right_w = 186.0;
    let gap = (width * 0.085).clamp(24.0, 92.0);
    let main_w = width - right_w - gap;
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.set_width(main_w);
            setup_notice(app, ui, main_w);
            ui.label(d("Say it, skip the typing", 46.0, theme::TEXT()));
            ui.add_space(20.0);
            ui.horizontal(|ui| {
                start_button(app, ui, 196.0);
                // The shortcut was only discoverable inside a rail card most
                // people stop seeing. Next to the button it is learned once.
                if let Some(k) = app.s.keys(Action::Dictate).first() {
                    ui.add_space(14.0);
                    // A key chip fills the height of the row it is in. This
                    // row is 50pt tall because of the button, so the hint
                    // gets a row of the rail's height instead.
                    ui.allocate_ui_with_layout(vec2(220.0, 30.0), Layout::left_to_right(Align::Center), |ui| {
                        ui.label(t("or press", 13.5, theme::MUTED()));
                        ui.add_space(2.0);
                        kbd(ui, k);
                    });
                }
            });
            ui.add_space(30.0);
            insights(app, ui, main_w);
        });
        ui.add_space(gap);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.set_width(right_w);
            ui.add_space(181.0);
            right_column(app, ui);
        });
    });
}

fn insights(app: &mut DictateApp, ui: &mut egui::Ui, w: f32) {
    // Dotted backdrop, filled in once we know how tall the cards are.
    let bg_idx = ui.painter().add(egui::Shape::Noop);
    let top = ui.cursor().top();
    let left = ui.cursor().left();
    let s = &app.stats;
    let mut share = false;
    ui.horizontal(|ui| {
        ui.add_space(0.0);
        ui.label(d("Insights", 24.0, theme::MUTED()));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            share = kit_button(ui, &format!("{}  Share", crate::icon::SHARE), Btn::Secondary, 0.0).clicked();
        });
    });
    if share {
        let summary = format!(
            "{} words dictated, {} saved, {:.0} WPM, {} active days.",
            s.words,
            duration_text(s.minutes_saved(app.s.typing_wpm)),
            s.wpm(),
            s.active_days()
        );
        ui.ctx().copy_text(summary);
        app.notify("Insights copied to clipboard", false);
    }
    ui.add_space(8.0);

    let s = &app.stats;
    let card_w = (w - 16.0) / 2.0;
    let saved = s.minutes_saved(app.s.typing_wpm);
    let total = s.seconds / 60.0;
    let cards = [
        (vec![(compact(s.words as f64), "words")], "Words dictated"),
        (duration_parts(saved), "Time saved"),
        (vec![(format!("{:.0}", s.wpm()), "WPM")], "Average dictation speed"),
        (duration_parts(total), "Total dictation time"),
    ];
    for row in cards.chunks(2) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 16.0;
            for (parts, label) in row {
                stat_card(ui, card_w, parts, label);
            }
        });
        ui.add_space(8.0);
    }
    activity_card(app, ui, w);
    ui.add_space(8.0);
    personalization_card(app, ui, w);

    let rect = Rect::from_min_max(pos2(left - 12.0, top - 12.0), pos2(left + w + 12.0, ui.cursor().top() + 4.0));
    let mut shapes = vec![egui::Shape::rect_filled(rect, 14.0, theme::PANEL_SHEER())];
    let mut y = rect.top() + 4.0;
    while y < rect.bottom() - 2.0 {
        let mut x = rect.left() + 4.0;
        while x < rect.right() - 2.0 {
            shapes.push(egui::Shape::rect_filled(Rect::from_min_size(pos2(x, y), vec2(1.0, 1.0)), 0.0, theme::INK().gamma_multiply(0.08)));
            x += 6.0;
        }
        y += 6.0;
    }
    ui.painter().set(bg_idx, egui::Shape::Vec(shapes));
}

fn stat_card(ui: &mut egui::Ui, w: f32, parts: &[(String, &str)], label: &str) {
    theme::ink_card().inner_margin(Margin { left: 16, right: 16, top: 16, bottom: 14 }).show(ui, |ui| {
        ui.set_width(w - 32.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (v, unit) in parts {
                    ui.label(d(v.clone(), 25.0, theme::TEXT()));
                    ui.label(b(*unit, 12.5, theme::MUTED()));
                    ui.add_space(2.0);
                }
            });
            ui.label(t(label, 14.0, theme::MUTED()));
        });
    });
}

fn activity_card(app: &mut DictateApp, ui: &mut egui::Ui, w: f32) {
    let s = &app.stats;
    theme::ink_card().inner_margin(Margin::same(16)).show(ui, |ui| {
        ui.set_width(w - 32.0);
        let col = (w - 32.0) / 3.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for (n, label) in [(s.active_days(), "Active days"), (s.current_streak(), "Current streak"), (s.longest_streak(), "Longest streak")] {
                ui.vertical(|ui| {
                    ui.set_width(col);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.label(d(n.to_string(), 23.0, theme::TEXT()));
                        ui.label(b(if n == 1 { "day" } else { "days" }, 12.5, theme::MUTED()));
                    });
                    ui.label(t(label, 13.0, theme::MUTED()));
                });
            }
        });
        ui.add_space(14.0);

        // One column per week, rows Sunday..Saturday, this week on the right.
        let label_w = 16.0;
        let cell = 16.0;
        let avail = w - 32.0 - label_w;
        let weeks = ((avail + 6.0) / (cell + 6.0)).floor().max(4.0) as i64;
        let gapx = (avail - weeks as f32 * cell) / (weeks as f32 - 1.0);
        let today = Local::now().date_naive();
        let this_sunday = today - Duration::days(today.weekday().num_days_from_sunday() as i64);
        let start = this_sunday - Duration::weeks(weeks - 1);
        let first = s.first_day().unwrap_or(today);
        let busiest = s.busiest_day().max(1);
        let (r, _) = ui.allocate_exact_size(vec2(w - 32.0, 7.0 * 22.0 + 14.0), Sense::hover());
        let p = ui.painter();
        for (i, d) in ["S", "M", "T", "W", "T", "F", "S"].iter().enumerate() {
            p.text(pos2(r.left() + 4.0, r.top() + i as f32 * 22.0 + cell / 2.0), Align2::CENTER_CENTER, d, FontId::proportional(9.0), theme::MUTED());
        }
        let hatch = Stroke::new(1.0, theme::LINE());
        let mut last_month = 0;
        for wk in 0..weeks {
            let x = r.left() + label_w + wk as f32 * (cell + gapx);
            for dow in 0..7 {
                let d = start + Duration::days(wk * 7 + dow);
                let cr = Rect::from_min_size(pos2(x, r.top() + dow as f32 * 22.0), vec2(cell, cell));
                if d > today {
                    p.rect_stroke(cr, 3.0, Stroke::new(1.0, theme::LINE()), StrokeKind::Inside);
                } else if d < first {
                    // Before the first dictation: striped, like the original.
                    p.rect_filled(cr, 3.0, theme::TRACK());
                    for k in [4.0, 9.0, 14.0] {
                        p.line_segment([pos2(cr.left() + k - 4.0, cr.bottom() - 1.0), pos2(cr.left() + k + 1.0, cr.top() + 1.0)], hatch);
                    }
                } else {
                    let n = s.count_on(d);
                    let c = if n == 0 { theme::TRACK() } else { theme::HEAT()[((n as f32 / busiest as f32) * 3.99) as usize] };
                    p.rect_filled(cr, 3.0, c);
                }
            }
            let d = start + Duration::days(wk * 7);
            if d.month() != last_month {
                if wk > 0 {
                    p.text(pos2(x + cell / 2.0, r.bottom() - 4.0), Align2::CENTER_CENTER, d.format("%b").to_string(), FontId::proportional(9.0), theme::MUTED());
                }
                last_month = d.month();
            }
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(t("Less", 12.0, theme::MUTED()));
            for c in theme::HEAT() {
                let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                ui.painter().rect_filled(r, 2.0, c);
            }
            ui.label(t("More", 12.0, theme::MUTED()));
        });
    });
}

fn personalization_card(app: &mut DictateApp, ui: &mut egui::Ui, w: f32) {
    let words = app.s.dictionary.len();
    theme::ink_card().inner_margin(Margin::same(16)).show(ui, |ui| {
        ui.set_width(w - 32.0);
        ui.set_min_height(210.0);
        let top = ui.min_rect();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (r, _) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::hover());
            ui.painter().rect_filled(r, 5.0, theme::TRACK());
            draw_icon(ui.painter(), "spark", r.center(), theme::MUTED());
            ui.label(d(words.to_string(), 25.0, theme::TEXT()));
            ui.label(b(if words == 1 { "word" } else { "words" }, 12.5, theme::MUTED()));
        });
        ui.label(t(format!("Personalization · {} tone", app.s.tone.label()), 14.0, theme::MUTED()));
        ui.add_space(10.0);
        if btn(ui, "Personalize").clicked() {
            app.settings_tab = Some(SettingsTab::Personalization);
        }
        // Radar-style circle with one dot per dictionary word.
        let c = pos2(top.left() + w - 32.0 - 100.0, top.top() + 105.0);
        let p = ui.painter();
        p.circle_filled(c, 100.0, theme::TRACK());
        for k in 0..5 {
            let a = k as f32 / 5.0 * std::f32::consts::TAU - 1.2;
            p.line_segment([c, c + vec2(a.cos(), a.sin()) * 100.0], Stroke::new(1.0, theme::LINE()));
        }
        for (i, wd) in app.s.dictionary.iter().enumerate().take(60) {
            let a = i as f32 * 2.399;
            let rad = 18.0 + (i as f32 * 13.0) % 74.0;
            p.circle_filled(c + vec2(a.cos(), a.sin()) * rad, 2.5, if wd.auto { theme::GREEN() } else { theme::HEAT()[3] });
        }
        p.circle_filled(c, 6.0, theme::HEAT()[3]);
        p.circle_stroke(c, 7.5, Stroke::new(1.0, theme::ACCENT()));
    });
}

/// The one thing the app is for. It sat third in the right rail, under a card
/// of keyboard shortcuts, on a page that opens on statistics; it belongs under
/// the title that invites it.
fn start_button(app: &mut DictateApp, ui: &mut egui::Ui, min_w: f32) {
    let rec = app.recording.as_ref().map(|(_, a, _)| *a);
    let busy = app.busy.is_some();
    let label = match (rec, busy) {
        (Some(_), _) => format!("{}  Stop & paste", crate::icon::SQUARE),
        (None, true) => "Thinking…".to_string(),
        _ => format!("{}  Start dictating", crate::icon::MIC),
    };
    // While the text is being cleaned up there is nothing to press, but the
    // hero must not collapse around the gap, so the button disables in place.
    if kit_button_on(ui, &label, Btn::Primary, min_w, !busy).clicked() {
        if rec.is_some() {
            let ctx = ui.ctx().clone();
            app.stop_recording(&ctx);
        } else {
            app.start_recording(Action::Dictate);
        }
    }
}

fn right_column(app: &mut DictateApp, ui: &mut egui::Ui) {
    theme::soft(theme::CARD_DIM(), 14, 2.5, 3).inner_margin(Margin::same(18)).show(ui, |ui| {
        ui.set_width(186.0 - 36.0);
        for (i, trigger) in Trigger::ALL.iter().enumerate() {
            ui.label(t(trigger.label(), 12.5, theme::MUTED()));
            ui.horizontal_wrapped(|ui| {
                let keys = app.s.trigger_keys(*trigger);
                if keys.is_empty() {
                    ui.label(t("Not set", 12.0, theme::FAINT()));
                }
                for k in keys.iter().take(2) {
                    kbd(ui, k);
                }
            });
            if i + 1 < Trigger::ALL.len() {
                ui.add_space(6.0);
            }
        }
    });
    ui.add_space(24.0);

    // One card for both models. Four facts each was three too many: once the
    // dots are green nothing here changes again, and the engine names
    // (whisper.cpp, Ollama) are detail the user never chose. The card opens
    // Models, where all of it lives; the chevron says so.
    let (speech_ok, speech) = app.speech_ready();
    let (text_ok, text) = app.text_ready();
    let card = theme::soft(theme::CARD_DIM(), 14, 2.5, 3).inner_margin(Margin::same(12)).show(ui, |ui| {
        ui.set_width(186.0 - 24.0);
        ui.horizontal(|ui| {
            let all_ok = speech_ok && text_ok;
            let (r, _) = ui.allocate_exact_size(vec2(17.0, 17.0), Sense::hover());
            kit_icon_at(ui.painter(), if all_ok { "check" } else { "x" }, r.center(),
                        if all_ok { theme::GREEN() } else if speech_ok { theme::WARN() } else { theme::RED() }, 16.0);
            ui.add_space(3.0);
            ui.label(b(if speech_ok { "Ready" } else { "Not ready" }, 16.0, theme::TEXT()));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (r, _) = ui.allocate_exact_size(vec2(13.0, 13.0), Sense::hover());
                kit_icon(ui.painter(), "chev", r.center(), theme::FAINT());
            });
        });
        ui.add_space(4.0);
        // A tick is a stronger claim than a dot: it says set up, not present.
        // Speech is required, so a gap there is wrong; text clean-up is
        // optional, so a gap there has only been noticed.
        for (ok, name, missing) in [(speech_ok, speech, theme::RED()), (text_ok, text, theme::WARN())] {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(14.0, 18.0), Sense::hover());
                kit_icon_at(ui.painter(), if ok { "check" } else { "x" }, r.center(),
                            if ok { theme::GREEN() } else { missing }, 13.0);
                ui.add_space(2.0);
                ui.add(egui::Label::new(t(name, 13.0, theme::MUTED())).truncate());
            });
        }
    });
    let click = ui.interact(card.response.rect, ui.id().with("models"), Sense::click());
    if click.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        app.settings_tab = Some(SettingsTab::Models);
        app.refresh_health(ui.ctx());
    }
}

pub fn kbd(ui: &mut egui::Ui, k: &str) {
    theme::soft(theme::SURFACE(), 14, 2.5, 3)
        .corner_radius(5)
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| ui.label(b(pretty_key(k), 13.0, theme::TEXT())));
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

fn compact(v: f64) -> String {
    if v >= 1_000_000.0 {
        format!("{:.1}M", v / 1_000_000.0)
    } else if v >= 1000.0 {
        format!("{:.1}K", v / 1000.0)
    } else {
        format!("{v:.0}")
    }
}

fn duration_parts(minutes: f64) -> Vec<(String, &'static str)> {
    let total = minutes.round() as u64;
    let (h, mi) = (total / 60, total % 60);
    if h > 0 { vec![(h.to_string(), "hr"), (mi.to_string(), "min")] } else { vec![(mi.to_string(), "min")] }
}

fn duration_text(minutes: f64) -> String {
    duration_parts(minutes).iter().map(|(v, u)| format!("{v} {u}")).collect::<Vec<_>>().join(" ")
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

fn history(app: &mut DictateApp, ui: &mut egui::Ui) {
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
                if ui.button(t("Clear all history", 14.0, theme::RED())).clicked() {
                    app.clear_history();
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
                    app.set_keep_history(k);
                }
            });
        });
        ui.add_space(6.0);
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
        ui.painter().rect_filled(r, 0.0, theme::LINE());
        ui.add_space(6.0);
        ui.label(m(format!("{} Your data stays yours", crate::icon::LOCK), 14.0, theme::TEXT()));
        ui.label(para(
            "Recordings and text are kept only on this Mac. Your voice goes only to the speech and text models you chose in Models.",
            14.0,
            theme::MUTED(),
        ));
    });
    ui.add_space(14.0);
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
        Some((RowAction::Delete, w)) => app.delete_entry(w),
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

fn meetings(app: &mut DictateApp, ui: &mut egui::Ui, w: f32) {
    ui.horizontal(|ui| {
        ui.label(b("Meetings", 38.0, theme::TEXT()));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(t(
                format!("{} Kept only on this Mac", crate::icon::LOCK),
                14.0,
                theme::MUTED(),
            ));
        });
    });
    ui.add_space(16.0);

    // The heading stays; everything under it - a live transcript, an opened
    // meeting, or the list - is what moves. A transcript runs to hundreds of
    // lines, and scrolling it used to take the page's name with it.
    egui::ScrollArea::vertical().auto_shrink(false).id_salt("meetings_body").show(ui, |ui| {
        // While one is running, it is the only thing worth looking at.
        if let Some(session) = &app.meeting {
            let (elapsed, segments, warning) = session
                .live
                .lock()
                .map(|l| (l.elapsed, l.segments.clone(), l.warning.clone()))
                .unwrap_or((0.0, Vec::new(), None));
            let mut stop = false;
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
                transcript_line(ui, seg, w);
            }
            if stop {
                app.toggle_meeting();
            }
            return;
        }

        // One stored meeting, opened.
        if let Some(id) = app.open_meeting.clone() {
            let Some(meta) = app.open_meta.clone() else {
                app.open_meeting = None;
                return;
            };
            let mut delete = false;
            ui.horizontal(|ui| {
                if link(ui, "All meetings").clicked() {
                    app.open_meeting = None;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    delete = btn(ui, "Delete").clicked();
                });
            });
            ui.add_space(12.0);
            ui.label(b(&meta.title, 24.0, theme::TEXT()));
            if !meta.subtitle.is_empty() {
                ui.label(t(&meta.subtitle, 14.0, theme::MUTED()));
            }
            let length = meta.ended.map(|e| hhmmss((e - meta.started) as f32)).unwrap_or_else(|| "unfinished".into());
            ui.label(t(format!("{} · {} · {length}", crate::stats::stamp(meta.started), meta.source), 13.0, theme::MUTED()));
            ui.add_space(16.0);

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
                    ui.set_width(w - 36.0);
                    ui.label(b("Summary", 19.0, theme::TEXT()));
                    ui.add_space(6.0);
                    ui.label(para(&sum.overview, 14.0, theme::TEXT()));
                    for (title, items, empty) in [
                        ("Decided", &sum.decisions, "Nothing was settled."),
                        ("To do", &sum.actions, "Nobody took anything on."),
                        ("Still open", &sum.questions, "Nothing was left hanging."),
                    ] {
                        ui.add_space(14.0);
                        ui.label(m(title, 13.0, theme::MUTED()));
                        ui.add_space(4.0);
                        if items.is_empty() {
                            ui.label(t(empty, 13.0, theme::FAINT()));
                        } else {
                            let refs: Vec<&str> = items.iter().map(String::as_str).collect();
                            bullets(ui, &refs, 14.0, theme::TEXT());
                        }
                    }
                    ui.add_space(12.0);
                    ui.label(t(format!("Written by {}", sum.model), 12.0, theme::FAINT()));
                });
                ui.add_space(14.0);
            } else if !app.open_segments.is_empty() {
                if btn(ui, "Summarise").clicked() {
                    app.start_summary(id.clone());
                }
                ui.add_space(14.0);
            }

            let segs = app.open_segments.clone();
            if segs.is_empty() {
                let why = if meta.peak < 0.001 {
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
            for seg in &segs {
                transcript_line(ui, seg, w);
            }
            if delete {
                crate::meeting::delete(&id);
                app.open_meeting = None;
                app.open_segments.clear();
                app.open_meta = None;
                app.meetings_dirty = true;
                app.notify("Meeting deleted", false);
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
                let key = app.s.meeting_keys.first().cloned().unwrap_or_else(|| "F16".into());
                ui.label(para(
                    format!(
                        "Press {} to start one. Open Dictate records what your Mac is playing along with your microphone, so everyone on the call is in the transcript, and writes it down as the meeting runs.",
                        pretty_key(&key)
                    ),
                    14.0,
                    theme::MUTED(),
                ));
            });
            return;
        }
        for m in all {
            let resp = theme::ink_card().inner_margin(Margin::same(16)).show(ui, |ui| {
                ui.set_width(w - 32.0);
                ui.horizontal(|ui| {
                    ui.label(b(&m.title, 17.0, theme::TEXT()));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let length = m.ended.map(|e| hhmmss((e - m.started) as f32)).unwrap_or_else(|| "unfinished".into());
                        ui.label(t(length, 13.0, theme::MUTED()));
                    });
                });
                // The title says what it was about and no longer says when,
                // so the when goes here - in full, with the year, because
                // this line is now the only record of it on the page.
                if !m.subtitle.is_empty() {
                    ui.label(t(&m.subtitle, 13.0, theme::MUTED()));
                }
                ui.label(t(format!("{} · {}", crate::stats::stamp(m.started), m.source), 12.5, theme::FAINT()));
            });
            let click = ui.interact(resp.response.rect, ui.id().with(&m.id), Sense::click());
            if click.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                app.open_meeting(m.id.clone());
            }
            ui.add_space(10.0);
        }
        ui.add_space(28.0);
    });
}

fn transcript_line(ui: &mut egui::Ui, seg: &crate::meeting::Segment, w: f32) {
    ui.horizontal_top(|ui| {
        ui.add_space(2.0);
        let (r, _) = ui.allocate_exact_size(vec2(88.0, 20.0), Sense::hover());
        let p = ui.painter();
        p.text(
            pos2(r.left(), r.top()),
            Align2::LEFT_TOP,
            hhmmss(seg.at_ms as f32 / 1000.0),
            FontId::new(12.0, theme::medium()),
            theme::FAINT(),
        );
        // Who was talking, from which side of the call was louder.
        let (who, colour) = match seg.voice.as_str() {
            "you" => ("you", theme::GREEN()),
            "them" => ("them", theme::ACCENT()),
            "both" => ("both", theme::FAINT()),
            _ => ("", theme::FAINT()),
        };
        if !who.is_empty() {
            p.text(
                pos2(r.left() + 46.0, r.top()),
                Align2::LEFT_TOP,
                who,
                FontId::new(11.5, theme::medium()),
                colour,
            );
        }
        ui.add(egui::Label::new(para(&seg.text, 14.0, theme::TEXT())).wrap());
    });
    ui.add_space(10.0);
    let _ = w;
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
