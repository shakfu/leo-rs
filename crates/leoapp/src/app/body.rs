//! The body pane: the vim grammar in NORMAL and VISUAL, and INSERT.

use super::*;

impl App {
    /// Text the terminal says was pasted, which is text and never keys.
    ///
    /// In the body it is inserted at the cursor as one change, entering and
    /// leaving INSERT if it was not already on. A one-line input takes it with
    /// its line breaks as spaces. The outline has nowhere to put text.
    pub fn handle_paste(&mut self, text: &str) {
        self.message.clear();
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        match self.mode {
            Mode::Headline | Mode::Command | Mode::Search => {
                let Some(mini) = self.mini.as_mut() else {
                    return;
                };
                for ch in text.trim_end_matches('\n').chars() {
                    mini.insert(if ch == '\n' { ' ' } else { ch });
                }
                self.preview_search();
            }
            Mode::Insert => self.insert_text(&text),
            Mode::Normal if self.focus == Focus::Body => {
                self.begin_body_edit();
                self.insert_text(&text);
                self.insert_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            }
            _ => {
                self.message =
                    "paste: nothing takes text here; open a body or a headline".to_string()
            }
        }
    }

    /// Type `text` into the INSERT session, each character as itself.
    fn insert_text(&mut self, text: &str) {
        let mut lines = self.buffer.clone().unwrap_or_else(|| self.body_buffer());
        for ch in text.chars() {
            match ch {
                '\n' => self.editor.insert_newline(&mut lines),
                ch => self.editor.insert_char(&mut lines, ch),
            }
        }
        self.buffer = Some(lines);
        self.scroll_to_cursor();
    }

    /// Enter INSERT at the cursor, as `i` does.
    pub fn begin_body_edit(&mut self) {
        self.focus = Focus::Body;
        let mut lines = self.body_buffer();
        self.editor.clamp(&lines);
        self.editor.begin_insert(&mut lines, InsertAt::Cursor, 1);
        self.buffer = Some(lines);
        self.mode = Mode::Insert;
    }

    /// The body's own text: the working copy if one is live, else the model.
    pub fn body_buffer(&self) -> Vec<String> {
        match &self.buffer {
            Some(lines) => lines.clone(),
            None => editor::split(self.current.b(self.outline())),
        }
    }

    /// `body_buffer().len()`, without splitting the body.
    pub fn body_line_count(&self) -> usize {
        match &self.buffer {
            Some(lines) => lines.len(),
            None => editor::line_count(self.current.b(self.outline())),
        }
    }

    /// Write the body back as one change, which is one undo bead.
    pub(super) fn commit_body(&mut self, lines: &[String]) {
        let p = self.current.clone();
        let text = editor::join(lines);
        self.doc.set_body(&p, &text);
        self.buffer = None;
    }

