//! Language servers: keeping them in step with the outline, and acting on
//! what they send back. `leolsp` does the protocol and the position mapping.
//!
//! The servers see committed text only. While INSERT types into the working
//! copy nothing is sent, as neovim's default `update_in_insert = false`; the
//! change goes out once Escape commits it.

use std::time::{Duration, Instant};

use leolsp::{BodyEdit, Event, Request, Severity, Target};

use super::*;

/// How long the outline must be still before the servers are sent it.
const DEBOUNCE: Duration = Duration::from_millis(150);

impl App {
    /// Sync the servers and act on what they sent. True if anything changed
    /// that a front end draws. Call it often: every frame, or every tick.
    pub fn poll(&mut self) -> bool {
        // A partial colouring whose pause is over: the next draw makes it
        // whole, so ask for one.
        let recolour = self.colouring.due_in().is_some_and(|d| d.is_zero());
        let called = self.poll_mcp();
        let Some(lsp) = self.lsp.as_mut() else {
            return recolour || called;
        };
        let now = (self.doc.outline().generation, self.current.v);
        let moved = self.lsp_synced.map(|s| s.1) != Some(now.1);
        let edited = self.lsp_synced.map(|s| s.0) != Some(now.0);
        let mut events = Vec::new();
        let due = edited
            && self
                .lsp_dirty_since
                .get_or_insert_with(Instant::now)
                .elapsed()
                >= DEBOUNCE;
        if moved || due {
            events = lsp.sync(self.doc.outline(), &self.current);
            self.lsp_synced = Some(now);
            self.lsp_dirty_since = None;
        }
        events.extend(lsp.poll(self.doc.outline()));
        let changed = moved || !events.is_empty();
        for event in events {
            self.lsp_event(event);
        }
        if changed {
            self.refresh_diagnostics();
        }
        changed || recolour || called
    }

