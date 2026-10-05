//! The one-line prompt at the bottom of the screen: headline edits, `:` and
//! `/` input, yes/no questions, and the search and theme previews they drive.

use super::*;

impl App {
    /// The prompt drawn before the minibuffer's text.
    pub fn mini_label(&self) -> String {
        let Some(mini) = &self.mini else {
            return String::new();
        };
        match (mini.kind, self.pending_overwrite.as_slice()) {
            (MiniKind::ConfirmOverwrite, [p]) => {
                let path = self.outline().full_path(p);
                let name = leolib::util::short_file_name(&path);
                match self.outline().changed_on_disk(&path) {
                    true => {
                        format!("overwrite {name}, which changed on disk since it was read? (y/n) ")
                    }
                    false => format!("overwrite {name}, which this outline has not read? (y/n) "),
                }
            }
            (MiniKind::ConfirmOverwrite, files) => format!(
                "overwrite {} files this outline has not read, or that changed on disk? (y/n) ",
                files.len()
            ),
            (MiniKind::ConfirmRead, _) if self.pending_read.0.len() == 1 => {
                let name = self.pending_read.0[0].h(self.outline()).to_string();
                format!("discard edits not written to {name}? (y/n) ")
            }
            (MiniKind::ConfirmQuit, _) => format!("{}. quit anyway? (y/n) ", self.unsaved_work()),
            (kind, _) => kind.label().to_string(),
        }
    }

    pub fn begin_headline_edit(&mut self) {
        let text = self.current.h(self.outline()).to_string();
        self.open_mini(MiniKind::Headline, text);
    }

    /// Open the line at the bottom of the screen, and enter its mode.
    pub fn open_mini(&mut self, kind: MiniKind, text: String) {
        self.mode = match kind {
            MiniKind::Command => Mode::Command,
            MiniKind::SearchForward | MiniKind::SearchBackward => Mode::Search,
            MiniKind::ConfirmQuit
            | MiniKind::ConfirmOverwrite
            | MiniKind::ConfirmSave
            | MiniKind::ConfirmRead => Mode::Confirm,
            _ => Mode::Headline,
        };
        if kind.is_search() {
            self.search_origin = Some(SearchOrigin {
                start: self.search_cursor(),
                focus: self.focus,
                cursor: self.editor.cursor,
                scroll: self.body_scroll,
                hlsearch: self.hlsearch.clone(),
                last_hit: self.last_hit.clone(),
                expanded: self.outline().expanded().clone(),
            });
        }
        if kind == MiniKind::Command && self.theme_names.is_empty() {
            self.theme_names = Rc::new(crate::theme::names());
        }
        self.mini = Some(Minibuffer::new(kind, text));
    }

