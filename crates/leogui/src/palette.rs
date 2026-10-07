//! The command palette: every command by name, filtered as you type.

use eframe::egui::{self, Align2, Key, RichText};
use leoapp::app::App;

use crate::menus::{self, Entry};
use crate::style::Palette;

#[derive(Default)]
pub struct CommandPalette {
    pub open: bool,
    query: String,
    selected: usize,
    /// Focus the text field on the frame it opens.
    fresh: bool,
}

impl CommandPalette {
    pub fn show_palette(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.fresh = true;
    }

    /// Draw it, and run the command chosen.
    pub fn ui(&mut self, ctx: &egui::Context, app: &mut App, colours: &Palette) {
        if !self.open {
            return;
        }
        let mut matches: Vec<(i32, Entry)> = menus::entries()
            .into_iter()
            .filter_map(|e| {
                let s = menus::score(&self.query, e.name)
                    .max(menus::score(&self.query, e.summary).map(|s| s - 1000));
                s.map(|s| (s, e))
            })
            .collect();
        matches.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
        matches.truncate(200);
        self.selected = self.selected.min(matches.len().saturating_sub(1));

        let (up, down, enter, escape) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, Key::Enter),
                i.consume_key(egui::Modifiers::NONE, Key::Escape),
            )
        });
        if up {
            self.selected = self.selected.saturating_sub(1);
        }
        if down {
            self.selected = (self.selected + 1).min(matches.len().saturating_sub(1));
        }
        let mut chosen = enter.then_some(self.selected);

        let width = (ctx.content_rect().width() * 0.5).clamp(420.0, 720.0);
        egui::Area::new(egui::Id::new("palette"))
            .anchor(Align2::CENTER_TOP, [0.0, 60.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(width);
                    let edit = egui::TextEdit::singleline(&mut self.query)
                        .hint_text("Type a command")
                        .desired_width(f32::INFINITY);
                    let response = ui.add(edit);
                    if self.fresh {
                        response.request_focus();
                        self.fresh = false;
                    }
                    if response.changed() {
                        self.selected = 0;
                    }
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .max_height(360.0)
                        .show(ui, |ui| {
                            for (i, (_, e)) in matches.iter().enumerate() {
                                let row = ui.horizontal(|ui| {
                                    ui.set_width(width - 16.0);
                                    let name = RichText::new(e.name).strong();
                                    let r = ui.selectable_label(i == self.selected, name);
                                    ui.label(RichText::new(e.summary).color(colours.dim));
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(&e.keys)
                                                    .monospace()
                                                    .color(colours.dim),
                                            );
                                        },
                                    );
                                    r
                                });
                                if i == self.selected && (up || down) {
                                    row.response.scroll_to_me(None);
                                }
                                if row.inner.clicked() {
                                    chosen = Some(i);
                                }
                            }
                            if matches.is_empty() {
                                ui.label(RichText::new("no command matches").color(colours.dim));
                            }
                        });
                });
            });

        if escape {
            self.open = false;
        }
        if let Some(i) = chosen {
            self.open = false;
            if let Some((_, e)) = matches.get(i) {
                match menus::ASKS.contains(&e.name) {
                    true => menus::ask(app, &format!("{} ", e.name)),
                    false => {
                        app.run_chosen(e.name);
                    }
                }
            }
        }
    }
}
