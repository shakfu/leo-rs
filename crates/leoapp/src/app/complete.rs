//! Completion from the language server, in INSERT.
//!
//! The servers see committed text only, and INSERT types into a working
//! copy. A completion sends the working copy first: it is put in the node's
//! body for one render and taken out again, so the undo history and the
//! outline's generation never see it.

use super::*;
use leolsp::Completion;

/// The completions on offer while a word is typed.
pub struct CompletionMenu {
    pub items: Vec<Completion>,
    /// The items matching what has been typed since they came, best first.
    pub shown: Vec<usize>,
    /// The highlighted one, an index into `shown`.
    pub selected: usize,
    /// Where the word being completed starts, in the working copy.
    pub start: (usize, usize),
}

impl CompletionMenu {
    /// The item at `i` of those shown.
    pub fn shown_item(&self, i: usize) -> Option<&Completion> {
        self.shown.get(i).map(|&k| &self.items[k])
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl App {
    /// The column where the word ending at the cursor starts.
    fn word_start(&self, lines: &[String]) -> usize {
        let (row, col) = self.editor.cursor;
        let line: Vec<char> = lines.get(row).map_or(vec![], |l| l.chars().collect());
        let mut start = col.min(line.len());
        while start > 0 && is_word(line[start - 1]) {
            start -= 1;
        }
        start
    }

    /// The character before the cursor.
    fn char_before_cursor(&self, lines: &[String]) -> Option<char> {
        let (row, col) = self.editor.cursor;
        lines
            .get(row)
            .and_then(|l| col.checked_sub(1).and_then(|c| l.chars().nth(c)))
    }

    /// Whether Tab completes here rather than indents: after a word
    /// character or a dot, in a body a server has.
    pub(super) fn completes_here(&self, lines: &[String]) -> bool {
        let served = self
            .lsp
            .as_ref()
            .is_some_and(|l| l.serves(self.current.gnx(self.outline())));
        served
            && self
                .char_before_cursor(lines)
                .is_some_and(|c| is_word(c) || c == '.')
    }

    /// Why Tab after a dot cannot complete, when no server has this body.
    /// Indenting after a dot is never what was meant, so Tab says this
    /// instead; after a word it still indents.
    pub(super) fn no_completion_after_dot(&self, lines: &[String]) -> Option<String> {
        if self.char_before_cursor(lines) != Some('.') {
            return None;
        }
        let language = self.outline().get_language(&self.current);
        let named = self.settings.servers.iter().any(|s| s.language == language);
        Some(match (self.settings.lsp, named) {
            (false, _) => "no completion: language servers are off (lsp = false)".into(),
            (true, false) => {
                format!("no completion: no language server for {language}; set lsp-{language} in the settings")
            }
            (true, true) => {
                format!("no completion: the {language} server is not serving this body; see :lsp-status")
            }
        })
    }

    /// Ask the server what could be typed at the cursor, after sending it
    /// the working copy. The answer opens the menu from `poll`.
    pub fn request_completion(&mut self) {
        let gnx = self.current.gnx(self.doc.outline()).to_string();
        let row = self.editor.cursor.0;
        match self.ask_with_working_copy(leolsp::Request::Completion) {
            Ok(()) => self.completion_asked = Some((gnx, row)),
            Err(e) => self.message = e,
        }
    }

    /// Open the menu with what a plugin completes at the cursor, if one
    /// offers anything there. True if one did.
    pub(super) fn offer_plugin_completion(&mut self) -> bool {
        let Some((start, names)) = crate::plugins::complete(self) else {
            return false;
        };
        if names.is_empty() {
            self.message = "nothing to complete".into();
            return true;
        }
        let items = names
            .into_iter()
            .map(|name| Completion {
                label: name.clone(),
                detail: None,
                kind: None,
                text: name,
                range: None,
            })
            .collect();
        self.completion = Some(CompletionMenu {
            items,
            shown: Vec::new(),
            selected: 0,
            start: (self.editor.cursor.0, start),
        });
        self.refilter_completion();
        true
    }

    /// Ask for the signature of the call being typed, after `(` or `,`.
    pub fn request_signature(&mut self) {
        if let Err(e) = self.ask_with_working_copy(leolsp::Request::SignatureHelp) {
            self.message = e;
        }
    }

    /// Send the server the working copy, then ask `request` at the cursor.
    /// The answer arrives from `poll`.
    fn ask_with_working_copy(&mut self, request: leolsp::Request) -> Result<(), String> {
        if self.lsp.is_none() {
            return Err("no language server: name one in the settings".into());
        }
        let lines = self.body_buffer();
        let v = self.current.v;
        let working = editor::join(&lines);
        let committed =
            std::mem::replace(&mut self.doc.outline_mut_untracked().node_mut(v).b, working);
        let events = match self.lsp.as_mut() {
            Some(lsp) => lsp.refresh(self.doc.outline(), &self.current),
            None => vec![],
        };
        self.doc.outline_mut_untracked().node_mut(v).b = committed;
        for event in events {
            self.lsp_event(event);
        }
        let gnx = self.current.gnx(self.doc.outline()).to_string();
        let (row, col) = self.editor.cursor;
        let lsp = self.lsp.as_mut().expect("checked");
        lsp.request(&gnx, row, col, request)
    }

    /// Whether a language server has this body.
    pub(super) fn served(&self) -> bool {
        self.lsp
            .as_ref()
            .is_some_and(|l| l.serves(self.current.gnx(self.outline())))
    }

    /// The server's answer: a menu, if the word is still being typed where
    /// it was asked about.
    pub(super) fn offer_completions(&mut self, items: Vec<Completion>) {
        let asked = self.completion_asked.take();
        let here = (
            self.current.gnx(self.outline()).to_string(),
            self.editor.cursor.0,
        );
        if self.mode != Mode::Insert || asked.as_ref() != Some(&here) {
            return;
        }
        if items.is_empty() {
            self.message = "no completions".into();
            return;
        }
        let lines = self.body_buffer();
        self.completion = Some(CompletionMenu {
            items,
            shown: Vec::new(),
            selected: 0,
            start: (here.1, self.word_start(&lines)),
        });
        self.refilter_completion();
    }

    /// Show the items that begin with what has been typed of the word,
    /// ignoring case; close the menu if none do or the cursor left the word.
    pub(super) fn refilter_completion(&mut self) {
        let lines = self.body_buffer();
        let (row, col) = self.editor.cursor;
        let Some(menu) = self.completion.as_mut() else {
            return;
        };
        if row != menu.start.0 || col < menu.start.1 {
            self.completion = None;
            return;
        }
        let typed: String = lines[row]
            .chars()
            .skip(menu.start.1)
            .take(col - menu.start.1)
            .collect::<String>()
            .to_lowercase();
        menu.shown = (0..menu.items.len())
            .filter(|&k| {
                let item = &menu.items[k];
                item.text.to_lowercase().starts_with(&typed)
                    || item.label.to_lowercase().starts_with(&typed)
            })
            .collect();
        menu.selected = menu.selected.min(menu.shown.len().saturating_sub(1));
        if menu.shown.is_empty() {
            self.completion = None;
        }
    }

    /// A key while the menu is open. True if the menu took it: the arrows
    /// or Ctrl-n and Ctrl-p select, Tab or Enter accepts, Escape closes it.
    pub(super) fn completion_key(&mut self, event: &KeyEvent) -> bool {
        let Some(menu) = self.completion.as_mut() else {
            return false;
        };
        let ctrl = event.modifiers == KeyModifiers::CONTROL;
        let last = menu.shown.len().saturating_sub(1);
        match event.code {
            KeyCode::Up => menu.selected = menu.selected.saturating_sub(1),
            KeyCode::Char('p') if ctrl => menu.selected = menu.selected.saturating_sub(1),
            KeyCode::Down => menu.selected = (menu.selected + 1).min(last),
            KeyCode::Char('n') if ctrl => menu.selected = (menu.selected + 1).min(last),
            KeyCode::Tab | KeyCode::Enter => {
                let i = menu.selected;
                self.accept_completion(i);
            }
            KeyCode::Esc => self.completion = None,
            _ => return false,
        }
        true
    }

    /// Type completion `i` of those shown over the word typed so far, as
    /// backspaces and characters, so `.` repeats it.
    pub fn accept_completion(&mut self, i: usize) {
        let Some(menu) = self.completion.take() else {
            return;
        };
        let Some(item) = menu.shown_item(i).cloned() else {
            return;
        };
        let mut lines = self.body_buffer();
        let (row, col) = self.editor.cursor;
        let gnx = self.current.gnx(self.outline()).to_string();
        let from = match &item.range {
            Some((a, _)) if a.gnx == gnx && a.row == row && a.col <= col => a.col,
            _ if menu.start.0 == row && menu.start.1 <= col => menu.start.1,
            _ => col,
        };
        for _ in from..col {
            self.editor.insert_backspace(&mut lines);
        }
        for ch in item.text.chars() {
            match ch {
                '\n' => self.editor.insert_newline(&mut lines),
                ch => self.editor.insert_char(&mut lines, ch),
            }
        }
        self.buffer = Some(lines);
        self.scroll_to_cursor();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leolsp::BodyPos;

    /// INSERT at the end of `pr` in "pr\n".
    fn typing() -> App {
        let mut doc = Document::new_empty("");
        let root = doc.outline().root_position().unwrap();
        doc.set_body(&root, "x = 1\npr\n");
        doc.clear_undo();
        let mut app = App::new(doc);
        app.focus = Focus::Body;
        app.editor.cursor = (1, 1);
        app.begin_body_edit();
        // INSERT may stand past the last character, as `a` does.
        app.editor.cursor = (1, 2);
        app
    }

    fn item(app: &App, label: &str, from: Option<usize>) -> Completion {
        let gnx = app.current.gnx(app.outline()).to_string();
        let at = |col| BodyPos {
            gnx: gnx.clone(),
            row: 1,
            col,
        };
        Completion {
            label: label.into(),
            detail: None,
            kind: None,
            text: label.into(),
            range: from.map(|c| (at(c), at(2))),
        }
    }

    fn offer(app: &mut App, items: Vec<Completion>) {
        let gnx = app.current.gnx(app.outline()).to_string();
        app.completion_asked = Some((gnx, app.editor.cursor.0));
        app.offer_completions(items);
    }

    fn key(app: &mut App, code: KeyCode) {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    /// INSERT at the end of `text`, the root's only line.
    fn typing_after(text: &str) -> App {
        let mut doc = Document::new_empty("");
        let root = doc.outline().root_position().unwrap();
        doc.set_body(&root, &format!("@language python\n{text}\n"));
        doc.clear_undo();
        let mut app = App::new(doc);
        app.focus = Focus::Body;
        app.editor.cursor = (1, 0);
        app.begin_body_edit();
        app.editor.cursor = (1, text.chars().count());
        app
    }

    #[test]
    fn tab_after_a_dot_with_no_server_says_why_and_does_not_indent() {
        let mut app = typing_after("os.");
        key(&mut app, KeyCode::Tab);
        assert_eq!(app.body_buffer()[1], "os.");
        assert_eq!(
            app.message,
            "no completion: no language server for python; set lsp-python in the settings"
        );

        let mut app = typing_after("os.");
        app.settings.lsp = false;
        key(&mut app, KeyCode::Tab);
        assert!(app.message.contains("lsp = false"), "{}", app.message);

        let mut app = typing_after("os.");
        app.settings.servers.push(leolsp::ServerConfig {
            language: "python".into(),
            command: "pylsp".into(),
        });
        key(&mut app, KeyCode::Tab);
        assert!(app.message.contains(":lsp-status"), "{}", app.message);
    }

    #[test]
    fn tab_after_a_word_with_no_server_still_indents() {
        let mut app = typing_after("os");
        key(&mut app, KeyCode::Tab);
        assert_eq!(app.body_buffer()[1], "os    ");
    }

    #[test]
    fn a_completion_replaces_the_word_and_typing_narrows_the_menu() {
        let mut app = typing();
        let items = vec![item(&app, "print", Some(0)), item(&app, "property", None)];
        offer(&mut app, items);
        assert_eq!(app.completion.as_ref().unwrap().shown.len(), 2);
        key(&mut app, KeyCode::Char('o'));
        assert_eq!(app.completion.as_ref().unwrap().shown.len(), 1);
        key(&mut app, KeyCode::Tab);
        assert!(app.completion.is_none());
        assert_eq!(app.mode, Mode::Insert);
        assert_eq!(app.body_buffer()[1], "property");
        assert_eq!(app.editor.cursor, (1, 8));
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.current.b(app.outline()), "x = 1\nproperty\n");
    }

    #[test]
    fn the_arrows_select_and_escape_closes_only_the_menu() {
        let mut app = typing();
        let items = vec![item(&app, "print", Some(0)), item(&app, "property", None)];
        offer(&mut app, items);
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.body_buffer()[1], "property");
        let more = vec![item(&app, "propertyx", None)];
        offer(&mut app, more);
        key(&mut app, KeyCode::Esc);
        assert!(app.completion.is_none());
        assert_eq!(app.mode, Mode::Insert);
        // A word nothing begins with closes it.
        let more = vec![item(&app, "propertyx", None)];
        offer(&mut app, more);
        key(&mut app, KeyCode::Char('z'));
        assert!(app.completion.is_none());
    }

    #[test]
    fn tab_indents_without_a_server_and_an_answer_after_typing_moved_on_is_dropped() {
        let mut app = typing();
        key(&mut app, KeyCode::Tab);
        assert_eq!(app.body_buffer()[1], "pr    ");
        let items = vec![item(&app, "print", None)];
        // Asked about another row.
        let gnx = app.current.gnx(app.outline()).to_string();
        app.completion_asked = Some((gnx, 0));
        app.offer_completions(items);
        assert!(app.completion.is_none());
    }
}