    /// Every mode whose keys are text: the headline, `:` and `/`.
    pub(super) fn mini_key(&mut self, event: KeyEvent) {
        let Some(mini) = self.mini.as_mut() else {
            self.mode = Mode::Normal;
            return;
        };
        match event.code {
            KeyCode::Esc => {
                if mini.kind == MiniKind::Command {
                    mini.cancel_completion();
                    // As vim: a `:` line ends VISUAL, run or not.
                    self.editor.visual = None;
                }
                self.restore_theme();
                self.finish_mini(false)
            }
            KeyCode::Enter => self.finish_mini(true),
            KeyCode::Tab | KeyCode::BackTab => {
                let backwards = event.code == KeyCode::BackTab;
                let names = Rc::clone(&self.theme_names);
                if let Some(mini) = self.mini.as_mut() {
                    mini.complete(backwards, &names);
                }
                self.preview_theme();
            }
            KeyCode::Backspace => {
                mini.backspace();
                self.preview_search();
            }
            KeyCode::Delete => {
                mini.delete();
                self.preview_search();
            }
            KeyCode::Left => mini.move_cursor(-1),
            KeyCode::Right => mini.move_cursor(1),
            KeyCode::Home => mini.home(),
            KeyCode::End => mini.end(),
            KeyCode::Up | KeyCode::Down => {
                let back = event.code == KeyCode::Up;
                let names = Rc::clone(&self.theme_names);
                // While a drop-down is on screen the arrows move through it.
                // It is what the eye is on, and Escape closes it.
                if self.mini.as_ref().is_some_and(|m| m.menu(&names).is_open()) {
                    if let Some(mini) = self.mini.as_mut() {
                        mini.select(back, &names);
                    }
                    self.preview_theme();
                    return;
                }
                let Some(mini) = self.mini.as_ref() else {
                    return;
                };
                let kind = mini.kind;
                let entries = match kind {
                    MiniKind::Command => self.command_history.clone(),
                    _ if kind.is_search() => self.search_history.clone(),
                    _ => Vec::new(),
                };
                if let Some(mini) = self.mini.as_mut() {
                    mini.history(&entries, back);
                }
            }
            KeyCode::Char('w' | 'u') if event.modifiers == KeyModifiers::CONTROL => {
                let n = match event.code {
                    KeyCode::Char('w') => {
                        mini.cursor - editor::word_start_before(&mini.buffer, mini.cursor)
                    }
                    _ => mini.cursor,
                };
                for _ in 0..n {
                    mini.backspace();
                }
                self.preview_search();
            }
            KeyCode::Char(_) if keys::is_chord(event.modifiers) => {}
            KeyCode::Char(ch) => {
                mini.insert(ch);
                self.preview_search();
            }
            _ => {}
        }
    }

    /// Show the theme the `:` line has landed on, as `/` shows a match.
    ///
    /// Only a chosen completion previews. Typing `theme onedar` should not
    /// keep failing to load a theme and saying so.
    fn preview_theme(&mut self) {
        let Some(mini) = self.mini.as_ref() else {
            return;
        };
        if mini.kind != MiniKind::Command {
            return;
        }
        let Some(name) = theme_argument(&mini.buffer) else {
            return;
        };
        if mini.selected().is_none() {
            return;
        }
        if self.theme_origin.is_none() {
            self.theme_origin = Some(self.theme.name().to_string());
        }
        self.set_theme(&name);
    }

    /// Record the current theme in the settings file, when there is one.
    ///
    /// Only an accepted `:theme` gets here: a preview and Escape never write.
    pub(super) fn save_theme(&mut self) {
        let Some(path) = self.config_path.clone() else {
            return;
        };
        let name = self.theme.name().to_string();
        self.message = match crate::config::save_theme_as(&path, self.theme_setting, &name) {
            Ok(()) => format!("theme: {name} (saved)"),
            Err(e) => format!("theme: {name} (not saved: {e})"),
        };
    }

    /// Record the split in the settings file, as `save_theme` does the theme.
    pub(super) fn save_split_ratio(&mut self) {
        let Some(path) = self.config_path.clone() else {
            return;
        };
        let percent = self.tree_percent;
        self.message = match crate::config::save_split_ratio(&path, percent) {
            Ok(()) => format!("split: {percent}% (saved)"),
            Err(e) => format!("split: {percent}% (not saved: {e})"),
        };
    }

    /// Put back the theme a preview replaced.
    fn restore_theme(&mut self) {
        if let Some(name) = self.theme_origin.take() {
            if name != self.theme.name() {
                match name.as_str() {
                    "builtin" => self.theme = crate::theme::Theme::builtin(),
                    name => {
                        self.set_theme(name);
                    }
                }
                self.message.clear();
            }
        }
    }

    /// While typing a search, show where it would land.
    pub(super) fn preview_search(&mut self) {
        let Some(mini) = self.mini.as_ref() else {
            return;
        };
        if !mini.kind.is_search() {
            return;
        }
        let pattern = mini.buffer.clone();
        let direction = if mini.kind == MiniKind::SearchForward {
            Direction::Forward
        } else {
            Direction::Backward
        };
        let Some(origin) = self.search_origin.clone() else {
            return;
        };
        // Each keystroke searches afresh from where the search began.
        self.restore_search_origin(&origin);
        if pattern.is_empty() {
            return;
        }
        // A pattern half typed, such as `(`, is not an error yet.
        let Ok(re) = search::compile(&pattern) else {
            return;
        };
        let scope = self.options.search_scope;
        match search::find(self.outline(), &re, &origin.start, direction, scope) {
            Some((hit, _)) => {
                self.land(hit);
                self.hlsearch = Some(re);
                self.message.clear();
            }
            None => self.message = format!("not found: {pattern}"),
        }
    }

