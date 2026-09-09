//! The body editor: vim editing over the model's own text.
//!
//! The model is authoritative for body text -- that was the point of
//! separating it from the view -- so the editor holds a working copy only for
//! as long as one change takes, and every completed change goes through
//! `Document::set_body`. See `docs/dev/tui-design.md` section 8.

pub mod change;
pub mod motion;
pub mod parse;

use change::{ordered, Change, InsertAt, Operator, Range, Register, Simple};
use motion::{apply, object_range, Kind, Motion, Pos, Target};

/// The editor's own state. The text is not here: it lives in the outline.
#[derive(Default)]
pub struct Editor {
    pub cursor: Pos,
    /// The column `j` and `k` aim for, which a short line does not change.
    pub desired_col: usize,
    /// The anchor of a VISUAL selection, and whether it is linewise.
    pub visual: Option<(Pos, Kind)>,
    pub register: Register,
    /// The last change, for `.`.
    pub last_change: Option<Change>,
    /// The last `f`/`t`, for `;` and `,`.
    pub last_find: Option<Motion>,
    /// Text typed since INSERT began, and where it began.
    insert: Option<InsertState>,
}

struct InsertState {
    at: InsertAt,
    text: String,
    count: usize,
    /// Set when `c` (or `s`, `S`, `C`) opened this session, so `.` repeats the
    /// deletion as well as the typing.
    operator: Option<(Operator, Range, usize)>,
}

