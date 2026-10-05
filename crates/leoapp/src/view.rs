//! What a front end draws, as plain data.
//!
//! A front end gives each pane's size in character cells and gets back what
//! fits, already scrolled. Scrolling lives here, not in the renderer, because
//! paging and `scroll_to_cursor` read the same heights. Styles do not: a
//! class or a scope is all this says about how a run looks.

use std::ops::Range;
use std::rc::Rc;

use regex::Regex;

use crate::app::{App, Focus, Mode, Row};
use crate::bindings::{self, BINDINGS};
use crate::commands;
use crate::editor::motion::{Kind, Pos};
use crate::highlight::{self, Class};
use crate::keys::{KeyCode, KeyEvent, KeyModifiers};
pub use leolsp::{BodyDiagnostic, Severity};

/// A pane's size in character cells, borders excluded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    pub rows: usize,
    pub cols: usize,
}

/// The outline rows that fit, from `top`.
pub struct TreeView {
    pub rows: Vec<TreeRow>,
    /// Index of the first shown row among all rows.
    pub top: usize,
    /// Index of the selected row among all rows.
    pub current: usize,
    pub total: usize,
}

pub struct TreeRow {
    pub row: Row,
    /// hlsearch matches in the headline, as byte ranges.
    pub hits: Vec<Range<usize>>,
}

/// The body, scrolled so the cursor is on screen.
pub struct BodyView {
    pub lines: Vec<String>,
    /// Classified runs per line, empty when the body is not coloured.
    pub spans: Rc<Vec<Vec<highlight::Span>>>,
    /// Line and character column, when the body has focus.
    pub cursor: Option<Pos>,
    /// The cursor's screen column within its line, tabs and wide characters
    /// counted.
    pub cursor_col: Option<usize>,
    pub selection: Option<(Pos, Pos, Kind)>,
    /// First line shown, and first column shown when lines are not wrapped.
    pub top: usize,
    pub hscroll: usize,
    pub height: usize,
    /// Columns the line numbers take, their trailing blank included; 0 without
    /// `:set number`.
    pub number_width: usize,
    pub text_width: usize,
    pub tab: usize,
    pub wrap: bool,
    /// The language server's diagnostics, shown only over committed text:
    /// while a change is typed, rows no longer match what the server saw.
    pub diagnostics: Vec<BodyDiagnostic>,
    hlsearch: Option<Regex>,
}

/// One screen row of the body: which line, and its first screen column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScreenLine {
    pub line: usize,
    /// The first screen row of its line, which carries the line number.
    pub first: bool,
    pub from: usize,
}

impl BodyView {
    /// Screen rows line `i` takes: one, or as many as wrapping needs. The
    /// cursor past the end of a line counts as one more column.
    pub fn screen_rows(&self, i: usize) -> usize {
        if !self.wrap {
            return 1;
        }
        let line = self.lines.get(i).map_or("", |l| l.as_str());
        let mut width = display_col(line, line.chars().count(), self.tab);
        if let Some((_, col)) = self.cursor.filter(|c| c.0 == i) {
            width = width.max(display_col(line, col, self.tab) + 1);
        }
        width.div_ceil(self.text_width).max(1)
    }

    /// The screen rows that fit, top to bottom.
    pub fn screen_lines(&self) -> Vec<ScreenLine> {
        let mut out = Vec::new();
        for i in self.top..self.lines.len() {
            if out.len() >= self.height {
                break;
            }
            let (from, count) = match self.wrap {
                true => (0, self.screen_rows(i)),
                false => (self.hscroll, 1),
            };
            for k in 0..count.min(self.height - out.len()) {
                out.push(ScreenLine {
                    line: i,
                    first: k == 0,
                    from: from + k * self.text_width,
                });
            }
        }
        out
    }

    /// Lines below the last one shown.
    pub fn more(&self) -> usize {
        let last = self.screen_lines().last().map_or(self.top, |s| s.line);
        self.lines.len().saturating_sub(last + 1)
    }

    /// The cursor's screen row and column within the pane, line numbers
    /// included in the column.
    pub fn cursor_screen(&self) -> Option<(usize, usize)> {
        let ((row, _), col) = (self.cursor?, self.cursor_col?);
        let (dy, dx) = match self.wrap {
            true => (col / self.text_width, col % self.text_width),
            false => (0, col - self.hscroll),
        };
        let y = (self.top..row).map(|i| self.screen_rows(i)).sum::<usize>() + dy;
        Some((
            y.min(self.height.saturating_sub(1)),
            self.number_width + dx.min(self.text_width - 1),
        ))
    }

