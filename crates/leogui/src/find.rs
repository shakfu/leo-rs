//! The find panel, Leo's Find tab: find and replace text, its options and
//! scope, and the matches as a list to click through.

use eframe::egui::{self, Key, RichText};
use leoapp::app::App;
use leoapp::search::{Find, FindScope, Found};

use crate::style::Palette;

#[derive(Default)]
pub struct FindPanel {
    find: Find,
    replace: String,
    found: Vec<Found>,
    /// The outline generation `found` was made at.
    generation: Option<u64>,
    /// Whether a text field of the panel has the keys.
    pub editing: bool,
    /// Focus the find field on the next draw.
    fresh: bool,
}

impl FindPanel {
    /// Drop the matches: they are positions in an outline no longer shown.
    pub fn forget(&mut self) {
        self.found.clear();
        self.generation = None;
    }

    /// Put the keys in the find field when the panel is next drawn.
    pub fn focus(&mut self) {
        self.fresh = true;
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, app: &mut App, colours: &Palette) {
        let mut go = None;
        let mut act: Option<fn(&mut Self, &mut App)> = None;
        let field = |ui: &mut egui::Ui, text: &mut String, hint: &str| {
            ui.add(
                egui::TextEdit::singleline(text)
                    .hint_text(hint)
                    .desired_width(260.0),
            )
        };
        ui.horizontal(|ui| {
            let find = field(ui, &mut self.find.pattern, "Find");
            let replace = field(ui, &mut self.replace, "Replace");
            if self.fresh {
                find.request_focus();
                self.fresh = false;
            }
            let enter = ui.input(|i| i.key_pressed(Key::Enter));
            if find.lost_focus() && enter {
                act = Some(Self::find_all);
            }
            self.editing = find.has_focus() || replace.has_focus();
            ui.separator();
            if ui.button("Find All").clicked() {
                act = Some(Self::find_all);
            }
            if ui.button("Replace All").clicked() {
                act = Some(|panel, app| {
                    app.replace_all(&panel.find, &panel.replace);
                    panel.found.clear();
                });
            }
            if ui
                .button("Clone Find All")
                .on_hover_text("Clone the matching nodes under a new Found node")
                .clicked()
            {
                act = Some(|panel, app| app.clone_find(&panel.find, false));
            }
            if ui
                .button("Flattened")
                .on_hover_text("Clone Find All, searching below each match too")
                .clicked()
            {
                act = Some(|panel, app| app.clone_find(&panel.find, true));
            }
        });
        ui.horizontal(|ui| {
            let f = &mut self.find;
            ui.checkbox(&mut f.regex, "Regex");
            ui.checkbox(&mut f.whole_word, "Whole word");
            ui.checkbox(&mut f.ignore_case, "Ignore case");
            ui.checkbox(&mut f.headlines, "Headlines");
            ui.checkbox(&mut f.bodies, "Bodies");
            ui.separator();
            ui.radio_value(&mut f.scope, FindScope::Outline, "Outline");
            ui.radio_value(&mut f.scope, FindScope::Subtree, "Subtree");
            ui.radio_value(&mut f.scope, FindScope::Marked, "Marked");
        });
        ui.separator();

        let stale = self.generation != Some(app.outline().generation);
        if stale && !self.found.is_empty() {
            ui.label(
                RichText::new("The outline changed since; Find All again.")
                    .size(12.0)
                    .color(colours.warning),
            );
        }
        egui::ScrollArea::vertical()
            .id_salt("found")
            .auto_shrink([false, false])
            .show_rows(ui, 20.0, self.found.len(), |ui, rows| {
                for i in rows {
                    let f = &self.found[i];
                    let o = app.outline();
                    let headline = match o.position_exists(&f.node) {
                        true => f.node.h(o).to_string(),
                        false => "(gone)".to_string(),
                    };
                    let place = match f.place {
                        leoapp::search::Place::Headline(_) => "headline".to_string(),
                        leoapp::search::Place::Body(row, _) => format!("Ln {}", row + 1),
                    };
                    let row = ui.horizontal(|ui| {
                        ui.set_height(20.0);
                        ui.label(RichText::new(headline).strong().size(12.5));
                        ui.label(RichText::new(place).size(12.0).color(colours.dim));
                        ui.label(line(&f.line, app, colours));
                    });
                    let click =
                        ui.interact(row.response.rect, ui.id().with(i), egui::Sense::click());
                    if click.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if click.clicked() {
                        go = Some(i);
                    }
                }
                if self.found.is_empty() {
                    ui.label(RichText::new("No matches listed.").color(colours.dim));
                }
            });

        if let Some(act) = act {
            if app.run_chosen("") {
                act(self, app);
                app.log_message();
            }
        }
        if let Some(i) = go {
            let found = self.found[i].clone();
            if app.run_chosen("") {
                app.go_to_found(&found);
            }
        }
    }

    fn find_all(&mut self, app: &mut App) {
        if let Some(found) = app.find_all(&self.find) {
            self.found = found;
            self.generation = Some(app.outline().generation);
        }
    }
}

/// A matched line, trimmed, its matches lit.
fn line(text: &str, app: &App, colours: &Palette) -> egui::text::LayoutJob {
    let text = text.trim();
    let font = egui::FontId::monospace(12.5);
    let mut job = egui::text::LayoutJob::default();
    let hits = app
        .hlsearch
        .as_ref()
        .map_or_else(Vec::new, |re| leoapp::search::ranges(re, text));
    for (piece, hit) in leoapp::view::cut(text, &hits) {
        let background = match hit {
            true => crate::style::ansi(3).gamma_multiply(0.6),
            false => egui::Color32::TRANSPARENT,
        };
        let format = egui::TextFormat {
            font_id: font.clone(),
            color: colours.fg,
            background,
            ..Default::default()
        };
        job.append(piece, 0.0, format);
    }
    job
}
