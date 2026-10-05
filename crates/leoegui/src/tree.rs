//! The outline sidebar: fold arrows, node icons, a selection that follows
//! the keys, a context menu, inline headline editing, and drag and drop.

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CursorIcon, FontId, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind,
};
use leoapp::app::{App, FileState, Focus, Mode};
use leoapp::view::Severity;
use leolib::{Place, Position};

use crate::style::Palette;
use crate::tabs::Tabs;

const ROW: f32 = 22.0;
const INDENT: f32 = 16.0;

#[derive(Default)]
pub struct Tree {
    /// The node being dragged, from the press that started it.
    drag: Option<Position>,
    /// The selection last drawn, to scroll to a selection the keys moved.
    last: Option<Position>,
    offset: f32,
}

/// What the context menu runs on the clicked node.
const CONTEXT: &[(&str, &str)] = &[
    ("Insert Node", "insert-node"),
    ("Insert Child", "insert-child"),
    ("Rename", "edit-headline"),
    ("Clone", "clone-node"),
    ("", ""),
    ("Cut", "cut-node"),
    ("Copy", "copy-node"),
    ("Paste", "paste-node"),
    ("Delete", "delete-node"),
    ("", ""),
    ("Mark", "mark"),
    ("Move Up", "move-outline-up"),
    ("Move Down", "move-outline-down"),
    ("Promote", "promote"),
    ("Demote", "demote"),
    ("", ""),
    ("Hoist", "hoist"),
];

/// Where a drop at `y` on a row lands: its top quarter before it, its bottom
/// quarter after it, the middle inside it.
fn place_at(rect: Rect, y: f32) -> Place {
    let t = (y - rect.top()) / rect.height();
    if t < 0.25 {
        Place::Before
    } else if t > 0.75 {
        Place::After
    } else {
        Place::Inside
    }
}