    /// The single-key NORMAL binding the body shares with the outline.
    fn body_binding(&self, key: Key) -> Option<&'static str> {
        bindings::for_context(Mode::Normal, Focus::Body)
            .find(|b| keys::parse(b.keys) == [key])
            .map(|b| b.command)
    }

    /// NORMAL and VISUAL with body focus: the vim grammar.
    pub(super) fn body_key(&mut self, event: KeyEvent) {
        let key = Key::from_event(event);
        self.parser.visual = self.mode == Mode::Visual;
        let action = self.parser.feed(key);
        let lines = self.body_buffer();
        let screen = (self.body_scroll, self.body_height);
        match action {
            Action::Pending => {}
            Action::Unknown => self.message = "no such command".to_string(),
            Action::GotoDefinition => self.run("open-url-under-cursor", 1),
            Action::Run(command, count) => self.run(command, count),
            Action::Unbound(count) => match self.body_binding(key) {
                Some(command) => self.run(command, count),
                None => self.message = "no such command".to_string(),
            },
            Action::Move(motion, count) => {
                self.editor.move_by(&lines, motion, count, screen);
                self.scroll_to_cursor();
            }
            Action::Operate {
                operator,
                range,
                count,
            } => self.operate(operator, range, count, screen),
            Action::Edit(edit, count) => {
                let change = Change::Simple { count, edit };
                self.run_change(change, screen);
            }
            Action::Insert(at, count) => {
                let mut lines = lines;
                self.editor.clamp(&lines);
                self.editor.begin_insert(&mut lines, at, count);
                self.buffer = Some(lines);
                self.mode = Mode::Insert;
            }
            Action::Visual { linewise } => {
                self.editor.start_visual(if linewise {
                    Kind::Linewise
                } else {
                    Kind::Charwise
                });
                self.mode = Mode::Visual;
            }
            Action::SwapEnds => self.editor.swap_visual_ends(),
            Action::Repeat(count) => {
                let Some(change) = self.editor.last_change.clone() else {
                    self.message = "nothing to repeat".to_string();
                    return;
                };
                // vim: a count replaces the change's own, and it runs once.
                let change = match count {
                    Some(n) => change.with_count(n),
                    None => change,
                };
                self.run_change(change, screen);
            }
            Action::Undo(count) => self.run("undo", count),
            Action::Redo(count) => self.run("redo", count),
            Action::SearchForward => self.open_mini(MiniKind::SearchForward, String::new()),
            Action::SearchBackward => self.open_mini(MiniKind::SearchBackward, String::new()),
            Action::FindNext(count) => self.repeat_search(true, count),
            Action::FindPrev(count) => self.repeat_search(false, count),
            Action::Pane { widen, count } => {
                self.run(if widen { "grow-pane" } else { "shrink-pane" }, count)
            }
            Action::RepeatFind { reverse, count } => {
                let Some(motion) = self.editor.last_find else {
                    return;
                };
                let motion = if reverse {
                    reverse_find(motion)
                } else {
                    motion
                };
                self.editor.move_by(&lines, motion, count, screen);
                self.scroll_to_cursor();
            }
            Action::Command => self.open_mini(MiniKind::Command, String::new()),
            Action::FocusTree => self.focus = Focus::Tree,
            Action::Escape => {
                if self.mode == Mode::Visual {
                    self.editor.visual = None;
                    self.mode = Mode::Normal;
                } else {
                    self.focus = Focus::Tree;
                }
            }
        }
    }

    /// Apply an operator, either to a selection or to a motion's range.
    fn operate(&mut self, operator: Operator, range: Range, count: usize, screen: (usize, usize)) {
        let lines = self.body_buffer();
        let result = if self.mode == Mode::Visual {
            let out = self.editor.apply_to_visual(&lines, operator);
            self.mode = Mode::Normal;
            out
        } else {
            self.editor.apply_change(
                &lines,
                &Change::Operator {
                    count,
                    operator,
                    range,
                },
                screen,
            )
        };
        let Some(new_lines) = result else {
            self.message = "nothing to do".to_string();
            return;
        };
        if operator.enters_insert() {
            // `c` deletes, then leaves the editor in INSERT: one change, made
            // of the deletion and whatever is typed next, so `.` repeats both.
            self.editor.begin_change_insert(operator, range, count);
            self.buffer = Some(new_lines);
            self.mode = Mode::Insert;
            return;
        }
        self.commit_body(&new_lines);
        self.scroll_to_cursor();
    }

    fn run_change(&mut self, change: Change, screen: (usize, usize)) {
        let lines = self.body_buffer();
        let Some(new_lines) = self.editor.apply_change(&lines, &change, screen) else {
            self.message = "nothing to do".to_string();
            return;
        };
        self.commit_body(&new_lines);
        self.scroll_to_cursor();
    }

    /// Keep the cursor on screen without recentring on every keypress.
    pub(super) fn scroll_to_cursor(&mut self) {
        let row = self.editor.cursor.0;
        let height = self.body_height.max(1);
        if row < self.body_scroll {
            self.body_scroll = row;
        } else if row >= self.body_scroll + height {
            self.body_scroll = row + 1 - height;
        }
    }

    /// INSERT. Escape commits the change.
    pub(super) fn insert_key(&mut self, event: KeyEvent) {
        if self.completion_key(&event) {
            return;
        }
        let mut lines = self.buffer.clone().unwrap_or_else(|| self.body_buffer());
        match event.code {
            // Tab completes after a word where a server can; else it indents.
            KeyCode::Tab if self.completes_here(&lines) => return self.request_completion(),
            KeyCode::Char('n') if event.modifiers == KeyModifiers::CONTROL => {
                return self.request_completion();
            }
            KeyCode::Esc => {
                self.completion = None;
                self.editor.end_insert(&mut lines);
                self.commit_body(&lines);
                self.mode = Mode::Normal;
                self.editor.clamp(&lines);
                return;
            }
            KeyCode::Enter => self.editor.insert_newline(&mut lines),
            KeyCode::Backspace => self.editor.insert_backspace(&mut lines),
            KeyCode::Tab => {
                for _ in 0..4 {
                    self.editor.insert_char(&mut lines, ' ');
                }
            }
            KeyCode::Left => {
                self.editor.cursor.1 = self.editor.cursor.1.saturating_sub(1);
            }
            KeyCode::Right => {
                let max = editor::line_len(&lines, self.editor.cursor.0);
                self.editor.cursor.1 = (self.editor.cursor.1 + 1).min(max);
            }
            KeyCode::Up => {
                self.editor.cursor.0 = self.editor.cursor.0.saturating_sub(1);
                self.editor.clamp(&lines);
            }
            KeyCode::Down => {
                let last = lines.len().saturating_sub(1);
                self.editor.cursor.0 = (self.editor.cursor.0 + 1).min(last);
                self.editor.clamp(&lines);
            }
            KeyCode::Home => self.editor.cursor.1 = 0,
            KeyCode::End => self.editor.cursor.1 = editor::line_len(&lines, self.editor.cursor.0),
            // vim's Ctrl-w and Ctrl-u, as backspaces so `.` replays them.
            KeyCode::Char('w' | 'u') if event.modifiers == KeyModifiers::CONTROL => {
                let (row, col) = self.editor.cursor;
                let n = match event.code {
                    _ if col == 0 => 1,
                    KeyCode::Char('w') => col - editor::word_start_before(&lines[row], col),
                    _ => col,
                };
                for _ in 0..n {
                    self.editor.insert_backspace(&mut lines);
                }
            }
            KeyCode::Char(_) if keys::is_chord(event.modifiers) => {}
            KeyCode::Char(ch) => self.editor.insert_char(&mut lines, ch),
            _ => {}
        }
        self.buffer = Some(lines);
        self.scroll_to_cursor();
        self.refilter_completion();
    }

    /// The section branch of Leo's `open-url-under-cursor`: select the node
    /// defining the `<< name >>` alone on the cursor's line. It matches as the
    /// `@file` writer does, where Leo's click compares headlines exactly.
    pub fn goto_section_definition(&mut self) {
        let lines = self.body_buffer();
        let line = lines.get(self.editor.cursor.0).map_or("", |l| l.trim());
        if !(line.len() > 4 && line.starts_with("<<") && line.ends_with(">>")) {
            self.message = "no section reference on this line".to_string();
            return;
        }
        let o = self.outline();
        let found = self
            .current
            .subtree(o)
            .into_iter()
            .find(|p| p.match_headline(o, line) && !p.is_at_ignore_node(o));
        match found {
            Some(p) => self.select(p),
            None => self.message = format!("undefined section: {line}"),
        }
    }

    /// Leo's `extract`: the VISUAL lines, or the cursor's line, become the
    /// current node's first child. The node stays selected, as in Leo.
    pub fn extract(&mut self) {
        let row = self.editor.cursor.0;
        let (first, last) = match self.editor.visual.take() {
            Some((anchor, _)) => (anchor.0.min(row), anchor.0.max(row)),
            None => (row, row),
        };
        if self.mode == Mode::Visual {
            self.mode = Mode::Normal;
        }
        let p = self.current.clone();
        match self.doc.extract(&p, first, last) {
            Some(child) => {
                self.message = format!("extracted: {}", child.h(self.outline()));
                self.editor.cursor = (first, 0);
                let lines = self.body_buffer();
                self.editor.clamp(&lines);
            }
            None => self.message = "nothing to extract".to_string(),
        }
    }

    /// Leo's `goto-global-line`: select the node that writes line `n` of the
    /// selection's external file, with the body cursor on that line.
    pub fn goto_global_line(&mut self, n: usize) {
        match leolib::goto::find_file_line(self.outline(), &self.current, n) {
            Some((p, row)) => {
                self.select(p);
                self.focus = Focus::Body;
                self.editor.cursor = (row, 0);
                let lines = self.body_buffer();
                self.editor.clamp(&lines);
                self.scroll_to_cursor();
                self.message = format!("goto-global-line found: {n}");
            }
            None => self.message = format!("goto-global-line not found: {n}"),
        }
    }

    /// Leo's `show-file-line`, the reverse: the line of the external file
    /// that the cursor's line is written to.
    pub fn show_file_line(&mut self) {
        let row = self.editor.cursor.0;
        self.message = match leolib::goto::file_line(self.outline(), &self.current, row) {
            Some(n) => format!("line {n}"),
            None => "this line is in no external file".to_string(),
        };
    }

    /// Leo's `reformat-paragraph`: wrap the paragraph at the cursor, or at the
    /// start of the VISUAL lines, to `@pagewidth`, and move to the next one.
    pub fn reformat_paragraph(&mut self) {
        let mut row = self.editor.cursor.0;
        if let Some((anchor, _)) = self.editor.visual.take() {
            row = row.min(anchor.0);
        }
        if self.mode == Mode::Visual {
            self.mode = Mode::Normal;
        }
        let p = self.current.clone();
        match self.doc.reformat_paragraph(&p, row) {
            Some(next) => {
                self.editor.cursor = (next, 0);
                let lines = self.body_buffer();
                self.editor.clamp(&lines);
                self.scroll_to_cursor();
            }
            None => self.message = "no paragraph here".to_string(),
        }
    }
}

/// `;` and `,` differ only in direction.
fn reverse_find(motion: crate::editor::motion::Motion) -> crate::editor::motion::Motion {
    match motion {
        crate::editor::motion::Motion::Find { ch, forward, till } => {
            crate::editor::motion::Motion::Find {
                ch,
                forward: !forward,
                till,
            }
        }
        other => other,
    }
}
