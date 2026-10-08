//! What leoapp asks for or shows in a mode of its own: the `:` and `/`
//! lines, a yes/no question, the key bindings, and overlays such as a hover.
//! The keys still go to the app; these only draw what it holds.

use eframe::egui::{self, Align2, FontId, Pos2, RichText, Stroke};
use leoapp::app::{App, Focus, Mode};

use crate::style::Palette;

#[derive(Default)]
pub struct Prompts {
    help_pane: Option<Focus>,
}

impl Prompts {
    /// Draw whichever the app's mode needs. `hover_at` is the body cursor,
    /// where a hover opens.
    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        app: &mut App,
        colours: &Palette,
        hover_at: Option<Pos2>,
    ) {
        match app.mode {
            Mode::Command | Mode::Search => quick_input(ctx, app, colours),
            Mode::Confirm => confirm(ctx, app),
            Mode::Help => self.help(ctx, app, colours, hover_at),
            Mode::Insert if app.completion.is_some() => completions(ctx, app, colours, hover_at),
            Mode::Insert if app.signature.is_some() => signature(ctx, app, colours, hover_at),
            _ => self.help_pane = None,
        }
    }

    fn help(
        &mut self,
        ctx: &egui::Context,
        app: &mut App,
        colours: &Palette,
        hover_at: Option<Pos2>,
    ) {
        let mut open = true;
        match app.overlay.clone() {
            Some((name, _)) if name == leoapp::app::CODE_ACTIONS => {
                code_actions(ctx, app, colours, hover_at);
                return;
            }
            Some((name, lines)) if name == "hover" => {
                let at = hover_at.unwrap_or(ctx.content_rect().center());
                egui::Area::new(egui::Id::new("hover"))
                    .fixed_pos(at + egui::vec2(0.0, 22.0))
                    .order(egui::Order::Foreground)
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style())
                            .fill(colours.popup)
                            .show(ui, |ui| {
                                ui.set_max_width(560.0);
                                egui::ScrollArea::vertical()
                                    .max_height(320.0)
                                    .show(ui, |ui| {
                                        for line in &lines {
                                            ui.label(RichText::new(line).monospace());
                                        }
                                    });
                                ui.label(
                                    RichText::new("q or Escape closes")
                                        .size(11.5)
                                        .color(colours.dim),
                                );
                            });
                    });
            }
            Some((name, lines)) => {
                egui::Window::new(name)
                    .open(&mut open)
                    .collapsible(false)
                    .default_size([620.0, 420.0])
                    .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                for line in &lines {
                                    ui.label(RichText::new(line).monospace());
                                }
                            });
                    });
            }
            None => {
                let pane = self.help_pane.get_or_insert(app.focus);
                egui::Window::new("Key Bindings")
                    .frame(egui::Frame::window(&ctx.global_style()).fill(colours.help))
                    .open(&mut open)
                    .collapsible(false)
                    .default_size([760.0, 520.0])
                    .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.selectable_value(pane, Focus::Tree, "Outline");
                            ui.selectable_value(pane, Focus::Body, "Body");
                        });
                        ui.separator();
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                egui::Grid::new("bindings")
                                    .striped(true)
                                    .spacing([18.0, 6.0])
                                    .show(ui, |ui| {
                                        for (keys, command, summary) in
                                            leoapp::view::help_entries(*pane)
                                        {
                                            ui.label(
                                                RichText::new(keys.join("  "))
                                                    .monospace()
                                                    .color(colours.accent),
                                            );
                                            ui.label(RichText::new(command).monospace());
                                            ui.label(RichText::new(summary).color(colours.dim));
                                            ui.end_row();
                                        }
                                    });
                            });
                    });
            }
        }
        if !open {
            app.run("close-help", 1);
        }
    }
}