impl Editor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn in_insert(&self) -> bool {
        self.insert.is_some()
    }

    /// Clamp the cursor into a buffer that may have changed underneath.
    pub fn clamp(&mut self, lines: &[String]) {
        let last = lines.len().saturating_sub(1);
        self.cursor.0 = self.cursor.0.min(last);
        let width = line_len(lines, self.cursor.0);
        let max = if self.in_insert() {
            width
        } else {
            width.saturating_sub(1)
        };
        self.cursor.1 = self.cursor.1.min(max);
    }

    // --- Motions ----------------------------------------------------------

    /// Move the cursor. `screen` is (first visible row, visible height).
    pub fn move_by(
        &mut self,
        lines: &[String],
        motion: Motion,
        count: usize,
        screen: (usize, usize),
    ) -> bool {
        let Some(target) = apply(lines, self.cursor, motion, count, screen) else {
            return false;
        };
        let keep_column = matches!(motion, Motion::Up | Motion::Down);
        self.cursor = target.pos;
        if keep_column {
            // A short line does not move the column `j` and `k` aim for.
            self.cursor.1 = self.desired_col.min(line_len(lines, self.cursor.0));
        } else {
            self.desired_col = self.cursor.1;
        }
        if matches!(motion, Motion::Find { .. }) {
            self.last_find = Some(motion);
        }
        self.clamp(lines);
        true
    }

    // --- Applying a change ------------------------------------------------

    /// Run one change against `lines`, returning the new text.
    pub fn apply_change(
        &mut self,
        lines: &[String],
        change: &Change,
        screen: (usize, usize),
    ) -> Option<Vec<String>> {
        let mut lines = lines.to_vec();
        match change {
            Change::Operator {
                count,
                operator,
                range,
            } => {
                let range = self.adjust_for_change(&lines, *operator, *range);
                let (start, end, kind) = self.resolve(&lines, range, *count, screen)?;
                self.run_operator(&mut lines, *operator, start, end, kind);
            }
            Change::OperatorInsert {
                count,
                operator,
                range,
                text,
            } => {
                let range = self.adjust_for_change(&lines, *operator, *range);
                let (start, end, kind) = self.resolve(&lines, range, *count, screen)?;
                self.run_operator(&mut lines, *operator, start, end, kind);
                insert_text(&mut lines, &mut self.cursor, text);
            }
            Change::Simple { count, edit } => self.run_simple(&mut lines, edit, *count)?,
            Change::Insert { at, text, count } => {
                self.position_for_insert(&mut lines, *at);
                for _ in 0..*count {
                    insert_text(&mut lines, &mut self.cursor, text);
                }
                // vim leaves the cursor on the last inserted character.
                self.cursor.1 = self.cursor.1.saturating_sub(1);
            }
        }
        self.clamp(&lines);
        self.last_change = Some(change.clone());
        Some(lines)
    }

    /// vim's one irregular rule: `cw` on a word behaves as `ce`, so it does
    /// not swallow the space after it. Users rely on this without knowing it
    /// is a special case.
    fn adjust_for_change(&self, lines: &[String], operator: Operator, range: Range) -> Range {
        let Range::Motion(Motion::WordForward { big }) = range else {
            return range;
        };
        if operator != Operator::Change {
            return range;
        }
        let on_blank = lines
            .get(self.cursor.0)
            .and_then(|l| l.chars().nth(self.cursor.1))
            .map(|c| c.is_whitespace())
            .unwrap_or(true);
        if on_blank {
            range
        } else {
            Range::Motion(Motion::WordEnd { big })
        }
    }

    /// The character range a motion, object or doubled operator covers.
    fn resolve(
        &self,
        lines: &[String],
        range: Range,
        count: usize,
        screen: (usize, usize),
    ) -> Option<(Pos, Pos, Kind)> {
        match range {
            Range::Line => {
                let end = (self.cursor.0 + count - 1).min(lines.len().saturating_sub(1));
                Some(((self.cursor.0, 0), (end, 0), Kind::Linewise))
            }
            Range::Object(object) => object_range(lines, self.cursor, object),
            Range::Motion(motion) => {
                let target: Target = apply(lines, self.cursor, motion, count, screen)?;
                let (a, b) = ordered(self.cursor, target.pos);
                match target.kind {
                    Kind::Linewise => Some((a, b, Kind::Linewise)),
                    Kind::Charwise => {
                        let end = if target.inclusive { (b.0, b.1 + 1) } else { b };
                        Some((a, end, Kind::Charwise))
                    }
                }
            }
            Range::Visual { rows, cols, kind } => {
                let end_row = (self.cursor.0 + rows).min(lines.len().saturating_sub(1));
                let end_col = if rows == 0 {
                    self.cursor.1 + cols
                } else {
                    cols
                };
                Some((self.cursor, (end_row, end_col), kind))
            }
        }
    }

    fn run_operator(
        &mut self,
        lines: &mut Vec<String>,
        operator: Operator,
        start: Pos,
        end: Pos,
        kind: Kind,
    ) {
        let text = extract(lines, start, end, kind);
        match operator {
            Operator::Yank => {
                self.register = Register {
                    text,
                    linewise: kind == Kind::Linewise,
                };
                self.cursor = start;
            }
            Operator::Delete | Operator::Change => {
                self.register = Register {
                    text,
                    linewise: kind == Kind::Linewise,
                };
                remove(lines, start, end, kind, operator == Operator::Change);
                self.cursor = if kind == Kind::Linewise && operator == Operator::Delete {
                    (start.0.min(lines.len().saturating_sub(1)), 0)
                } else {
                    start
                };
            }
            Operator::Indent | Operator::Unindent => {
                for row in start.0..=end.0.min(lines.len().saturating_sub(1)) {
                    let line = &mut lines[row];
                    if operator == Operator::Indent {
                        if !line.trim().is_empty() {
                            line.insert_str(0, "    ");
                        }
                    } else {
                        for _ in 0..4 {
                            if line.starts_with(' ') {
                                line.remove(0);
                            }
                        }
                    }
                }
                self.cursor = (start.0, 0);
            }
            Operator::Lowercase | Operator::Uppercase | Operator::ToggleCase => {
                map_range(lines, start, end, kind, |c| match operator {
                    Operator::Lowercase => c.to_lowercase().next().unwrap_or(c),
                    Operator::Uppercase => c.to_uppercase().next().unwrap_or(c),
                    _ => toggle_case(c),
                });
                self.cursor = start;
            }
        }
    }

    fn run_simple(&mut self, lines: &mut Vec<String>, edit: &Simple, count: usize) -> Option<()> {
        match edit {
            Simple::DeleteChar { before } => {
                for _ in 0..count {
                    let (row, col) = self.cursor;
                    let width = line_len(lines, row);
                    if *before {
                        if col == 0 {
                            break;
                        }
                        remove_char(lines, (row, col - 1));
                        self.cursor.1 -= 1;
                    } else {
                        if col >= width {
                            break;
                        }
                        remove_char(lines, (row, col));
                    }
                }
            }
            Simple::Replace(ch) => {
                let (row, col) = self.cursor;
                let mut c: Vec<char> = lines.get(row)?.chars().collect();
                for i in 0..count {
                    if col + i >= c.len() {
                        return None;
                    }
                    c[col + i] = *ch;
                }
                lines[row] = c.into_iter().collect();
                self.cursor.1 = col + count - 1;
            }
            Simple::JoinLines => {
                for _ in 0..count.max(1) {
                    let row = self.cursor.0;
                    if row + 1 >= lines.len() {
                        break;
                    }
                    let next = lines.remove(row + 1);
                    let joined = format!("{} {}", lines[row].trim_end(), next.trim_start());
                    self.cursor.1 = lines[row].trim_end().chars().count();
                    lines[row] = joined;
                }
            }
            Simple::ToggleCaseChar => {
                for _ in 0..count {
                    let (row, col) = self.cursor;
                    let mut c: Vec<char> = lines.get(row)?.chars().collect();
                    if col >= c.len() {
                        break;
                    }
                    c[col] = toggle_case(c[col]);
                    lines[row] = c.into_iter().collect();
                    self.cursor.1 = (col + 1).min(line_len(lines, row).saturating_sub(1));
                }
            }
            Simple::Put { before } => {
                if self.register.text.is_empty() {
                    return None;
                }
                for _ in 0..count {
                    self.put(lines, *before);
                }
            }
        }
        Some(())
    }

    fn put(&mut self, lines: &mut Vec<String>, before: bool) {
        let text = self.register.text.clone();
        if self.register.linewise {
            let at = if before {
                self.cursor.0
            } else {
                self.cursor.0 + 1
            };
            let new: Vec<String> = text
                .strip_suffix('\n')
                .unwrap_or(&text)
                .split('\n')
                .map(|s| s.to_string())
                .collect();
            let at = at.min(lines.len());
            for (i, line) in new.iter().enumerate() {
                lines.insert(at + i, line.clone());
            }
            self.cursor = (at, 0);
        } else {
            let (row, col) = self.cursor;
            let col = if before { col } else { col + 1 };
            let col = col.min(line_len(lines, row));
            let mut cursor = (row, col);
            insert_text(lines, &mut cursor, &text);
            self.cursor = (cursor.0, cursor.1.saturating_sub(1));
        }
    }

    fn position_for_insert(&mut self, lines: &mut Vec<String>, at: InsertAt) {
        let (row, col) = self.cursor;
        match at {
            InsertAt::Cursor => {}
            InsertAt::After => self.cursor.1 = (col + 1).min(line_len(lines, row)),
            InsertAt::LineStart => {
                let line = lines.get(row).cloned().unwrap_or_default();
                let i = line.chars().position(|c| !c.is_whitespace()).unwrap_or(0);
                self.cursor.1 = i;
            }
            InsertAt::LineEnd => self.cursor.1 = line_len(lines, row),
            InsertAt::OpenBelow => {
                lines.insert(row + 1, String::new());
                self.cursor = (row + 1, 0);
            }
            InsertAt::OpenAbove => {
                lines.insert(row, String::new());
                self.cursor = (row, 0);
            }
        }
    }

    // --- INSERT -----------------------------------------------------------

    pub fn begin_insert(&mut self, lines: &mut Vec<String>, at: InsertAt, count: usize) {
        self.position_for_insert(lines, at);
        self.insert = Some(InsertState {
            at,
            text: String::new(),
            count,
            operator: None,
        });
    }

    /// Enter INSERT as the second half of a `c` operator, which has already
    /// deleted its range. `.` then repeats both halves.
    pub fn begin_change_insert(&mut self, operator: Operator, range: Range, count: usize) {
        self.insert = Some(InsertState {
            at: InsertAt::Cursor,
            text: String::new(),
            count: 1,
            operator: Some((operator, range, count)),
        });
    }

    /// Feed one character to an INSERT session.
    pub fn insert_char(&mut self, lines: &mut Vec<String>, ch: char) {
        if let Some(state) = self.insert.as_mut() {
            state.text.push(ch);
        }
        let mut cursor = self.cursor;
        insert_text(lines, &mut cursor, &ch.to_string());
        self.cursor = cursor;
    }

    pub fn insert_newline(&mut self, lines: &mut Vec<String>) {
        if let Some(state) = self.insert.as_mut() {
            state.text.push('\n');
        }
        let (row, col) = self.cursor;
        let line = lines[row].clone();
        let i = byte_index(&line, col);
        let (head, tail) = line.split_at(i);
        lines[row] = head.to_string();
        lines.insert(row + 1, tail.to_string());
        self.cursor = (row + 1, 0);
    }

    /// Backspace in INSERT. It is part of the change, not a change of its own.
    pub fn insert_backspace(&mut self, lines: &mut Vec<String>) {
        if let Some(state) = self.insert.as_mut() {
            // A backspace that eats typed text shortens what `.` will replay.
            if state.text.pop().is_none() {
                // It ate text that was there before; `.` cannot replay that.
                state.text.clear();
            }
        }
        let (row, col) = self.cursor;
        if col > 0 {
            remove_char(lines, (row, col - 1));
            self.cursor.1 -= 1;
        } else if row > 0 {
            let line = lines.remove(row);
            self.cursor = (row - 1, line_len(lines, row - 1));
            lines[row - 1].push_str(&line);
        }
    }

    /// Finish an INSERT session, returning the change it made.
    pub fn end_insert(&mut self, lines: &mut Vec<String>) -> Option<Change> {
        let state = self.insert.take()?;
        // vim's `3ifoo<Esc>` inserts foo three times.
        for _ in 1..state.count {
            let mut cursor = self.cursor;
            insert_text(lines, &mut cursor, &state.text);
            self.cursor = cursor;
        }
        self.cursor.1 = self.cursor.1.saturating_sub(1);
        let change = match state.operator {
            Some((operator, range, count)) => Change::OperatorInsert {
                count,
                operator,
                range,
                text: state.text,
            },
            None => Change::Insert {
                at: state.at,
                text: state.text,
                count: state.count,
            },
        };
        self.last_change = Some(change.clone());
        Some(change)
    }

    /// Abandon an INSERT session without recording a change.
    pub fn cancel_insert(&mut self) {
        self.insert = None;
    }

    // --- VISUAL -----------------------------------------------------------

    pub fn start_visual(&mut self, kind: Kind) {
        self.visual = Some((self.cursor, kind));
    }

    pub fn swap_visual_ends(&mut self) {
        if let Some((anchor, kind)) = self.visual {
            self.visual = Some((self.cursor, kind));
            self.cursor = anchor;
        }
    }

    /// The selection as (start, end_exclusive, kind).
    pub fn visual_range(&self, lines: &[String]) -> Option<(Pos, Pos, Kind)> {
        let (anchor, kind) = self.visual?;
        let (a, b) = ordered(anchor, self.cursor);
        Some(match kind {
            Kind::Linewise => (a, b, Kind::Linewise),
            Kind::Charwise => (
                a,
                (b.0, (b.1 + 1).min(line_len(lines, b.0))),
                Kind::Charwise,
            ),
        })
    }

    /// Apply an operator to the selection, and record it so `.` can repeat it.
    pub fn apply_to_visual(&mut self, lines: &[String], operator: Operator) -> Option<Vec<String>> {
        let (start, end, kind) = self.visual_range(lines)?;
        let mut lines = lines.to_vec();
        self.run_operator(&mut lines, operator, start, end, kind);
        self.visual = None;
        self.last_change = Some(Change::Operator {
            count: 1,
            operator,
            range: Range::Visual {
                rows: end.0 - start.0,
                cols: end.1,
                kind,
            },
        });
        self.clamp(&lines);
        Some(lines)
    }
}

