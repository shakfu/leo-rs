//! Tabs for the nodes opened, and the breadcrumb of the selected one.
//!
//! As in VS Code: selecting a node opens it in a preview tab, which the next
//! selection replaces. A node you edit, or open with a double click, keeps
//! its tab until it is closed.

use eframe::egui::{self, pos2, vec2, FontId, RichText, Sense, Stroke};
use leoapp::app::App;
use leolib::Position;

use crate::style::Palette;

pub struct Tab {
    gnx: String,
    pinned: bool,
}

#[derive(Default)]
pub struct Tabs {
    list: Vec<Tab>,
    /// Each tab's node, found again only when the outline's shape changes:
    /// a search is O(outline), and every frame draws every tab.
    found: std::collections::HashMap<String, Position>,
    generation: Option<u64>,
}

/// More tabs than this and the oldest kept tab goes; the preview stays.
const MOST: usize = 16;

impl Tabs {
    fn find(&self, app: &App, gnx: &str) -> Option<Position> {
        match app.current.gnx(app.outline()) == gnx {
            true => Some(app.current.clone()),
            false => self.found.get(gnx).cloned(),
        }
    }

    /// Find every tab's node in one walk, if the outline changed shape.
    fn refresh(&mut self, app: &App) {
        let o = app.outline();
        if self.generation == Some(o.generation) {
            return;
        }
        self.generation = Some(o.generation);
        let wanted: std::collections::HashSet<&str> =
            self.list.iter().map(|t| t.gnx.as_str()).collect();
        self.found.clear();
        for p in o.all_unique_positions() {
            let gnx = p.gnx(o);
            if wanted.contains(gnx) && !self.found.contains_key(gnx) {
                self.found.insert(gnx.to_string(), p);
            }
        }
    }

    /// Open the selection in a tab, and pin a tab whose node was edited.
    pub fn track(&mut self, app: &App) {
        self.refresh(app);
        let o = app.outline();
        let gnx = app.current.gnx(o).to_string();
        // The selection is where a later frame looks for its tab.
        self.found.insert(gnx.clone(), app.current.clone());
        let dirty = app.current.is_dirty(o) || app.buffer.is_some();
        let found = &self.found;
        self.list
            .retain(|t| t.gnx == gnx || found.contains_key(&t.gnx));
        match self.list.iter_mut().find(|t| t.gnx == gnx) {
            Some(tab) => tab.pinned |= dirty,
            None => {
                let tab = Tab { gnx, pinned: dirty };
                match self.list.iter().position(|t| !t.pinned) {
                    Some(i) => self.list[i] = tab,
                    None => self.list.push(tab),
                }
            }
        }
        while self.list.len() > MOST {
            match self.list.iter().position(|t| t.pinned) {
                Some(i) => self.list.remove(i),
                None => self.list.remove(0),
            };
        }
    }

    pub fn pin(&mut self, gnx: &str) {
        if let Some(t) = self.list.iter_mut().find(|t| t.gnx == gnx) {
            t.pinned = true;
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, app: &mut App, colours: &Palette) {
        let current = app.current.gnx(app.outline()).to_string();
        let mut select: Option<String> = None;
        let mut close: Option<usize> = None;
        egui::ScrollArea::horizontal()
            .id_salt("tabs")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for (i, tab) in self.list.iter().enumerate() {
                        let Some(p) = self.find(app, &tab.gnx) else {
                            continue;
                        };
                        let o = app.outline();
                        let mut title: String = p.h(o).chars().take(28).collect();
                        if title.is_empty() {
                            title = "(untitled)".into();
                        }
                        let active = tab.gnx == current;
                        let mut text = RichText::new(title).size(13.5);
                        if !tab.pinned {
                            text = text.italics();
                        }
                        text = text.color(if active {
                            colours.tab_active.1
                        } else {
                            colours.tab.1
                        });
                        let galley = ui.painter().layout_no_wrap(
                            text.text().to_string(),
                            FontId::proportional(13.5),
                            colours.fg,
                        );
                        let size = vec2(galley.size().x + 44.0, 30.0);
                        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
                        let painter = ui.painter_at(rect);
                        let fill = if active {
                            colours.tab_active.0
                        } else {
                            colours.tab.0
                        };
                        painter.rect_filled(rect, 0.0, fill);
                        if active {
                            painter.hline(
                                rect.x_range(),
                                rect.top() + 1.0,
                                Stroke::new(2.0, colours.accent),
                            );
                        }
                        painter.vline(
                            rect.right(),
                            rect.y_range(),
                            Stroke::new(1.0, colours.border),
                        );
                        let label = egui::WidgetText::from(text).into_galley(
                            ui,
                            Some(egui::TextWrapMode::Extend),
                            f32::INFINITY,
                            egui::TextStyle::Body,
                        );
                        painter.galley(
                            pos2(rect.left() + 12.0, rect.center().y - label.size().y / 2.0),
                            label,
                            colours.fg,
                        );

                        // The close button, or a dot for unsaved changes.
                        let x = rect.right() - 16.0;
                        let c = pos2(x, rect.center().y);
                        let button = egui::Rect::from_center_size(c, vec2(16.0, 16.0));
                        let over = response.hover_pos().is_some_and(|p| button.contains(p));
                        if response.hovered() || active {
                            let colour = if over { colours.fg } else { colours.dim };
                            let s = Stroke::new(1.3, colour);
                            painter.line_segment([c + vec2(-3.5, -3.5), c + vec2(3.5, 3.5)], s);
                            painter.line_segment([c + vec2(-3.5, 3.5), c + vec2(3.5, -3.5)], s);
                        } else if p.is_dirty(o) {
                            painter.circle_filled(c, 3.5, colours.dim);
                        }
                        if response.clicked() {
                            match over {
                                true => close = Some(i),
                                false => select = Some(tab.gnx.clone()),
                            }
                        }
                        if response.middle_clicked() {
                            close = Some(i);
                        }
                    }
                });
            });
        if let Some(i) = close {
            let closed = self.list.remove(i);
            if closed.gnx == current {
                // The neighbour takes its place, as in an editor.
                if let Some(next) = self.list.get(i.min(self.list.len().saturating_sub(1))) {
                    select = Some(next.gnx.clone());
                }
            }
        }
        if let Some(gnx) = select {
            if let Some(p) = self.find(app, &gnx) {
                if app.run_chosen("") {
                    app.select(p);
                }
            }
        }
    }
}

/// The selection's ancestors, each a link to select it.
pub fn breadcrumb(ui: &mut egui::Ui, app: &mut App, colours: &Palette) {
    let o = app.outline();
    let mut path: Vec<Position> = app.current.parents(o);
    path.reverse();
    path.push(app.current.clone());
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (i, p) in path.iter().enumerate() {
            if i > 0 {
                ui.label(RichText::new(">").color(colours.dim).size(12.5));
            }
            let last = i + 1 == path.len();
            let text = RichText::new(p.h(app.outline())).size(12.5).color(if last {
                colours.fg
            } else {
                colours.dim
            });
            let link = ui.add(egui::Label::new(text).sense(Sense::click()));
            if link.hovered() && !last {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if link.clicked() && !last {
                chosen = Some(p.clone());
            }
        }
    });
    if let Some(p) = chosen {
        if app.run_chosen("") {
            app.select(p);
        }
    }
}