    /// Start the language servers the settings name, or none if they turn
    /// them off; servers already running stop. Their workspace is the
    /// outline's directory. `wake` is called when a server sends a message.
    pub fn set_lsp(&mut self, settings: &crate::config::Config, wake: leolsp::server::Wake) {
        self.lsp = None;
        self.diagnostics.clear();
        self.lsp_synced = None;
        if !settings.lsp || settings.servers.is_empty() {
            return;
        }
        let file = &self.outline().file_name;
        let root = match file.is_empty() {
            true => std::env::current_dir().ok(),
            false => std::path::absolute(file)
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf())),
        };
        let root = root.unwrap_or_else(|| ".".into());
        self.lsp = Some(leolsp::Lsp::new(settings.servers.clone(), root, wake));
    }

    /// When `poll` next has work it cannot do yet: the end of a debounce,
    /// or of the pause before a partial colouring is made whole.
    pub fn poll_after(&self) -> Option<Duration> {
        let sync = self
            .lsp_dirty_since
            .map(|since| DEBOUNCE.saturating_sub(since.elapsed()));
        [sync, self.colouring.due_in()].into_iter().flatten().min()
    }

    fn refresh_diagnostics(&mut self) {
        self.diagnostics = match &self.lsp {
            Some(lsp) => lsp.diagnostics(self.current.gnx(self.doc.outline())),
            None => Vec::new(),
        };
    }

    /// Send the outline now, so a request is answered about this text.
    fn sync_now(&mut self) {
        let Some(lsp) = self.lsp.as_mut() else { return };
        let events = lsp.sync(self.doc.outline(), &self.current);
        self.lsp_synced = Some((self.doc.outline().generation, self.current.v));
        self.lsp_dirty_since = None;
        for event in events {
            self.lsp_event(event);
        }
    }

    /// Ask about the body cursor. The answer arrives through `poll`.
    pub fn lsp_request(&mut self, request: Request) {
        if self.lsp.is_none() {
            self.message =
                "no language server: name one in the settings, as lsp-python = \"pylsp\"".into();
            return;
        }
        self.sync_now();
        let gnx = self.current.gnx(self.doc.outline()).to_string();
        let (row, col) = self.editor.cursor;
        let lsp = self.lsp.as_mut().expect("checked");
        if let Err(e) = lsp.request(&gnx, row, col, request) {
            self.message = e;
        }
    }

    fn lsp_event(&mut self, event: Event) {
        // An answer that arrives while a line or a change is being typed
        // would move the cursor or the text under the typing.
        let idle = self.mode == Mode::Normal && self.buffer.is_none();
        match event {
            Event::Diagnostics => {}
            Event::Message(m) => self.message = m,
            Event::Hover(lines) if lines.is_empty() => self.message = "no hover information".into(),
            Event::Hover(lines) if idle => {
                self.help_scroll = 0;
                self.overlay = Some(("hover".to_string(), lines));
                self.mode = Mode::Help;
            }
            Event::Definition(targets) if idle => match targets.into_iter().next() {
                None => self.message = "no definition found".into(),
                Some(Target::Body(at)) => self.go_to_body(&at.gnx, at.row, at.col),
                Some(Target::File { path, line, .. }) => {
                    self.message = format!(
                        "definition at {}:{}, in a file this outline does not hold",
                        path.display(),
                        line + 1
                    )
                }
            },
            Event::Rename(Err(e)) => self.message = format!("rename refused: {e}"),
            Event::Rename(Ok(edits)) if idle => self.apply_edits(edits),
            Event::Hover(_) | Event::Definition(_) | Event::Rename(_) => {
                self.message = "a language server answered while you were typing; ask again".into()
            }
        }
    }

    /// Select node `gnx`, this position of it if it is current, and put the
    /// body cursor at row, col.
    pub fn go_to_body(&mut self, gnx: &str, row: usize, col: usize) {
        let o = self.doc.outline();
        let target = match self.current.gnx(o) == gnx {
            true => Some(self.current.clone()),
            false => o.all_positions().into_iter().find(|p| p.gnx(o) == gnx),
        };
        let Some(p) = target else {
            self.message = "the definition's node is gone".into();
            return;
        };
        self.select(p);
        self.focus = Focus::Body;
        self.editor.cursor = (row, col);
        self.editor.desired_col = col;
        let lines = self.body_buffer();
        self.editor.clamp(&lines);
        self.scroll_to_cursor();
    }

    /// Apply a server's edits as one undo step, every body or none.
    fn apply_edits(&mut self, edits: Vec<BodyEdit>) {
        let o = self.doc.outline();
        let mut bodies: Vec<(Position, String, Vec<BodyEdit>)> = Vec::new();
        for edit in edits {
            match bodies
                .iter_mut()
                .find(|(p, _, _)| p.gnx(o) == edit.start.gnx)
            {
                Some((_, _, list)) => list.push(edit),
                None => {
                    let found = o
                        .all_positions()
                        .into_iter()
                        .find(|p| p.gnx(o) == edit.start.gnx);
                    let Some(p) = found else {
                        self.message = "rename refused: a node it edits is gone".into();
                        return;
                    };
                    let text = p.b(o).to_string();
                    bodies.push((p, text, vec![edit]));
                }
            }
        }
        let mut changed = Vec::new();
        for (p, text, mut list) in bodies {
            // Last first, so each edit's offsets are still those of `text`.
            list.sort_by_key(|e| std::cmp::Reverse((e.start.row, e.start.col)));
            let mut text = text;
            for e in list {
                let (Some(a), Some(b)) = (
                    offset(&text, e.start.row, e.start.col),
                    offset(&text, e.end.row, e.end.col),
                ) else {
                    self.message = "rename refused: an edit is outside its body".into();
                    return;
                };
                if a > b {
                    self.message = "rename refused: an edit ends before it starts".into();
                    return;
                }
                text.replace_range(a..b, &e.text);
            }
            changed.push((p, text));
        }
        let n = changed.len();
        self.doc.begin_group("rename");
        for (p, text) in changed {
            self.doc.set_body(&p, &text);
        }
        self.doc.end_group();
        let lines = self.body_buffer();
        self.editor.clamp(&lines);
        self.message = format!("renamed in {}", plural(n, "node"));
    }

    /// `]d` and `[d`: the next or previous diagnostic in this body, round
    /// past the end, as neovim's.
    pub fn next_diagnostic(&mut self, forward: bool, count: usize) {
        if self.diagnostics.is_empty() {
            self.message = "no diagnostics".into();
            return;
        }
        let mut at = self.editor.cursor;
        for _ in 0..count.max(1) {
            let starts = self.diagnostics.iter().map(|d| (d.row, d.col));
            at = match forward {
                true => starts
                    .clone()
                    .find(|s| *s > at)
                    .or_else(|| starts.clone().next()),
                false => starts
                    .clone()
                    .rev()
                    .find(|s| *s < at)
                    .or_else(|| starts.clone().next_back()),
            }
            .expect("not empty");
        }
        self.focus = Focus::Body;
        self.editor.cursor = at;
        self.editor.desired_col = at.1;
        self.scroll_to_cursor();
        if let Some(d) = self.diagnostic_at(at.0) {
            self.message = diagnostic_line(d);
        }
    }

    /// The first diagnostic on body row `row`.
    pub fn diagnostic_at(&self, row: usize) -> Option<&leolsp::BodyDiagnostic> {
        self.diagnostics
            .iter()
            .find(|d| d.row <= row && row <= d.end_row)
    }

    /// `:lsp-diagnostics`: this body's diagnostics, in the help overlay.
    pub fn show_diagnostics(&mut self) {
        let lines = match self.diagnostics.is_empty() {
            true => vec!["no diagnostics".to_string()],
            false => self
                .diagnostics
                .iter()
                .map(|d| format!("{}:{}  {}", d.row + 1, d.col + 1, diagnostic_line(d)))
                .collect(),
        };
        self.help_scroll = 0;
        self.overlay = Some(("diagnostics".to_string(), lines));
        self.mode = Mode::Help;
    }
}