    /// The line and character a click at screen row `y`, column `x` of the
    /// pane lands on: the character under it, or the end of a short line.
    pub fn position_at(&self, y: usize, x: usize) -> Option<Pos> {
        let screen = *self.screen_lines().get(y)?;
        let line = self.lines.get(screen.line).map_or("", |l| l.as_str());
        let target = screen.from + x.saturating_sub(self.number_width);
        // The first character whose right edge is past the click.
        let chars = line.trim_end_matches('\n').chars().count();
        let col = (0..chars)
            .find(|&c| display_col(line, c + 1, self.tab) > target)
            .unwrap_or(chars);
        Some((screen.line, col))
    }

    /// The diagnostics over line `i`, as byte ranges, worst last so it is
    /// drawn over the others. A diagnostic with no width marks one character.
    pub fn marks(&self, i: usize) -> Vec<(Range<usize>, Severity)> {
        let Some(line) = self.lines.get(i) else {
            return Vec::new();
        };
        let line = line.trim_end_matches('\n');
        let byte = |col: usize| line.char_indices().nth(col).map_or(line.len(), |(b, _)| b);
        let mut out: Vec<(Range<usize>, Severity)> = self
            .diagnostics
            .iter()
            .filter(|d| d.row <= i && i <= d.end_row)
            .filter_map(|d| {
                let start = if d.row == i { byte(d.col) } else { 0 };
                let mut end = if d.end_row == i {
                    byte(d.end_col)
                } else {
                    line.len()
                };
                if end <= start {
                    end = line[start..]
                        .chars()
                        .next()
                        .map_or(start, |c| start + c.len_utf8());
                }
                (start < end).then_some((start..end, d.severity))
            })
            .collect();
        out.sort_by_key(|(_, s)| std::cmp::Reverse(*s));
        out
    }

    /// hlsearch matches in line `i`, as byte ranges.
    pub fn hits(&self, i: usize) -> Vec<Range<usize>> {
        match (&self.hlsearch, self.lines.get(i)) {
            (Some(re), Some(line)) => crate::search::ranges(re, line),
            _ => Vec::new(),
        }
    }
}

/// The help overlay's lines that fit.
pub struct HelpView {
    pub name: String,
    pub lines: Vec<String>,
    pub first: usize,
    pub total: usize,
}

impl App {
    /// The outline rows that fit in `v`, scrolled to keep the selection on
    /// screen without recentring on every keypress.
    pub fn tree_view(&mut self, v: Viewport) -> TreeView {
        self.tree_height = v.rows.max(1);
        let (total, current) = (self.row_count(), self.current_row());
        if current < self.top {
            self.top = current;
        } else if v.rows > 0 && current >= self.top + v.rows {
            self.top = current + 1 - v.rows;
        }
        let rows = self
            .rows_in(self.top..self.top + v.rows)
            .into_iter()
            .map(|row| TreeRow {
                hits: self
                    .hlsearch
                    .as_ref()
                    .map_or_else(Vec::new, |re| crate::search::ranges(re, &row.headline)),
                row,
            })
            .collect();
        TreeView {
            rows,
            top: self.top,
            current,
            total,
        }
    }

    /// The body as it fits in `v`. While editing it scrolls to the cursor;
    /// otherwise it keeps the scroll it was given.
    pub fn body_view(&mut self, v: Viewport) -> BodyView {
        // The body is the model's text, unless a change is being typed.
        let lines = self.body_buffer();
        let cursor = (self.focus == Focus::Body).then_some(self.editor.cursor);
        let selection = self.editor.visual_range(&lines);
        self.body_height = v.rows.max(1);
        let number_width = match self.options.number {
            true => format!("{} ", lines.len()).len(),
            false => 0,
        };
        let text_width = v.cols.saturating_sub(number_width).max(1);
        let tab = self.tab_stop();
        let cursor_col = cursor
            .map(|(row, col)| display_col(lines.get(row).map_or("", |l| l.as_str()), col, tab));
        let mut view = BodyView {
            lines,
            spans: Rc::default(),
            cursor,
            cursor_col,
            selection,
            top: 0,
            hscroll: 0,
            height: v.rows,
            number_width,
            text_width,
            tab,
            wrap: self.options.wrap,
            diagnostics: match self.buffer {
                None => self.diagnostics.clone(),
                Some(_) => Vec::new(),
            },
            hlsearch: self.hlsearch.clone(),
        };

        let max_top = view.lines.len().saturating_sub(1);
        let mut top = match cursor {
            Some((row, _)) if row < self.body_scroll => row,
            Some(_) => self.body_scroll,
            None => self.body_scroll.min(max_top),
        };
        if let (Some((row, _)), Some(col)) = (cursor, cursor_col) {
            let below = |top: usize| {
                (top..row).map(|i| view.screen_rows(i)).sum::<usize>()
                    + if view.wrap { col / text_width } else { 0 }
            };
            while top < row && below(top) >= v.rows {
                top += 1;
            }
        }
        self.body_scroll = top;
        // Without wrapping, the view slides sideways to keep the cursor on it.
        if view.wrap || cursor.is_none() {
            self.body_hscroll = 0;
        } else if let Some(col) = cursor_col {
            if col < self.body_hscroll {
                self.body_hscroll = col;
            } else if col >= self.body_hscroll + text_width {
                self.body_hscroll = col + 1 - text_width;
            }
        }
        view.top = top;
        view.hscroll = self.body_hscroll;

        // The language comes from the model, and the body may change it partway
        // through: see `highlight`. A node nothing declares a language for is left
        // plain rather than coloured as whatever the outline's default is.
        let language = match self.options.syntax {
            true => highlight::language_of(self.outline(), &self.current),
            false => None,
        };
        if let Some(language) = language {
            view.spans = self.colouring.of(&view.lines, &language);
        }
        view
    }