    fn restore_search_origin(&mut self, origin: &SearchOrigin) {
        if self.current != origin.start.node {
            self.select(origin.start.node.clone());
        }
        self.focus = origin.focus;
        self.editor.cursor = origin.cursor;
        self.body_scroll = origin.scroll;
        self.hlsearch = origin.hlsearch.clone();
        self.last_hit = origin.last_hit.clone();
        self.doc
            .outline_mut_untracked()
            .set_expanded(origin.expanded.clone());
    }

    /// The cursor, as a search starts from it.
    fn search_cursor(&self) -> search::Hit {
        let place = match self.focus {
            Focus::Body => {
                let (row, col) = self.editor.cursor;
                let lines = self.body_buffer();
                let byte = lines
                    .get(row)
                    .map_or(0, |l| l.char_indices().nth(col).map_or(l.len(), |(i, _)| i));
                search::Place::Body(row, byte)
            }
            // The outline has no column, so a headline is searched from its
            // start, or from the match a search last landed on in it.
            Focus::Tree => match &self.last_hit {
                Some(hit) if hit.node == self.current => match hit.place {
                    search::Place::Headline(col) => search::Place::Headline(col),
                    search::Place::Body(..) => search::Place::Headline(0),
                },
                _ => search::Place::Headline(0),
            },
        };
        search::Hit {
            node: self.current.clone(),
            place,
        }
    }

    /// Go to a match: a headline in the outline, body text in the body.
    fn land(&mut self, hit: search::Hit) {
        if self.current != hit.node {
            self.select(hit.node.clone());
        }
        match hit.place {
            search::Place::Headline(_) => self.focus = Focus::Tree,
            search::Place::Body(row, byte) => {
                self.focus = Focus::Body;
                let lines = self.body_buffer();
                let col = lines
                    .get(row)
                    .map_or(0, |l| l[..byte.min(l.len())].chars().count());
                self.editor.cursor = (row, col);
                self.editor.desired_col = col;
                self.scroll_to_cursor();
            }
        }
        self.last_hit = Some(hit);
    }

    /// Close the line at the bottom, acting on it if it was accepted.
    /// Answer the yes/no question on the line, as typing `y` or `n` and
    /// Enter would. A dialog's buttons and keys answer with one press.
    pub fn answer(&mut self, yes: bool) {
        if self.mode != Mode::Confirm {
            return;
        }
        if let Some(mini) = self.mini.as_mut() {
            mini.buffer = if yes { "y" } else { "n" }.to_string();
            mini.cursor = 1;
        }
        self.finish_mini(true);
    }