/// `E: message`, with the severity's letter, as the status line shows it.
pub fn diagnostic_line(d: &leolsp::BodyDiagnostic) -> String {
    let letter = match d.severity {
        Severity::Error => 'E',
        Severity::Warning => 'W',
        Severity::Information => 'I',
        Severity::Hint => 'H',
    };
    let first = d.message.lines().next().unwrap_or("");
    format!("{letter}: {first}")
}

/// The byte offset of row, character column col in `text`; a column past the
/// end of its line is the line's end.
fn offset(text: &str, row: usize, col: usize) -> Option<usize> {
    let mut start = 0;
    for (i, line) in text.split_inclusive('\n').enumerate() {
        if i == row {
            let body = line.trim_end_matches('\n');
            let at = body.char_indices().nth(col).map_or(body.len(), |(b, _)| b);
            return Some(start + at);
        }
        start += line.len();
    }
    // The row after a final newline is the end of the text.
    (row == text.split_inclusive('\n').count() && col == 0).then_some(text.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use leolsp::{BodyDiagnostic, BodyPos};

    fn app(body: &str) -> App {
        let mut doc = Document::new_empty("");
        let root = doc.outline().root_position().unwrap();
        doc.set_body(&root, body);
        doc.clear_undo();
        App::new(doc)
    }

    fn pos(app: &App, row: usize, col: usize) -> BodyPos {
        BodyPos {
            gnx: app.current.gnx(app.outline()).to_string(),
            row,
            col,
        }
    }

    fn diag(row: usize, col: usize) -> BodyDiagnostic {
        BodyDiagnostic {
            row,
            col,
            end_row: row,
            end_col: col + 1,
            severity: Severity::Warning,
            message: format!("at {row}"),
        }
    }

    #[test]
    fn an_offset_counts_characters_and_stops_at_the_line_end() {
        let text = "ab\nc\u{e9}d\n";
        assert_eq!(offset(text, 0, 1), Some(1));
        assert_eq!(offset(text, 1, 2), Some(6));
        assert_eq!(offset(text, 1, 99), Some(7));
        assert_eq!(offset(text, 2, 0), Some(8));
        assert_eq!(offset(text, 3, 0), None);
    }

    #[test]
    fn edits_apply_as_one_undo_step() {
        let mut app = app("def f():\n    return f\n");
        let edits = vec![
            BodyEdit {
                start: pos(&app, 0, 4),
                end: pos(&app, 0, 5),
                text: "go".into(),
            },
            BodyEdit {
                start: pos(&app, 1, 11),
                end: pos(&app, 1, 12),
                text: "go".into(),
            },
        ];
        app.lsp_event(Event::Rename(Ok(edits)));
        assert_eq!(app.current.b(app.outline()), "def go():\n    return go\n");
        assert_eq!(app.message, "renamed in 1 node");
        app.doc.undo();
        assert_eq!(app.current.b(app.outline()), "def f():\n    return f\n");
    }

    #[test]
    fn an_answer_while_typing_is_not_acted_on() {
        let mut app = app("x\n");
        app.mode = Mode::Insert;
        app.buffer = Some(vec!["x\n".into()]);
        app.lsp_event(Event::Hover(vec!["doc".into()]));
        assert_eq!(app.mode, Mode::Insert);
        assert!(app.message.contains("ask again"));
    }

    #[test]
    fn a_hover_opens_in_the_overlay() {
        let mut app = app("x\n");
        app.lsp_event(Event::Hover(vec!["int x".into()]));
        assert_eq!(app.mode, Mode::Help);
        assert_eq!(app.overlay, Some(("hover".into(), vec!["int x".into()])));
    }

    #[test]
    fn diagnostics_are_walked_round_the_body() {
        let mut app = app("a\nb\nc\nd\n");
        app.diagnostics = vec![diag(1, 0), diag(3, 0)];
        app.next_diagnostic(true, 1);
        assert_eq!(app.editor.cursor, (1, 0));
        assert_eq!(app.message, "W: at 1");
        app.next_diagnostic(true, 2);
        assert_eq!(app.editor.cursor, (1, 0));
        app.next_diagnostic(false, 1);
        assert_eq!(app.editor.cursor, (3, 0));
        app.diagnostics.clear();
        app.next_diagnostic(true, 1);
        assert_eq!(app.message, "no diagnostics");
    }

    #[test]
    fn without_a_server_a_request_says_how_to_name_one() {
        let mut app = app("x\n");
        app.lsp_request(Request::Hover);
        assert!(app.message.contains("lsp-python"));
        assert!(!app.poll());
    }
}
