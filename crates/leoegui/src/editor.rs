//! The body editor: a monospace grid, so the columns leoapp counts for vim
//! motions are the columns drawn, inside a gutter, a scrollbar and the mouse.

use eframe::egui::{self, pos2, vec2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use leoapp::app::{App, Focus, Mode};
use leoapp::editor::motion::Kind;
use leoapp::highlight::Class;
use leoapp::theme::UnderlineStyle;
use leoapp::view::{self, BodyView, Viewport, SCOPES};

use crate::style::{self, Palette};

pub const FONT_SIZE: f32 = 14.0;
/// Where the body cursor was last drawn, kept in egui's memory for a hover.
pub const CURSOR: &str = "body-cursor";
/// Space between the text and the pane's edges.
const PAD: f32 = 6.0;
const SCROLLBAR: f32 = 10.0;

#[derive(Clone, Copy, PartialEq)]
pub struct Look {
    fg: Color32,
    bg: Option<Color32>,
    italic: bool,
    /// A diagnostic's underline: its colour and style.
    squiggle: Option<(Color32, UnderlineStyle)>,
}

#[derive(Default)]
pub struct Editor {
    /// Wheel movement not yet a whole line.
    wheel: f32,
    /// Where a scrollbar drag started, in lines from the top.
    thumb_from: Option<(f32, usize)>,
    /// What an IME is composing, drawn at the cursor.
    pub preedit: String,
}

pub fn cell(ctx: &egui::Context) -> Vec2 {
    let font = FontId::monospace(FONT_SIZE);
    ctx.fonts_mut(|f| vec2(f.glyph_width(&font, 'M'), f.row_height(&font) + 2.0))
}

impl Editor {
    pub fn ui(&mut self, ui: &mut egui::Ui, app: &mut App, colours: &Palette) {
        let cell = cell(ui.ctx());
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, colours.bg);

        let digits = app.body_buffer().len().max(1).to_string().len() + 1;
        let gutter = if app.options.number {
            (digits + 2) as f32 * cell.x
        } else {
            2.0 * cell.x
        };
        if colours.gutter != colours.bg {
            let strip = Rect::from_min_max(
                rect.min,
                pos2(rect.left() + gutter - cell.x * 0.5, rect.bottom()),
            );
            painter.rect_filled(strip, 0.0, colours.gutter);
        }
        let text = Rect::from_min_max(
            pos2(rect.left() + gutter, rect.top() + PAD),
            pos2(rect.right() - SCROLLBAR - PAD, rect.bottom() - PAD),
        );
        let v = Viewport {
            rows: (text.height() / cell.y).floor().max(1.0) as usize,
            cols: (text.width() / cell.x).floor().max(1.0) as usize
                + if app.options.number { digits } else { 0 },
        };
        let body = app.body_view(v);
        // leoapp counts the line numbers into the text's columns; the gutter
        // draws them, so the text starts after them.
        let origin = text.min - vec2(body.number_width as f32 * cell.x, 0.0);
        let at = |row: usize, col: usize| origin + vec2(col as f32 * cell.x, row as f32 * cell.y);
        let focused = app.focus == Focus::Body;

        if body.lines.iter().all(|l| l.trim().is_empty()) && !focused {
            painter.text(
                text.min + vec2(0.0, cell.y),
                egui::Align2::LEFT_TOP,
                "An empty body. Press Tab, then i, to type.",
                FontId::proportional(14.0),
                colours.dim,
            );
        }

        let lines = body.screen_lines();
        let cursor_row = body.cursor.map(|c| c.0);
        let palette = classes(app);
        let plain = Look {
            fg: colours.fg,
            bg: None,
            italic: false,
            squiggle: None,
        };
        let mut cells = Vec::new();
        for (y, screen) in lines.iter().enumerate() {
            let i = screen.line;
            let row_rect =
                Rect::from_min_size(pos2(rect.left(), at(y, 0).y), vec2(rect.width(), cell.y));
            if focused && Some(i) == cursor_row && app.mode != Mode::Visual {
                painter.rect_filled(row_rect, 0.0, colours.line);
            }
            if screen.first {
                let spans = body.spans.get(i).map(|v| v.as_slice()).unwrap_or(&[]);
                let (hits, marks) = (body.hits(i), body.marks(i));
                let pieces: Vec<(&str, Look)> =
                    view::decorate(&body.lines[i], spans, &hits, &marks)
                        .into_iter()
                        .map(|seg| {
                            let mut look = match palette.iter().find(|(c, _, _)| *c == seg.class) {
                                Some(&(_, fg, italic)) => Look {
                                    fg,
                                    italic,
                                    ..plain
                                },
                                None => plain,
                            };
                            if seg.hit {
                                look.bg = Some(style::ansi(3).gamma_multiply(0.55));
                            }
                            look.squiggle = seg.severity.map(|s| {
                                let m = colours.mark(s);
                                (m.underline, m.style)
                            });
                            (seg.text, look)
                        })
                        .collect();
                cells = view::expand(&pieces, body.tab);
                if app.options.number {
                    self.gutter(
                        &painter,
                        &body,
                        i,
                        at(y, 0),
                        cell,
                        digits,
                        Some(i) == cursor_row && focused,
                        colours,
                        rect.left(),
                    );
                }
            }
            if let Some((a, b)) = selected_columns(&body, i) {
                let (a, b) = (a.max(screen.from), b.min(screen.from + body.text_width));
                if a < b {
                    let x0 = at(y, body.number_width + a - screen.from).x;
                    let r = Rect::from_min_size(
                        pos2(x0, row_rect.top()),
                        vec2((b - a) as f32 * cell.x, cell.y),
                    );
                    painter.rect_filled(r, 0.0, colours.selection);
                }
            }
            let runs = view::columns(&cells, screen.from, body.text_width);
            paint_runs(&painter, at(y, body.number_width), cell, &runs);
        }

        // The cursor: a block in NORMAL, a bar while typing.
        if let (true, Some((y, x))) = (focused, body.cursor_screen()) {
            let min = at(y, x);
            let cursor = match app.mode {
                Mode::Insert => Rect::from_min_size(min, vec2(2.0, cell.y)),
                _ => Rect::from_min_size(min, cell),
            };
            let look = colours.cursor(app.mode);
            painter.rect_filled(cursor, 1.0, look.fill);
            // A block cursor shows the character under it, in the theme's
            // colour for that: a reversed cursor's is the background.
            if let (Some(text), true) = (look.text, app.mode != Mode::Insert) {
                let (row, col) = body.cursor.unwrap_or_default();
                let ch = body.lines.get(row).and_then(|l| l.chars().nth(col));
                if let Some(ch) = ch.filter(|c| !c.is_whitespace()) {
                    let mut buf = [0; 4];
                    painter.text(
                        min + vec2(0.0, 1.0),
                        egui::Align2::LEFT_TOP,
                        ch.encode_utf8(&mut buf),
                        FontId::monospace(FONT_SIZE),
                        text,
                    );
                }
            }
            ui.ctx()
                .memory_mut(|m| m.data.insert_temp(egui::Id::new(CURSOR), min));
            if app.mode == Mode::Insert {
                self.paint_preedit(&painter, min, cell, colours);
                ui.ctx().output_mut(|o| {
                    o.ime = Some(egui::output::IMEOutput {
                        purpose: egui::IMEPurpose::Normal,
                        rect,
                        cursor_rect: cursor,
                        should_interrupt_composition: false,
                    })
                });
            }
        }

        self.scrollbar(ui, &painter, app, &body, rect, colours);

        // The mouse: a click places the cursor, a drag selects.
        let grid = |p: Pos2| {
            let y = ((p.y - origin.y) / cell.y).floor().max(0.0) as usize;
            let x = ((p.x - origin.x) / cell.x).floor().max(0.0) as usize;
            (y, x.max(body.number_width))
        };
        let on_text = |p: Pos2| p.x < rect.right() - SCROLLBAR;
        if let Some(p) = response.interact_pointer_pos().filter(|p| on_text(*p)) {
            let (y, x) = grid(p);
            if response.drag_started() || response.clicked() {
                if app.run_chosen("") {
                    app.click_body(&body, y, x);
                }
            } else if response.dragged() && self.thumb_from.is_none() {
                app.drag_body(&body, y, x);
            }
        }
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
            if let Some(p) = response.hover_pos() {
                let (y, x) = grid(p);
                if let Some((row, col)) = body.position_at(y, x) {
                    let hit = body.diagnostics.iter().find(|d| {
                        (d.row, d.col) <= (row, col)
                            && (row, col) < (d.end_row, d.end_col.max(d.col + 1))
                    });
                    if let Some(d) = hit {
                        response
                            .clone()
                            .on_hover_text_at_pointer(leoapp::app::diagnostic_line(d));
                    }
                }
            }
            let lines = wheel_lines(ui, cell.y, &mut self.wheel);
            if lines != 0 {
                app.scroll(Focus::Body, lines);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn gutter(
        &self,
        painter: &egui::Painter,
        body: &BodyView,
        i: usize,
        at: Pos2,
        cell: Vec2,
        digits: usize,
        current: bool,
        colours: &Palette,
        left: f32,
    ) {
        let number = format!("{:>w$}", i + 1, w = digits - 1);
        let colour = if current {
            colours.linenr_current
        } else {
            colours.linenr
        };
        let x = left + cell.x;
        painter.text(
            pos2(x, at.y),
            egui::Align2::LEFT_TOP,
            number,
            FontId::monospace(FONT_SIZE),
            colour,
        );
        let worst = body
            .diagnostics
            .iter()
            .filter(|d| d.row <= i && i <= d.end_row)
            .map(|d| d.severity)
            .min();
        if let Some(s) = worst {
            painter.circle_filled(
                pos2(left + cell.x * 0.5, at.y + cell.y / 2.0),
                3.0,
                colours.mark(s).gutter,
            );
        }
    }

    fn paint_preedit(&self, painter: &egui::Painter, at: Pos2, cell: Vec2, colours: &Palette) {
        if self.preedit.is_empty() {
            return;
        }
        let width: usize = self.preedit.chars().map(view::char_width).sum();
        let r = Rect::from_min_size(at, vec2(width as f32 * cell.x, cell.y));
        painter.rect_filled(r, 0.0, colours.bg);
        painter.text(
            at,
            egui::Align2::LEFT_TOP,
            &self.preedit,
            FontId::monospace(FONT_SIZE),
            colours.fg,
        );
        painter.hline(r.x_range(), r.bottom() - 1.0, Stroke::new(1.0, colours.fg));
    }

    /// A scrollbar on the right, drawn when the body is taller than the pane.
    fn scrollbar(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        app: &mut App,
        body: &BodyView,
        rect: Rect,
        colours: &Palette,
    ) {
        let total = body.lines.len();
        let shown = body.height.max(1);
        if total <= shown {
            return;
        }
        let track = Rect::from_min_max(
            pos2(rect.right() - SCROLLBAR, rect.top()),
            rect.right_bottom(),
        );
        let h = (track.height() * shown as f32 / total as f32).max(24.0);
        let top =
            track.top() + (track.height() - h) * body.top as f32 / (total - shown).max(1) as f32;
        let thumb = Rect::from_min_size(pos2(track.left() + 2.0, top), vec2(SCROLLBAR - 4.0, h));
        let id = ui.id().with("body-scrollbar");
        let response = ui.interact(track, id, Sense::drag());
        let colour = if response.hovered() || response.dragged() {
            colours.dim
        } else {
            colours.border
        };
        painter.rect_filled(thumb, 3.0, colour);
        if response.drag_started() {
            if let Some(p) = response.interact_pointer_pos() {
                self.thumb_from = Some((p.y, body.top));
            }
        }
        if let (Some((y0, top0)), Some(p)) = (self.thumb_from, response.interact_pointer_pos()) {
            let per_line = (track.height() - h) / (total - shown).max(1) as f32;
            let target = (top0 as f32 + (p.y - y0) / per_line.max(0.01))
                .round()
                .max(0.0) as isize;
            app.scroll(Focus::Body, target - body.top as isize);
        }
        if response.drag_stopped() {
            self.thumb_from = None;
        }
    }
}

/// The colours the theme gives each class.
fn classes(app: &App) -> Vec<(Class, Color32, bool)> {
    SCOPES
        .iter()
        .filter_map(|&(class, scope)| {
            let face = app.theme.face(scope);
            Some((class, style::colour(app, face.fg)?, face.italic))
        })
        .collect()
}

/// The screen columns of line `i` the selection covers, end exclusive.
fn selected_columns(body: &BodyView, i: usize) -> Option<(usize, usize)> {
    let (a, b, kind) = body.selection?;
    if i < a.0 || i > b.0 {
        return None;
    }
    let line = body.lines.get(i).map_or("", |l| l.trim_end_matches('\n'));
    let width = view::display_col(line, line.chars().count(), body.tab);
    if kind == Kind::Linewise {
        return Some((0, width.max(1)));
    }
    let start = if i == a.0 {
        view::display_col(line, a.1, body.tab)
    } else {
        0
    };
    let end = match i == b.0 {
        true => view::display_col(line, b.1 + 1, body.tab).max(start + 1),
        false => width.max(start + 1),
    };
    Some((start, end))
}

/// Paint runs left to right from `at`. A character that is not one cell wide
/// is placed by itself, so the cells after it stay on the grid.
fn paint_runs(painter: &egui::Painter, at: Pos2, cell: Vec2, runs: &[(String, Look)]) {
    let mut col = 0;
    for (text, look) in runs {
        let width: usize = text.chars().map(view::char_width).sum();
        let min = at + vec2(col as f32 * cell.x, 0.0);
        let r = Rect::from_min_size(min, vec2(width as f32 * cell.x, cell.y));
        if let Some(bg) = look.bg {
            painter.rect_filled(r, 0.0, bg);
        }
        if text.chars().all(|c| view::char_width(c) == 1) {
            paint_text(painter, min, text, *look);
        } else {
            let mut c = col;
            for ch in text.chars() {
                let mut buf = [0; 4];
                paint_text(
                    painter,
                    at + vec2(c as f32 * cell.x, 0.0),
                    ch.encode_utf8(&mut buf),
                    *look,
                );
                c += view::char_width(ch);
            }
        }
        if let Some((colour, style)) = look.squiggle {
            underline(
                painter,
                r.left()..=r.right(),
                r.bottom() - 2.0,
                colour,
                style,
            );
        }
        col += width;
    }
}

fn paint_text(painter: &egui::Painter, pos: Pos2, text: &str, look: Look) {
    let format = egui::TextFormat {
        font_id: FontId::monospace(FONT_SIZE),
        color: look.fg,
        italics: look.italic,
        ..Default::default()
    };
    let job = egui::text::LayoutJob::single_section(text.to_string(), format);
    painter.galley(pos + vec2(0.0, 1.0), painter.layout_job(job), look.fg);
}

/// A diagnostic's line under its text, in the theme's style: wavy for
/// `curl`, the rest as they are named.
fn underline(
    painter: &egui::Painter,
    x: std::ops::RangeInclusive<f32>,
    y: f32,
    colour: Color32,
    style: UnderlineStyle,
) {
    let stroke = Stroke::new(1.0, colour);
    let (a, b) = (*x.start(), *x.end());
    match style {
        UnderlineStyle::Curl => {
            let mut points = Vec::new();
            let mut px = a;
            let mut up = false;
            while px <= b {
                points.push(pos2(px, if up { y - 1.5 } else { y + 1.0 }));
                up = !up;
                px += 2.0;
            }
            if points.len() > 1 {
                painter.add(Shape::line(points, stroke));
            }
        }
        UnderlineStyle::Line => {
            painter.hline(a..=b, y, stroke);
        }
        UnderlineStyle::Double => {
            painter.hline(a..=b, y - 1.0, stroke);
            painter.hline(a..=b, y + 1.0, stroke);
        }
        UnderlineStyle::Dashed | UnderlineStyle::Dotted => {
            let (on, off) = match style {
                UnderlineStyle::Dashed => (4.0, 2.0),
                _ => (1.0, 2.0),
            };
            let mut px = a;
            while px < b {
                painter.hline(px..=(px + on).min(b), y, stroke);
                px += on + off;
            }
        }
    }
}

/// Whole lines the wheel moved this frame, down positive.
pub fn wheel_lines(ui: &egui::Ui, row: f32, carry: &mut f32) -> isize {
    let dy: f32 = ui.input(|i| {
        i.events
            .iter()
            .map(|e| match e {
                egui::Event::MouseWheel { unit, delta, .. } => match unit {
                    egui::MouseWheelUnit::Line => delta.y,
                    egui::MouseWheelUnit::Point => delta.y / row,
                    egui::MouseWheelUnit::Page => delta.y * 10.0,
                },
                _ => 0.0,
            })
            .sum()
    });
    *carry -= dy;
    let lines = carry.trunc();
    *carry -= lines;
    lines as isize
}