    /// The help overlay's lines that fit in `rows`: the overlay's own, or the
    /// bindings of the focused pane.
    pub fn help_view(&mut self, rows: usize) -> HelpView {
        let (name, lines) = match &self.overlay {
            Some((name, lines)) => (name.clone(), lines.clone()),
            None => {
                let pane = match self.focus {
                    Focus::Tree => "outline",
                    Focus::Body => "body",
                };
                (format!("keys: {pane} pane"), help_lines(self.focus))
            }
        };
        self.help_scroll = self.help_scroll.min(lines.len().saturating_sub(rows));
        HelpView {
            name,
            total: lines.len(),
            first: self.help_scroll,
            lines: lines
                .into_iter()
                .skip(self.help_scroll)
                .take(rows)
                .collect(),
        }
    }

    /// The status line's text after the mode: the message, or the outline's
    /// state and a hint.
    pub fn status_text(&mut self) -> String {
        if !self.message.is_empty() {
            return self.message.clone();
        }
        let hint = self.status_hint();
        format!("{}  {hint}", self.status())
    }

    /// What the status line says when there is no message: keys typed
    /// towards a binding, the diagnostic on the cursor's line, or what the
    /// mode's way out is.
    pub fn status_hint(&self) -> String {
        let pending = self.pending_keys();
        let on_row = (self.focus == Focus::Body && self.buffer.is_none())
            .then(|| self.diagnostic_at(self.editor.cursor.0))
            .flatten();
        match (pending.is_empty(), self.mode) {
            (false, _) => pending,
            (true, Mode::Normal) if on_row.is_some() => {
                crate::app::diagnostic_line(on_row.expect("checked"))
            }
            (true, Mode::Help) => "q closes".to_string(),
            (true, Mode::Insert) => "Esc commits".to_string(),
            (true, Mode::Visual) => "d c y > < to operate, Esc cancels".to_string(),
            _ => "F1 help".to_string(),
        }
    }

    /// The columns a tab advances to: the node's `@tabwidth`, as a width.
    pub fn tab_stop(&self) -> usize {
        self.outline()
            .get_tab_width(&self.current)
            .unsigned_abs()
            .clamp(1, 16) as usize
    }
}

impl App {
    /// Text a front end received as typing or an IME commit, one key per
    /// character, as a terminal would send it.
    pub fn handle_text(&mut self, text: &str) {
        for ch in text.chars() {
            let code = match ch {
                '\n' | '\r' => KeyCode::Enter,
                '\t' => KeyCode::Tab,
                ch => KeyCode::Char(ch),
            };
            self.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        }
    }

    /// A click on outline row `row` of the last `tree_view`. Only in NORMAL:
    /// elsewhere the keyboard owns a line or a change being typed.
    pub fn click_tree(&mut self, tree: &TreeView, row: usize) {
        if let Some(r) = tree.rows.get(row) {
            self.click_node(&r.row.position.clone());
        }
    }

    /// A click on node p in an outline the front end lays out itself.
    pub fn click_node(&mut self, p: &leolib::Position) {
        if self.mode != Mode::Normal {
            return;
        }
        self.message.clear();
        self.focus = Focus::Tree;
        self.select(p.clone());
    }

    /// A click at screen row `y`, column `x` of the last `body_view`.
    pub fn click_body(&mut self, body: &BodyView, y: usize, x: usize) {
        if self.mode != Mode::Normal {
            return;
        }
        let Some(pos) = body.position_at(y, x) else {
            return;
        };
        self.message.clear();
        self.focus = Focus::Body;
        self.editor.cursor = pos;
        self.editor.desired_col = pos.1;
        self.editor.clamp(&body.lines);
    }