// --- Buffer helpers -------------------------------------------------------

pub fn line_len(lines: &[String], row: usize) -> usize {
    lines.get(row).map(|l| l.chars().count()).unwrap_or(0)
}

fn byte_index(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map(|(i, _)| i).unwrap_or(s.len())
}

fn toggle_case(c: char) -> char {
    if c.is_uppercase() {
        c.to_lowercase().next().unwrap_or(c)
    } else {
        c.to_uppercase().next().unwrap_or(c)
    }
}

/// The text between two positions.
fn extract(lines: &[String], start: Pos, end: Pos, kind: Kind) -> String {
    match kind {
        Kind::Linewise => {
            let last = end.0.min(lines.len().saturating_sub(1));
            let mut out = lines[start.0..=last].join("\n");
            out.push('\n');
            out
        }
        Kind::Charwise if start.0 == end.0 => {
            let line = lines.get(start.0).cloned().unwrap_or_default();
            let a = byte_index(&line, start.1);
            let b = byte_index(&line, end.1);
            line[a.min(b)..b.max(a)].to_string()
        }
        Kind::Charwise => {
            let first = lines.get(start.0).cloned().unwrap_or_default();
            let mut out = first[byte_index(&first, start.1)..].to_string();
            for line in &lines[start.0 + 1..end.0.min(lines.len())] {
                out.push('\n');
                out.push_str(line);
            }
            if end.0 < lines.len() {
                out.push('\n');
                let last = &lines[end.0];
                out.push_str(&last[..byte_index(last, end.1)]);
            }
            out
        }
    }
}

