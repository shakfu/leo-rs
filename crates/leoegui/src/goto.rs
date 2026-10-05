//! Go to node: every headline, filtered as you type, as an editor's quick
//! open. The palette's scoring ranks the matches.

use eframe::egui::{self, Align2, Key, RichText};
use leoapp::app::App;
use leolib::{Outline, Position};

use crate::menus;
use crate::style::Palette;

/// A node the picker offers: its position, headline and ancestors.
struct Entry {
    position: Position,
    headline: String,
    path: String,
}

#[derive(Default)]
pub struct GoToNode {
    pub open: bool,
    query: String,
    selected: usize,
    fresh: bool,
    /// Every node, built once per outline shape: 21,000 nodes take a walk.
    entries: Vec<Entry>,
    generation: Option<u64>,
}

/// One entry per node, clones once, in outline order.
fn entries(o: &Outline) -> Vec<Entry> {
    o.all_unique_positions()
        .into_iter()
        .map(|p| {
            let mut parents: Vec<&str> = p.parents(o).iter().map(|q| q.h(o)).collect();
            parents.reverse();
            Entry {
                headline: p.h(o).to_string(),
                path: parents.join(" > "),
                position: p,
            }
        })
        .collect()
}

/// The indices of `entries` that match `query`, best first, at most `most`.
/// A headline match outranks a match in the ancestors.
fn ranked(entries: &[Entry], query: &str, most: usize) -> Vec<usize> {
    let mut scored: Vec<(i32, usize)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let s = menus::score(query, &e.headline)
                .max(menus::score(query, &e.path).map(|s| s - 1000));
            s.map(|s| (s, i))
        })
        .collect();
    // Stable, so equal scores keep outline order.
    scored.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
    scored.truncate(most);
    scored.into_iter().map(|(_, i)| i).collect()
}

impl GoToNode {
    pub fn show_picker(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
        self.fresh = true;
    }

    /// Draw it, and select the node chosen.
    pub fn ui(&mut self, ctx: &egui::Context, app: &mut App, colours: &Palette) {
        if !self.open {
            return;
        }
        let o = app.outline();
        if self.generation != Some(o.generation) {
            self.entries = entries(o);
            self.generation = Some(o.generation);
        }
        let matches = ranked(&self.entries, &self.query, 200);
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
        egui::Area::new(egui::Id::new("go-to-node"))
            .anchor(Align2::CENTER_TOP, [0.0, 60.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_width(width);
                    let edit = egui::TextEdit::singleline(&mut self.query)
                        .hint_text("Go to node")
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
                            for (row, &i) in matches.iter().enumerate() {
                                let e = &self.entries[i];
                                let r = ui.horizontal(|ui| {
                                    ui.set_width(width - 16.0);
                                    let headline = match e.headline.is_empty() {
                                        true => "(untitled)",
                                        false => &e.headline,
                                    };
                                    let r = ui.selectable_label(
                                        row == self.selected,
                                        RichText::new(headline).strong(),
                                    );
                                    ui.label(RichText::new(&e.path).color(colours.dim));
                                    r
                                });
                                if row == self.selected && (up || down) {
                                    r.response.scroll_to_me(None);
                                }
                                if r.inner.clicked() {
                                    chosen = Some(row);
                                }
                            }
                            if matches.is_empty() {
                                ui.label(RichText::new("no node matches").color(colours.dim));
                            }
                        });
                });
            });

        if escape {
            self.open = false;
        }
        if let Some(row) = chosen {
            self.open = false;
            let found = matches.get(row).map(|&i| self.entries[i].position.clone());
            if let Some(p) = found {
                if app.run_chosen("") {
                    app.select(p);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outline() -> Outline {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "src");
        let a = o.insert_as_last_child(&root);
        o.set_headline(&a, "parse");
        let b = o.insert_as_last_child(&root);
        o.set_headline(&b, "print");
        let c = o.insert_as_last_child(&a);
        o.set_headline(&c, "tokens");
        o
    }

    #[test]
    fn every_node_is_offered_with_its_ancestors() {
        let all = entries(&outline());
        let names: Vec<_> = all.iter().map(|e| e.headline.as_str()).collect();
        assert_eq!(names, ["src", "parse", "tokens", "print"]);
        assert_eq!(all[2].path, "src > parse");
    }

    #[test]
    fn a_headline_match_outranks_an_ancestor_match() {
        let all = entries(&outline());
        let names = |q: &str| -> Vec<String> {
            ranked(&all, q, 10)
                .into_iter()
                .map(|i| all[i].headline.clone())
                .collect()
        };
        assert_eq!(names("pr"), ["print", "parse", "tokens"]);
        // `tokens` matches only through its ancestor `parse`.
        assert_eq!(names("parse"), ["parse", "tokens"]);
        assert_eq!(names("").len(), 4);
        assert!(names("zz").is_empty());
    }
}