impl Tree {
    pub fn ui(&mut self, ui: &mut egui::Ui, app: &mut App, colours: &Palette, tabs: &mut Tabs) {
        // Only the rows on screen are built; a big outline has thousands.
        let (total, current) = (app.row_count(), app.current_row());
        let height = ui.available_height();
        app.tree_height = ((height / ROW) as usize).max(1);

        // A selection the keys moved off screen is brought back into view.
        let moved = self.last.as_ref() != Some(&app.current);
        let mut area = egui::ScrollArea::vertical()
            .id_salt("outline")
            .auto_shrink([false, false]);
        if moved {
            let y = current as f32 * (ROW + ui.spacing().item_spacing.y);
            if y < self.offset || y + ROW > self.offset + height {
                area = area.vertical_scroll_offset((y - height / 2.0).max(0.0));
            }
        }
        self.last = Some(app.current.clone());

        let focused = app.focus == Focus::Tree;
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let released = ui.input(|i| i.pointer.any_released());
        let mut drop: Option<(Position, Place)> = None;
        let mut run: Option<(&str, Position)> = None;
        let mut fold: Option<Position> = None;
        let mut click: Option<(Position, bool)> = None;
        let mut goto: Option<Position> = None;

        let output = area.show_rows(ui, ROW, total, |ui, range| {
            let first = range.start;
            let rows = app.rows_in(range.clone());
            for i in range {
                let Some(row) = rows.get(i - first) else {
                    break;
                };
                let (rect, response) = ui
                    .allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click_and_drag());
                let painter = ui.painter_at(rect);
                let selected = i == current;
                if selected {
                    let fill = match focused {
                        true => colours.selection,
                        false => colours.selection.gamma_multiply(0.55),
                    };
                    painter.rect_filled(rect, 3.0, fill);
                } else if response.hovered() {
                    painter.rect_filled(rect, 3.0, colours.line);
                }

                let left = rect.left() + 6.0;
                for d in 0..row.depth {
                    let x = left + d as f32 * INDENT + 6.0;
                    painter.vline(x, rect.y_range(), Stroke::new(1.0, colours.guide));
                }
                let x = left + row.depth as f32 * INDENT;
                let arrow = Rect::from_min_size(pos2(x, rect.top()), vec2(14.0, ROW));
                if row.has_children {
                    triangle(&painter, arrow.center(), row.expanded, colours.dim);
                }
                let icon =
                    Rect::from_center_size(pos2(x + 21.0, rect.center().y), vec2(11.0, 11.0));
                node_icon(&painter, icon, row, colours);

                // The headline, or the line editing it.
                let text_x = x + 32.0;
                let editing = selected && app.mode == Mode::Headline;
                if editing {
                    self.paint_editing(&painter, app, rect, text_x, colours);
                } else {
                    paint_headline(&painter, row, text_x, rect, colours, app);
                }

                // Flags at the right: the file's state or changed since the
                // save, then marked.
                let mut fx = rect.right() - 10.0;
                if let Some(state) = row.file_state {
                    fx = file_badge(&painter, state, fx, rect.center().y, colours) - 6.0;
                } else if row.dirty {
                    painter.circle_filled(pos2(fx, rect.center().y), 3.0, colours.dim);
                    fx -= 12.0;
                }
                if row.cloned {
                    let count = painter.text(
                        pos2(fx, rect.center().y),
                        Align2::RIGHT_CENTER,
                        row.clones.to_string(),
                        FontId::proportional(11.0),
                        colours.dim,
                    );
                    fx = count.left() - 6.0;
                }
                if row.marked {
                    let c = pos2(fx, rect.center().y);
                    painter.add(Shape::convex_polygon(
                        vec![
                            c + vec2(-3.0, -5.0),
                            c + vec2(3.0, -5.0),
                            c + vec2(3.0, 5.0),
                            c + vec2(0.0, 2.0),
                            c + vec2(-3.0, 5.0),
                        ],
                        colours.warning,
                        Stroke::NONE,
                    ));
                }

                // Where a dragged node would land.
                if let (Some(from), Some(p)) = (&self.drag, pointer) {
                    if rect.contains(p) && *from != row.position {
                        let place = place_at(rect, p.y);
                        let stroke = Stroke::new(2.0, colours.accent);
                        match place {
                            Place::Before => {
                                painter.hline(rect.x_range(), rect.top() + 1.0, stroke);
                            }
                            Place::After => {
                                painter.hline(rect.x_range(), rect.bottom() - 1.0, stroke);
                            }
                            Place::Inside => {
                                painter.rect_stroke(
                                    rect.shrink(1.0),
                                    3.0,
                                    stroke,
                                    StrokeKind::Inside,
                                );
                            }
                        }
                        if released {
                            drop = Some((row.position.clone(), place));
                        }
                    }
                }

                let mut tip = Vec::new();
                if let Some(state) = row.file_state {
                    tip.push(file_tip(app, &row.position, state));
                }
                if row.cloned {
                    tip.push(format!(
                        "In {} places. ]c or Alt-N goes to the next; the menu lists them.",
                        row.clones
                    ));
                }
                let response = match tip.is_empty() {
                    true => response,
                    false => response.on_hover_text(tip.join("\n\n")),
                };
                if response.drag_started() && app.mode == Mode::Normal {
                    self.drag = Some(row.position.clone());
                }
                let on_arrow = response
                    .interact_pointer_pos()
                    .is_some_and(|p| arrow.contains(p) && row.has_children);
                if response.clicked() && on_arrow {
                    fold = Some(row.position.clone());
                } else if response.double_clicked() {
                    click = Some((row.position.clone(), true));
                } else if response.clicked() || response.secondary_clicked() {
                    click = Some((row.position.clone(), false));
                }
                response.context_menu(|ui| {
                    for (label, command) in CONTEXT {
                        if label.is_empty() {
                            ui.separator();
                        } else if ui.button(*label).clicked() {
                            run = Some((command, row.position.clone()));
                            ui.close();
                        }
                    }
                    if row.cloned {
                        ui.separator();
                        ui.menu_button(format!("Clones ({})", row.clones), |ui| {
                            for p in app.clones_of(&row.position) {
                                let o = app.outline();
                                let mut path: Vec<&str> =
                                    p.self_and_parents(o).iter().map(|q| q.h(o)).collect();
                                path.reverse();
                                let here = p == row.position;
                                let label = egui::RichText::new(path.join(" > "));
                                if ui.selectable_label(here, label).clicked() {
                                    goto = Some(p);
                                    ui.close();
                                }
                            }
                        });
                    }
                });
            }
        });
        self.offset = output.state.offset.y;