    pub fn finish_mini(&mut self, accepted: bool) {
        // An accepted line keeps whatever the preview applied, so there is
        // nothing left to put back.
        if accepted {
            self.theme_origin = None;
        }
        let Some(mini) = self.mini.take() else {
            return;
        };
        self.mode = Mode::Normal;
        let text = mini.buffer;
        let refused = std::mem::take(&mut self.pending_overwrite);
        let (to_read, refresh) = std::mem::take(&mut self.pending_read);
        let approved = accepted && text.trim().eq_ignore_ascii_case("y");
        if !approved {
            match mini.kind {
                MiniKind::ConfirmOverwrite => {
                    self.message = format!("not overwritten: {}", plural(refused.len(), "file"))
                }
                MiniKind::ConfirmSave => self.save_now(false),
                MiniKind::ConfirmRead => self.message = "not read".to_string(),
                _ => {}
            }
        }
        if !accepted {
            if mini.kind.is_search() {
                if let Some(origin) = self.search_origin.take() {
                    self.restore_search_origin(&origin);
                }
            }
            return;
        }
        match mini.kind {
            MiniKind::Headline => {
                let p = self.current.clone();
                let o = self.outline();
                let was_file = p.is_at_file_node(o);
                self.doc.begin_group("rename-node");
                self.doc.set_headline(&p, &text);
                // A sentinel header would push a `#!` line down to line 3.
                if !was_file && p.is_at_file_node(self.outline()) {
                    let n = self.doc.add_first_directives(&p);
                    if n > 0 {
                        self.message = format!("added @first to {}", plural(n, "line"));
                    }
                }
                self.doc.end_group();
            }
            // An existing file is refused as `:saveas` refuses it, which the
            // message names.
            MiniKind::SaveAs => self.run_command_line(&format!("saveas {text}")),
            MiniKind::ConfirmQuit => {
                if text.trim().eq_ignore_ascii_case("y") {
                    self.quit = true;
                }
            }
            MiniKind::ConfirmSave => {
                if approved {
                    self.save_now(true);
                }
            }
            MiniKind::ConfirmRead => {
                if approved {
                    self.read_files(to_read, refresh);
                }
            }
            MiniKind::ConfirmOverwrite => {
                if approved {
                    for p in &refused {
                        let path = self.outline().full_path(p);
                        self.doc
                            .outline_mut_untracked()
                            .remember_read_path(p, &path);
                        let stamp = leolib::util::file_stamp(&path);
                        self.doc
                            .outline_mut_untracked()
                            .record_file_stamp(&path, stamp);
                    }
                    let result = self.doc.write_files(refused);
                    self.report_write(result);
                }
            }
            MiniKind::Command => {
                if !text.trim().is_empty() {
                    remember(&mut self.command_history, &text);
                }
                self.run_command_line(&text);
                // After the line, which may act on the selection: `:extract`.
                self.editor.visual = None;
            }
            MiniKind::SearchForward | MiniKind::SearchBackward => {
                let origin = self.search_origin.take();
                if text.is_empty() {
                    return;
                }
                remember(&mut self.search_history, &text);
                // The preview has already landed; a bad pattern, or none
                // found, is the news.
                match search::compile(&text) {
                    Ok(re) => {
                        let direction = match mini.kind {
                            MiniKind::SearchForward => Direction::Forward,
                            _ => Direction::Backward,
                        };
                        let scope = self.options.search_scope;
                        let found = origin.is_some_and(|origin| {
                            search::find(self.outline(), &re, &origin.start, direction, scope)
                                .is_some()
                        });
                        if !found {
                            self.message = format!("not found: {text}");
                        }
                        self.hlsearch = Some(re);
                    }
                    Err(e) => {
                        self.message = e;
                        return;
                    }
                }
                self.last_search = Some(LastSearch {
                    pattern: text,
                    direction: if mini.kind == MiniKind::SearchForward {
                        Direction::Forward
                    } else {
                        Direction::Backward
                    },
                });
            }
        }
    }

    /// Repeat the last search. `same` is `n`; otherwise `N`.
    pub fn repeat_search(&mut self, same: bool, count: usize) {
        let Some(last) = self.last_search.clone() else {
            self.message = "no previous search".to_string();
            return;
        };
        let direction = if same {
            last.direction
        } else {
            last.direction.reverse()
        };
        let re = match search::compile(&last.pattern) {
            Ok(re) => re,
            Err(e) => {
                self.message = e;
                return;
            }
        };
        // As in vim, `n` after `:noh` highlights again.
        self.hlsearch = Some(re.clone());
        let scope = self.options.search_scope;
        for _ in 0..count {
            let from = self.search_cursor();
            match search::find(self.outline(), &re, &from, direction, scope) {
                Some((hit, wrapped)) => {
                    if wrapped {
                        self.message = match direction {
                            Direction::Forward => "search hit BOTTOM, continuing at TOP",
                            Direction::Backward => "search hit TOP, continuing at BOTTOM",
                        }
                        .to_string();
                    }
                    self.land(hit);
                }
                None => {
                    self.message = format!("not found: {}", last.pattern);
                    return;
                }
            }
        }
    }
}