/// Delete the range. `keep_line` is `cc`, which empties the lines rather than
/// removing them.
fn remove(lines: &mut Vec<String>, start: Pos, end: Pos, kind: Kind, keep_line: bool) {
    match kind {
        Kind::Linewise => {
            let last = end.0.min(lines.len().saturating_sub(1));
            if keep_line {
                lines.drain(start.0..=last);
                lines.insert(start.0, String::new());
            } else {
                lines.drain(start.0..=last);
                if lines.is_empty() {
                    lines.push(String::new());
                }
            }
        }
        Kind::Charwise if start.0 == end.0 => {
            let line = lines[start.0].clone();
            let a = byte_index(&line, start.1);
            let b = byte_index(&line, end.1);
            lines[start.0] = format!("{}{}", &line[..a.min(b)], &line[b.max(a)..]);
        }
        Kind::Charwise => {
            let first = lines[start.0].clone();
            let head = first[..byte_index(&first, start.1)].to_string();
            let tail = if end.0 < lines.len() {
                let last = &lines[end.0];
                last[byte_index(last, end.1)..].to_string()
            } else {
                String::new()
            };
            let last = end.0.min(lines.len().saturating_sub(1));
            lines.drain(start.0..=last);
            lines.insert(start.0, format!("{head}{tail}"));
        }
    }
}