        if self.drag.is_some() {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        }
        if released {
            if let (Some(from), Some((onto, place))) = (self.drag.take(), drop) {
                app.drop_node(&from, &onto, place);
            }
            self.drag = None;
        }
        if let Some(p) = fold {
            app.toggle_fold(&p);
        }
        if let Some((p, double)) = click {
            if app.run_chosen("") {
                app.click_node(&p);
                if double {
                    tabs.pin(p.gnx(app.outline()));
                    app.focus = Focus::Body;
                }
            }
        }
        if let Some(p) = goto {
            if app.run_chosen("") {
                app.select(p);
            }
        }
        if let Some((command, p)) = run {
            if app.run_chosen("") {
                app.click_node(&p);
                app.run_chosen(command);
            }
        }
    }

    /// The headline being edited: a text field drawn over the row, its text
    /// and cursor the app's, since the keys go to the app.
    fn paint_editing(
        &self,
        painter: &egui::Painter,
        app: &App,
        rect: Rect,
        x: f32,
        colours: &Palette,
    ) {
        let Some(mini) = &app.mini else { return };
        let field = Rect::from_min_max(
            pos2(x - 4.0, rect.top() + 1.0),
            pos2(rect.right() - 4.0, rect.bottom() - 1.0),
        );
        painter.rect_filled(field, 3.0, colours.bg);
        painter.rect_stroke(
            field,
            3.0,
            Stroke::new(1.0, colours.accent),
            StrokeKind::Inside,
        );
        let font = FontId::proportional(14.0);
        let before: String = mini.buffer.chars().take(mini.cursor).collect();
        let at = painter
            .layout_no_wrap(before, font.clone(), colours.fg)
            .size()
            .x;
        painter.text(
            pos2(x, rect.center().y),
            Align2::LEFT_CENTER,
            &mini.buffer,
            font,
            colours.fg,
        );
        painter.vline(
            x + at,
            field.shrink(3.0).y_range(),
            Stroke::new(1.5, colours.fg),
        );
    }
}

/// The word a file state is shown as, and its colour.
fn file_word(state: FileState, colours: &Palette) -> (&'static str, Color32) {
    match state {
        FileState::Unread => ("unread", colours.mark(Severity::Error).gutter),
        FileState::ChangedOnDisk => ("changed on disk", colours.warning),
        FileState::Refused => ("not read", colours.warning),
        FileState::Unwritten => ("unwritten", colours.dim),
    }
}

/// A file state's word in a box, its right edge at `right`. Returns the
/// box's left edge.
fn file_badge(
    painter: &egui::Painter,
    state: FileState,
    right: f32,
    y: f32,
    colours: &Palette,
) -> f32 {
    let (word, colour) = file_word(state, colours);
    let galley = painter.layout_no_wrap(word.to_string(), FontId::proportional(11.0), colour);
    let size = galley.size() + vec2(8.0, 2.0);
    let r = Rect::from_min_size(pos2(right - size.x, y - size.y / 2.0), size);
    painter.rect_stroke(r, 3.0, Stroke::new(1.0, colour), StrokeKind::Inside);
    painter.galley(r.min + vec2(4.0, 1.0), galley, colour);
    r.left()
}

/// What a file state means, and what to do about it.
fn file_tip(app: &App, p: &leolib::Position, state: FileState) -> String {
    match state {
        FileState::Unread => {
            let path = app.outline().full_path(p);
            let why = app.unread.get(&path).map_or("", String::as_str);
            format!("The last read failed: {why}\nThe node does not hold the file.")
        }
        FileState::ChangedOnDisk => {
            "Another program changed the file.\nReload or Keep it in the bar above the body."
                .to_string()
        }
        FileState::Refused => "The file exists but was never read, so writing it asks first.\nFile > Read Files Here reads it.".to_string(),
        FileState::Unwritten => {
            "Edits not yet in the file.\nFile > Write Changed Files writes them.".to_string()
        }
    }
}