/// The completions on offer, as a menu under the body cursor. The keys go
/// to the app; a click accepts one.
/// The signature of the call being typed, above the cursor, its active
/// parameter in bold.
fn signature(ctx: &egui::Context, app: &App, colours: &Palette, at: Option<Pos2>) {
    let Some(sig) = &app.signature else { return };
    let at = at.unwrap_or(ctx.content_rect().center());
    egui::Area::new(egui::Id::new("signature"))
        .fixed_pos(at - egui::vec2(0.0, 30.0))
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(colours.popup)
                .show(ui, |ui| {
                    ui.set_max_width(640.0);
                    let range = sig.active.clone().unwrap_or(0..0);
                    let mut job = egui::text::LayoutJob::default();
                    let font = egui::FontId::monospace(13.0);
                    for (text, strong) in [
                        (&sig.label[..range.start], false),
                        (&sig.label[range.clone()], true),
                        (&sig.label[range.end..], false),
                    ] {
                        let colour = if strong { colours.accent } else { colours.fg };
                        job.append(text, 0.0, egui::TextFormat::simple(font.clone(), colour));
                    }
                    ui.label(job);
                    if let Some(doc) = &sig.documentation {
                        let first: String =
                            doc.lines().next().unwrap_or("").chars().take(120).collect();
                        ui.label(RichText::new(first).size(12.0).color(colours.dim));
                    }
                });
        });
}

fn completions(ctx: &egui::Context, app: &mut App, colours: &Palette, at: Option<Pos2>) {
    let Some(menu) = &app.completion else { return };
    let at = at.unwrap_or(ctx.content_rect().center());
    let mut chosen = None;
    egui::Area::new(egui::Id::new("completions"))
        .fixed_pos(at + egui::vec2(0.0, 20.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(colours.popup)
                .show(ui, |ui| {
                    ui.set_min_width(260.0);
                    ui.set_max_width(520.0);
                    egui::ScrollArea::vertical()
                        .max_height(260.0)
                        .show(ui, |ui| {
                            for i in 0..menu.shown.len() {
                                let Some(c) = menu.shown_item(i) else { break };
                                let selected = i == menu.selected;
                                let row = ui.horizontal(|ui| {
                                    let label = RichText::new(&c.label).monospace();
                                    let r = ui.selectable_label(selected, label);
                                    if let Some(detail) = &c.detail {
                                        let detail: String = detail.chars().take(60).collect();
                                        ui.label(
                                            RichText::new(detail).size(12.0).color(colours.dim),
                                        );
                                    }
                                    r
                                });
                                if selected {
                                    row.response.scroll_to_me(None);
                                }
                                let r = match &c.kind {
                                    Some(kind) => row.inner.on_hover_text(kind),
                                    None => row.inner,
                                };
                                if r.clicked() {
                                    chosen = Some(i);
                                }
                            }
                        });
                });
        });
    if let Some(i) = chosen {
        app.accept_completion(i);
    }
}

/// The code actions offered, as a menu at the body cursor. The arrows or
/// j/k select, Enter, a digit or a click applies; q or Escape closes it.
fn code_actions(ctx: &egui::Context, app: &mut App, colours: &Palette, at: Option<Pos2>) {
    let at = at.unwrap_or(ctx.content_rect().center());
    let mut chosen = None;
    egui::Area::new(egui::Id::new("code-actions"))
        .fixed_pos(at + egui::vec2(0.0, 22.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(colours.popup)
                .show(ui, |ui| {
                    ui.set_max_width(560.0);
                    egui::ScrollArea::vertical()
                        .max_height(320.0)
                        .show(ui, |ui| {
                            for (i, a) in app.code_actions.iter().enumerate() {
                                let mut text = RichText::new(format!("{}  {}", i + 1, a.title));
                                if a.preferred {
                                    text = text.strong();
                                }
                                let selected = i == app.code_action_selected;
                                let row = ui.selectable_label(selected, text);
                                if selected {
                                    row.scroll_to_me(None);
                                }
                                let row = match &a.kind {
                                    Some(kind) => row.on_hover_text(kind),
                                    None => row,
                                };
                                if row.clicked() {
                                    chosen = Some(i);
                                }
                            }
                        });
                    ui.label(
                        RichText::new("Up/Down selects, Enter or 1-9 applies; q or Escape closes")
                            .size(11.5)
                            .color(colours.dim),
                    );
                });
        });
    if let Some(i) = chosen {
        app.run("close-help", 1);
        app.apply_code_action(i);
        app.log_message();
    }
}