    /// The mouse dragged to screen row `y`, column `x` of the last
    /// `body_view` after a click: a charwise VISUAL selection from the click.
    pub fn drag_body(&mut self, body: &BodyView, y: usize, x: usize) {
        let Some(pos) = body.position_at(y, x) else {
            return;
        };
        match self.mode {
            Mode::Normal if pos != self.editor.cursor => {
                self.editor.start_visual(Kind::Charwise);
                self.mode = Mode::Visual;
            }
            Mode::Visual => {}
            _ => return,
        }
        self.editor.cursor = pos;
        self.editor.desired_col = pos.1;
        self.editor.clamp(&body.lines);
    }

    /// A menu or palette choice: `line` run as a `:` line. INSERT commits and
    /// VISUAL or the help closes first; a line being typed is left alone,
    /// since abandoning it would lose it. False if nothing ran.
    pub fn run_chosen(&mut self, line: &str) -> bool {
        match self.mode {
            Mode::Insert | Mode::Visual => {
                self.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            }
            Mode::Help => self.run("close-help", 1),
            _ => {}
        }
        if self.mode != Mode::Normal {
            self.message = "finish the line being typed first: Enter or Escape".into();
            return false;
        }
        self.message.clear();
        self.run_command_line(line);
        self.log_message();
        true
    }

    /// Fold or unfold p without moving the selection, unless the selection
    /// would be folded away, when p is selected as Leo does.
    pub fn toggle_fold(&mut self, p: &leolib::Position) {
        if self.mode != Mode::Normal {
            return;
        }
        let o = self.doc.outline_mut_untracked();
        if o.is_expanded(p) {
            o.contract(p);
            let hidden = self
                .current
                .parents(self.doc.outline())
                .iter()
                .any(|a| a == p);
            if hidden {
                self.select(p.clone());
            }
        } else {
            o.expand(p);
        }
    }

    /// A node dropped on another: moved before, after or into it, as one
    /// undo step.
    pub fn drop_node(
        &mut self,
        from: &leolib::Position,
        onto: &leolib::Position,
        place: leolib::Place,
    ) {
        if self.mode != Mode::Normal || from == onto {
            return;
        }
        match self.doc.move_node(from, onto, place) {
            Some(p) => {
                if place == leolib::Place::Inside {
                    self.doc.outline_mut_untracked().expand(onto);
                }
                self.select(p);
            }
            None => self.message = "a node cannot move into its own tree".into(),
        }
    }

    /// The wheel over a pane, in lines, down positive. The outline moves its
    /// selection; the body moves its view, and the cursor only as far as
    /// staying in view needs, as vim's Ctrl-e and Ctrl-y.
    pub fn scroll(&mut self, pane: Focus, lines: isize) {
        if self.mode != Mode::Normal {
            return;
        }
        match pane {
            Focus::Tree => self.move_rows(lines as i32),
            Focus::Body => {
                let text = self.body_buffer();
                let last = text.len().saturating_sub(1);
                self.body_scroll = self.body_scroll.saturating_add_signed(lines).min(last);
                if self.focus == Focus::Body {
                    let bottom = self.body_scroll + self.body_height.max(1) - 1;
                    let row = self.editor.cursor.0.clamp(self.body_scroll, bottom);
                    if row != self.editor.cursor.0 {
                        self.editor.cursor = (row, self.editor.desired_col);
                        self.editor.clamp(&text);
                    }
                }
            }
        }
    }
}

/// Each command bound in NORMAL for `focus`, with its keys there and its
/// summary. The single source is the binding table, so the help cannot
/// drift from what the keys do.
pub fn help_entries(focus: Focus) -> Vec<(Vec<&'static str>, &'static str, &'static str)> {
    let mut out = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for binding in bindings::for_context(Mode::Normal, focus) {
        if seen.contains(&binding.command) {
            continue;
        }
        seen.push(binding.command);
        let keys = bindings::keys_for(binding.command)
            .into_iter()
            .filter(|k| {
                BINDINGS.iter().any(|x| {
                    x.keys == *k
                        && x.command == binding.command
                        && x.mode == Mode::Normal
                        && (x.focus.is_none() || x.focus == Some(focus))
                })
            })
            .collect();
        let summary = commands::find(binding.command).map_or("", |x| x.summary);
        out.push((keys, binding.command, summary));
    }
    out
}

/// `help_entries` as the lines the help overlay and `--keys` print.
pub fn help_lines(focus: Focus) -> Vec<String> {
    help_entries(focus)
        .into_iter()
        .map(|(keys, command, summary)| format!("{:22} {:24} {}", keys.join(" "), command, summary))
        .collect()
}