/// A fold arrow: right when folded, down when open.
fn triangle(painter: &egui::Painter, c: Pos2, open: bool, colour: Color32) {
    let points = match open {
        true => vec![
            c + vec2(-4.0, -2.0),
            c + vec2(4.0, -2.0),
            c + vec2(0.0, 3.0),
        ],
        false => vec![
            c + vec2(-2.0, -4.0),
            c + vec2(3.0, 0.0),
            c + vec2(-2.0, 4.0),
        ],
    };
    painter.add(Shape::convex_polygon(points, colour, Stroke::NONE));
}

/// A node's icon: a page for an `@<file>` node, a box for the rest, filled
/// when the body has text, with a second box behind it for a clone.
fn node_icon(painter: &egui::Painter, r: Rect, row: &leoapp::app::Row, colours: &Palette) {
    let stroke = Stroke::new(
        1.2,
        if row.is_file {
            colours.file
        } else {
            colours.dim
        },
    );
    if row.cloned {
        let back = r.translate(vec2(2.5, -2.5));
        painter.rect_stroke(back, 2.0, Stroke::new(1.0, colours.dim), StrokeKind::Inside);
        painter.rect_filled(r, 2.0, colours.panel);
    }
    if row.is_file {
        let fold = 3.5;
        let points = vec![
            r.left_top(),
            r.right_top() - vec2(fold, 0.0),
            r.right_top() + vec2(0.0, fold),
            r.right_bottom(),
            r.left_bottom(),
        ];
        painter.add(Shape::closed_line(points, stroke));
        return;
    }
    match row.has_body {
        true => painter.rect_filled(r.shrink(1.0), 2.0, stroke.color),
        false => painter.rect_stroke(r, 2.0, stroke, StrokeKind::Inside),
    };
}

/// A headline, its `@<file>` kind dimmed, search matches lit.
fn paint_headline(
    painter: &egui::Painter,
    row: &leoapp::app::Row,
    x: f32,
    rect: Rect,
    colours: &Palette,
    app: &App,
) {
    let font = FontId::proportional(14.0);
    let mut job = egui::text::LayoutJob::default();
    let format = |color: Color32, background: Color32| egui::TextFormat {
        font_id: font.clone(),
        color,
        background,
        ..Default::default()
    };
    let (kind, rest) = match row.is_file {
        true => row.headline.split_once(' ').unwrap_or((&row.headline, "")),
        false => ("", row.headline.as_str()),
    };
    if !kind.is_empty() {
        job.append(kind, 0.0, format(colours.dim, Color32::TRANSPARENT));
        job.append(" ", 0.0, format(colours.dim, Color32::TRANSPARENT));
    }
    let colour = if row.marked {
        colours.warning
    } else {
        colours.fg
    };
    let hits = app
        .hlsearch
        .as_ref()
        .map_or_else(Vec::new, |re| leoapp::search::ranges(re, rest));
    for (piece, hit) in leoapp::view::cut(rest, &hits) {
        let bg = if hit {
            crate::style::ansi(3).gamma_multiply(0.6)
        } else {
            Color32::TRANSPARENT
        };
        job.append(piece, 0.0, format(colour, bg));
    }
    let galley = painter.layout_job(job);
    let y = rect.center().y - galley.size().y / 2.0;
    painter.galley(pos2(x, y), galley, colours.fg);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drop_lands_by_the_part_of_the_row_it_is_on() {
        let r = Rect::from_min_size(pos2(0.0, 100.0), vec2(200.0, 20.0));
        assert_eq!(place_at(r, 102.0), Place::Before);
        assert_eq!(place_at(r, 110.0), Place::Inside);
        assert_eq!(place_at(r, 118.0), Place::After);
    }
}