/// The `:` or `/` line: a box at the top, its text the app's.
fn quick_input(ctx: &egui::Context, app: &App, colours: &Palette) {
    let Some(mini) = &app.mini else { return };
    let label = app.mini_label();
    let width = (ctx.content_rect().width() * 0.5).clamp(420.0, 720.0);
    egui::Area::new(egui::Id::new("quick-input"))
        .anchor(Align2::CENTER_TOP, [0.0, 60.0])
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(width);
                let font = FontId::monospace(14.0);
                let text = format!("{label}{}", mini.buffer);
                let galley = ui.painter().layout_no_wrap(text, font.clone(), colours.fg);
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(width, 26.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 3.0, colours.bg);
                ui.painter().rect_stroke(
                    rect,
                    3.0,
                    Stroke::new(1.0, colours.accent),
                    egui::StrokeKind::Inside,
                );
                let at = rect.left_center() + egui::vec2(8.0, -galley.size().y / 2.0);
                let before: String = label
                    .chars()
                    .chain(mini.buffer.chars().take(mini.cursor))
                    .collect();
                let x = ui
                    .painter()
                    .layout_no_wrap(before, font, colours.fg)
                    .size()
                    .x;
                ui.painter().galley(at, galley, colours.fg);
                ui.painter().vline(
                    at.x + x,
                    rect.shrink(5.0).y_range(),
                    Stroke::new(1.5, colours.fg),
                );

                let menu = mini.menu(&app.theme_names);
                if menu.is_open() {
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .max_height(300.0)
                        .show(ui, |ui| {
                            for (i, item) in menu.items.iter().enumerate() {
                                let r =
                                    ui.selectable_label(menu.selected == Some(i), item.as_str());
                                if menu.selected == Some(i) {
                                    r.scroll_to_me(None);
                                }
                            }
                        });
                } else if app.mode == Mode::Command {
                    let word = mini.buffer.split_whitespace().next().unwrap_or("");
                    if !word.is_empty() && !mini.buffer.contains(' ') {
                        let hints: Vec<_> = leoapp::commands::all()
                            .filter(|c| c.name.starts_with(word))
                            .take(8)
                            .collect();
                        if !hints.is_empty() {
                            ui.separator();
                            for c in hints {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(c.name).monospace());
                                    ui.label(RichText::new(c.summary).color(colours.dim));
                                });
                            }
                            ui.label(RichText::new("Tab completes").size(11.5).color(colours.dim));
                        }
                    }
                }
            });
        });
}

/// A yes/no question as a dialog. Its buttons press the keys it waits for.
fn confirm(ctx: &egui::Context, app: &mut App) {
    let quitting = app
        .mini
        .as_ref()
        .is_some_and(|m| m.kind == leoapp::minibuffer::MiniKind::ConfirmQuit);
    // The app asks as a terminal would, "... (y/n)"; a dialog has buttons.
    let question = match quitting {
        true => format!("{}. Save before quitting?", app.unsaved_work()),
        false => app
            .mini_label()
            .trim()
            .trim_end_matches("(y/n)")
            .trim_end()
            .to_string(),
    };
    let mut answer = None;
    egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
        ui.set_max_width(460.0);
        ui.label(RichText::new(question).size(14.5));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if quitting {
                if ui.button("Save (s)").clicked() {
                    answer = Some('s');
                }
                if ui.button("Quit without saving (y)").clicked() {
                    answer = Some('y');
                }
                if ui.button("Cancel (n)").clicked() {
                    answer = Some('n');
                }
                return;
            }
            if ui.button("Yes (y)").clicked() {
                answer = Some('y');
            }
            if ui.button("No (n)").clicked() {
                answer = Some('n');
            }
        });
    });
    if let Some(key) = answer {
        app.answer_key(key);
        app.log_message();
    }
}

/// The About box.
pub fn about(ctx: &egui::Context, open: &mut bool) {
    if !*open {
        return;
    }
    let response = egui::Modal::new(egui::Id::new("about")).show(ctx, |ui| {
        ui.set_max_width(420.0);
        ui.heading("leogui");
        ui.label(format!("version {}", env!("CARGO_PKG_VERSION")));
        ui.add_space(6.0);
        ui.label("A desktop front end for leolib, a Rust port of Leo's outline model.");
        ui.add_space(10.0);
        if ui.button("Close").clicked() {
            ui.close();
        }
    });
    if response.should_close() {
        *open = false;
    }
}