fn map_range(lines: &mut [String], start: Pos, end: Pos, kind: Kind, f: impl Fn(char) -> char) {
    let rows = start.0..=end.0.min(lines.len().saturating_sub(1));
    for row in rows {
        let c: Vec<char> = lines[row].chars().collect();
        let (a, b) = match kind {
            Kind::Linewise => (0, c.len()),
            Kind::Charwise => (
                if row == start.0 { start.1 } else { 0 },
                if row == end.0 {
                    end.1.min(c.len())
                } else {
                    c.len()
                },
            ),
        };
        let mapped: String = c
            .iter()
            .enumerate()
            .map(|(i, &ch)| if i >= a && i < b { f(ch) } else { ch })
            .collect();
        lines[row] = mapped;
    }
}

fn remove_char(lines: &mut [String], (row, col): Pos) {
    let line = lines[row].clone();
    let i = byte_index(&line, col);
    if i < line.len() {
        let mut new = line.clone();
        new.remove(i);
        lines[row] = new;
    }
}

/// Insert text at the cursor, moving it past what was inserted.
fn insert_text(lines: &mut Vec<String>, cursor: &mut Pos, text: &str) {
    for (n, part) in text.split('\n').enumerate() {
        if n > 0 {
            let (row, col) = *cursor;
            let line = lines[row].clone();
            let i = byte_index(&line, col);
            let (head, tail) = line.split_at(i);
            lines[row] = head.to_string();
            lines.insert(row + 1, tail.to_string());
            *cursor = (row + 1, 0);
        }
        if part.is_empty() {
            continue;
        }
        let (row, col) = *cursor;
        while lines.len() <= row {
            lines.push(String::new());
        }
        let i = byte_index(&lines[row], col);
        lines[row].insert_str(i, part);
        *cursor = (row, col + part.chars().count());
    }
}

/// Split a body into editable lines, and join them back.
pub fn split(body: &str) -> Vec<String> {
    if body.is_empty() {
        return vec![String::new()];
    }
    let mut lines: Vec<String> = body.split('\n').map(|s| s.to_string()).collect();
    if lines.len() > 1 && lines.last().map(|s| s.is_empty()) == Some(true) {
        lines.pop();
    }
    lines
}

pub fn join(lines: &[String]) -> String {
    if lines.len() == 1 && lines[0].is_empty() {
        return String::new();
    }
    format!("{}\n", lines.join("\n"))
}