/// The Helix scope each class is drawn as.
///
/// A theme names scopes, not classes, and resolves `type.builtin` to `type`
/// when it defines only the second. `Plain` is absent, and keeps the
/// front end's own foreground.
pub const SCOPES: &[(Class, &str)] = &[
    (Class::Directive, "keyword.directive"),
    (Class::Section, "markup.link.text"),
    (Class::Comment, "comment"),
    (Class::Str, "string"),
    (Class::Number, "constant.numeric"),
    (Class::Keyword, "keyword"),
    (Class::BuiltinFunction, "function.builtin"),
    (Class::BuiltinType, "type.builtin"),
    (Class::BuiltinConstant, "constant.builtin"),
    (Class::Function, "function"),
    (Class::Type, "type"),
    (Class::Property, "variable.other.member"),
    (Class::Attribute, "attribute"),
];

/// The cells a character takes. A control character takes none.
pub fn char_width(ch: char) -> usize {
    unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0)
}

/// The screen column at which character `col` of `line` starts.
pub fn display_col(line: &str, col: usize, tab: usize) -> usize {
    line.chars().take(col).fold(0, |at, ch| match ch {
        '\t' => at + tab - at % tab,
        ch => at + char_width(ch),
    })
}

/// A line's characters, each with its style and screen column, tabs expanded
/// to blanks at stops of `tab` columns.
pub fn expand<S: Copy>(pieces: &[(&str, S)], tab: usize) -> Vec<(char, S, usize)> {
    let mut out = Vec::new();
    let mut at = 0;
    for (text, style) in pieces {
        for ch in text.chars() {
            if ch == '\t' {
                for _ in 0..tab - at % tab {
                    out.push((' ', *style, at));
                    at += 1;
                }
            } else if char_width(ch) > 0 {
                out.push((ch, *style, at));
                at += char_width(ch);
            }
        }
    }
    out
}

/// The runs covering `width` screen columns from column `from`, adjacent
/// cells of one style merged. A wide character cut by either edge is drawn
/// as a blank.
pub fn columns<S: Copy + PartialEq>(
    cells: &[(char, S, usize)],
    from: usize,
    width: usize,
) -> Vec<(String, S)> {
    let end = from + width;
    let mut runs: Vec<(String, S)> = Vec::new();
    let mut push = |text: &str, style: S| match runs.last_mut() {
        Some((last, s)) if *s == style => last.push_str(text),
        _ => runs.push((text.to_string(), style)),
    };
    for &(ch, style, at) in cells {
        let w = char_width(ch);
        if at + w <= from || at >= end {
            continue;
        }
        if at < from || at + w > end {
            let blanks = (at + w).min(end) - at.max(from);
            push(&" ".repeat(blanks), style);
        } else {
            push(ch.encode_utf8(&mut [0; 4]), style);
        }
    }
    runs
}

/// A run of a line that is drawn one way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment<'a> {
    pub text: &'a str,
    pub class: Class,
    /// Inside an hlsearch match.
    pub hit: bool,
    /// Inside a diagnostic, the worst one there.
    pub severity: Option<Severity>,
}

/// `line` cut wherever its colouring, a search match or a diagnostic starts
/// or ends. `marks` comes from `BodyView::marks`, worst last.
pub fn decorate<'a>(
    line: &'a str,
    spans: &[highlight::Span],
    hits: &[Range<usize>],
    marks: &[(Range<usize>, Severity)],
) -> Vec<Segment<'a>> {
    let end = line.trim_end_matches('\n').len();
    let mut cuts: Vec<usize> = vec![0, end];
    for s in spans {
        cuts.extend([s.start, s.end]);
    }
    for r in hits.iter().chain(marks.iter().map(|(r, _)| r)) {
        cuts.extend([r.start, r.end]);
    }
    cuts.retain(|&c| c <= end && line.is_char_boundary(c));
    cuts.sort_unstable();
    cuts.dedup();
    cuts.windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            let inside = |r: &Range<usize>| r.start <= a && b <= r.end;
            Segment {
                text: &line[a..b],
                class: spans
                    .iter()
                    .find(|s| s.start <= a && b <= s.end)
                    .map_or(Class::Plain, |s| s.class),
                hit: hits.iter().any(inside),
                severity: marks.iter().rev().find(|(r, _)| inside(r)).map(|(_, s)| *s),
            }
        })
        .collect()
}

/// `text` cut at the edges of `ranges`, each piece marked if it lies in one.
/// Ranges are byte offsets, sorted and apart, as `Regex::find_iter` gives
/// them; any part past the end of `text` is dropped.
pub fn cut<'a>(text: &'a str, ranges: &[Range<usize>]) -> Vec<(&'a str, bool)> {
    let mut out = Vec::new();
    let mut at = 0;
    for r in ranges {
        let (start, end) = (r.start.min(text.len()), r.end.min(text.len()));
        if start < at || start >= end {
            continue;
        }
        if start > at {
            out.push((&text[at..start], false));
        }
        out.push((&text[start..end], true));
        at = end;
    }
    if at < text.len() {
        out.push((&text[at..], false));
    }
    out
}

/// A line split into its classified runs, plain text included.
pub fn split_line<'a>(line: &'a str, spans: &[highlight::Span]) -> Vec<(&'a str, Class)> {
    let end = line.trim_end_matches('\n').len();
    let mut out = Vec::new();
    let mut at = 0usize;
    for span in spans {
        if span.start > at {
            out.push((&line[at..span.start.min(end)], Class::Plain));
        }
        out.push((&line[span.start.min(end)..span.end.min(end)], span.class));
        at = span.end.min(end);
    }
    if at < end {
        out.push((&line[at..end], Class::Plain));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use leolib::Document;

    fn app_with_body(body: &str) -> App {
        let mut doc = Document::new_empty("");
        let root = doc.outline().root_position().unwrap();
        doc.set_body(&root, body);
        let mut app = App::new(doc);
        app.focus = Focus::Body;
        app
    }

    #[test]
    fn every_class_but_plain_has_a_scope() {
        // A class with no entry is drawn plain, which for anything but
        // `Plain` would be a colour silently lost.
        for class in [
            Class::Directive,
            Class::Section,
            Class::Comment,
            Class::Str,
            Class::Number,
            Class::Keyword,
            Class::BuiltinFunction,
            Class::BuiltinType,
            Class::BuiltinConstant,
            Class::Function,
            Class::Type,
            Class::Property,
            Class::Attribute,
        ] {
            assert!(
                SCOPES.iter().any(|(c, _)| *c == class),
                "{class:?} has no scope"
            );
        }
        assert!(!SCOPES.iter().any(|(c, _)| *c == Class::Plain));
    }

    #[test]
    fn the_tree_scrolls_to_keep_the_selection_shown() {
        let mut doc = Document::new_empty("");
        let root = doc.outline().root_position().unwrap();
        let mut p = root;
        for _ in 0..9 {
            p = doc.outline_mut_untracked().insert_after(&p);
        }
        let mut app = App::new(doc);
        let v = Viewport { rows: 4, cols: 20 };
        app.move_to_row(6);
        let tree = app.tree_view(v);
        assert_eq!((tree.top, tree.current, tree.total), (3, 6, 10));
        assert_eq!(tree.rows.len(), 4);
        app.move_to_row(1);
        assert_eq!(app.tree_view(v).top, 1);
    }

    #[test]
    fn the_body_scrolls_to_the_cursor_and_says_what_is_below() {
        let body: String = (0..10).map(|i| format!("line {i}\n")).collect();
        let mut app = app_with_body(&body);
        app.editor.cursor = (7, 2);
        let view = app.body_view(Viewport { rows: 3, cols: 20 });
        assert_eq!(view.top, 5);
        assert_eq!(view.cursor_screen(), Some((2, 2)));
        assert_eq!(view.more(), 2);
        assert_eq!(app.body_scroll, 5);
    }

    #[test]
    fn a_wrapped_line_takes_as_many_rows_as_it_needs() {
        let mut app = app_with_body("abcdefghij\nx\n");
        app.options.wrap = true;
        app.editor.cursor = (0, 7);
        let view = app.body_view(Viewport { rows: 5, cols: 4 });
        assert_eq!(view.screen_rows(0), 3);
        let froms: Vec<(usize, bool, usize)> = view
            .screen_lines()
            .iter()
            .map(|s| (s.line, s.first, s.from))
            .collect();
        assert_eq!(
            froms,
            [(0, true, 0), (0, false, 4), (0, false, 8), (1, true, 0)]
        );
        assert_eq!(view.cursor_screen(), Some((1, 3)));
    }

    #[test]
    fn an_unwrapped_body_slides_sideways_to_the_cursor() {
        let mut app = app_with_body("abcdefghij\n");
        app.editor.cursor = (0, 8);
        let view = app.body_view(Viewport { rows: 2, cols: 4 });
        assert_eq!(view.hscroll, 5);
        assert_eq!(view.cursor_screen(), Some((0, 3)));
    }

    #[test]
    fn line_numbers_take_columns_from_the_text() {
        let mut app = app_with_body("a\nb\n");
        app.options.number = true;
        let view = app.body_view(Viewport { rows: 2, cols: 10 });
        assert_eq!((view.number_width, view.text_width), (2, 8));
        assert_eq!(view.cursor_screen(), Some((0, 2)));
    }

    #[test]
    fn help_scroll_stops_at_the_last_page() {
        let mut app = app_with_body("");
        app.overlay = Some(("m".into(), (0..10).map(|i| i.to_string()).collect()));
        app.help_scroll = 50;
        let help = app.help_view(4);
        assert_eq!((help.first, help.total), (6, 10));
        assert_eq!(help.lines, ["6", "7", "8", "9"]);
    }

    #[test]
    fn a_click_lands_on_the_character_under_it() {
        let mut app = app_with_body("a\tbc\nxy\n");
        app.options.number = true;
        app.focus = Focus::Tree;
        let body = app.body_view(Viewport { rows: 5, cols: 20 });
        // Two columns of line number, then `a`, the tab to column 4, `b`.
        assert_eq!(body.tab, 4);
        assert_eq!(body.position_at(0, 2), Some((0, 0)));
        assert_eq!(body.position_at(0, 5), Some((0, 1)));
        assert_eq!(body.position_at(0, 6), Some((0, 2)));
        assert_eq!(body.position_at(0, 19), Some((0, 4)));
        assert_eq!(body.position_at(9, 0), None);
        app.click_body(&body, 1, 19);
        // Past the end in NORMAL is the last character, as vim's cursor.
        assert_eq!((app.focus, app.editor.cursor), (Focus::Body, (1, 1)));
    }

    #[test]
    fn a_click_selects_an_outline_row_only_in_normal_mode() {
        let mut doc = Document::new_empty("");
        let root = doc.outline().root_position().unwrap();
        let b = doc.outline_mut_untracked().insert_after(&root);
        doc.set_headline(&b, "b");
        let mut app = App::new(doc);
        app.focus = Focus::Body;
        let tree = app.tree_view(Viewport { rows: 5, cols: 20 });
        app.click_tree(&tree, 1);
        assert_eq!((app.focus, app.current_row()), (Focus::Tree, 1));
        app.mode = Mode::Insert;
        app.click_tree(&tree, 0);
        assert_eq!(app.current_row(), 1);
    }

    #[test]
    fn text_types_one_key_per_character() {
        let mut app = app_with_body("");
        app.handle_text("i");
        assert_eq!(app.mode, Mode::Insert);
        app.handle_text("h\u{e9}");
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(
            app.outline().root_position().unwrap().b(app.outline()),
            "h\u{e9}\n"
        );
    }

    #[test]
    fn the_wheel_moves_the_body_view_and_keeps_the_cursor_in_it() {
        let body: String = (0..20).map(|i| format!("{i}\n")).collect();
        let mut app = app_with_body(&body);
        app.body_view(Viewport { rows: 5, cols: 10 });
        app.scroll(Focus::Body, 3);
        assert_eq!((app.body_scroll, app.editor.cursor.0), (3, 3));
        app.scroll(Focus::Body, 100);
        assert_eq!(app.body_scroll, 19);
        app.scroll(Focus::Body, -17);
        assert_eq!((app.body_scroll, app.editor.cursor.0), (2, 6));
    }

    #[test]
    fn a_yes_or_no_answers_the_quit_question_in_one_press() {
        let mut app = app_with_body("x\n");
        app.handle_text("ihi");
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.request_quit();
        assert_eq!(app.mode, Mode::Confirm);
        app.answer(false);
        assert_eq!((app.mode, app.quit), (Mode::Normal, false));
        app.request_quit();
        // A key typed before the answer does not spoil it.
        app.handle_text("x");
        app.answer(true);
        assert!(app.quit);
    }

    #[test]
    fn the_row_cache_follows_folds_inserts_hoists_and_headlines() {
        let mut doc = Document::new_empty("");
        let a = doc.outline().root_position().unwrap();
        doc.set_headline(&a, "a");
        let a1 = doc.outline_mut_untracked().insert_as_last_child(&a);
        doc.set_headline(&a1, "a1");
        let mut app = App::new(doc);
        let heads =
            |app: &App| -> Vec<String> { app.rows().into_iter().map(|r| r.headline).collect() };
        let a = app.rows()[0].position.clone();
        app.doc.outline_mut_untracked().contract(&a);
        assert_eq!(heads(&app), ["a"]);
        app.doc.outline_mut_untracked().expand(&a);
        assert_eq!(heads(&app), ["a", "a1"]);
        app.run("insert-node", 1);
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.row_count(), 3);
        let a1 = app.rows()[1].position.clone();
        app.doc.set_headline(&a1, "renamed");
        assert_eq!(heads(&app)[1], "renamed");
        app.select(a.clone());
        assert_eq!(app.current_row(), 0);
        // The new node went in as a's first child, as Leo inserts under an
        // open node.
        app.run("hoist", 1);
        assert_eq!(heads(&app), ["a", "renamed", "a1"]);
        app.run("dehoist", 1);
        assert_eq!(app.row_count(), 3);
    }

    #[test]
    fn a_dropped_node_moves_and_a_fold_keeps_the_selection_shown() {
        let mut doc = Document::new_empty("");
        let a = doc.outline().root_position().unwrap();
        doc.set_headline(&a, "a");
        let b = doc.outline_mut_untracked().insert_after(&a);
        doc.set_headline(&b, "b");
        let mut app = App::new(doc);
        app.drop_node(&b, &a, leolib::Place::Inside);
        let heads: Vec<(usize, String)> = app
            .rows()
            .iter()
            .map(|r| (r.depth, r.headline.clone()))
            .collect();
        assert_eq!(heads, [(0, "a".to_string()), (1, "b".to_string())]);
        assert_eq!(app.current.h(app.outline()), "b");
        let a = app.rows()[0].position.clone();
        app.drop_node(&a, &app.current.clone(), leolib::Place::After);
        assert!(app.message.contains("its own tree"));
        // Folding a away hides b, so a is selected.
        app.toggle_fold(&a);
        assert_eq!(app.current.h(app.outline()), "a");
    }

    #[test]
    fn a_menu_choice_commits_insert_and_spares_a_line_being_typed() {
        let mut app = app_with_body("x\n");
        app.handle_text("ihi");
        assert!(app.run_chosen("insert-node"));
        // The typing was committed; the new node's headline is being edited.
        assert_eq!(app.rows()[0].position.b(app.outline()), "hix\n");
        assert_eq!(app.mode, Mode::Headline);
        app.handle_text("new");
        assert!(!app.run_chosen("insert-node"));
        assert_eq!(app.rows().len(), 2);
        assert!(app.mini.as_ref().is_some_and(|m| m.buffer.ends_with("new")));
    }

    #[test]
    fn a_line_is_cut_at_every_edge_of_colour_match_and_diagnostic() {
        use crate::highlight::Span;
        let line = "let x = 1;\n";
        let spans = [Span {
            start: 0,
            end: 3,
            class: Class::Keyword,
        }];
        let marks = [(4..5, Severity::Warning), (4..9, Severity::Error)];
        let hits = [Range { start: 8, end: 9 }];
        let got: Vec<(&str, Class, bool, Option<Severity>)> = decorate(line, &spans, &hits, &marks)
            .into_iter()
            .map(|s| (s.text, s.class, s.hit, s.severity))
            .collect();
        assert_eq!(
            got,
            [
                ("let", Class::Keyword, false, None),
                (" ", Class::Plain, false, None),
                ("x", Class::Plain, false, Some(Severity::Error)),
                (" = ", Class::Plain, false, Some(Severity::Error)),
                ("1", Class::Plain, true, Some(Severity::Error)),
                (";", Class::Plain, false, None),
            ]
        );
    }

    #[test]
    fn a_diagnostic_marks_its_columns_and_the_status_line_reads_it() {
        let mut app = app_with_body("a\u{e9}cd\nxy\n");
        app.diagnostics = vec![
            BodyDiagnostic {
                row: 0,
                col: 1,
                end_row: 1,
                end_col: 1,
                severity: Severity::Error,
                message: "bad\nmore".into(),
            },
            BodyDiagnostic {
                row: 1,
                col: 2,
                end_row: 1,
                end_col: 2,
                severity: Severity::Hint,
                message: "end".into(),
            },
        ];
        let body = app.body_view(Viewport { rows: 3, cols: 20 });
        assert_eq!(body.marks(0), [(1..5, Severity::Error)]);
        // The hint has no width and sits past the last character: no mark.
        assert_eq!(body.marks(1), [(0..1, Severity::Error)]);
        assert!(app.status_text().ends_with("E: bad"));
        // While a change is typed the rows are not the server's.
        app.buffer = Some(vec!["a\n".into()]);
        assert!(app
            .body_view(Viewport { rows: 3, cols: 20 })
            .diagnostics
            .is_empty());
    }

    #[test]
    fn columns_merge_one_style_and_blank_a_cut_wide_character() {
        let cells = expand(&[("a\tb", 1), ("\u{4e2d}", 2)], 4);
        // a, three blanks to the tab stop, b, then a two-column character.
        assert_eq!(cells.last(), Some(&('\u{4e2d}', 2, 5)));
        assert_eq!(
            columns(&cells, 0, 6),
            [("a   b".to_string(), 1), (" ".to_string(), 2)]
        );
    }
}
