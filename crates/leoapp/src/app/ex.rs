//! The `:` command line: vim's spellings, Leo's command names, and `:set`.

use super::*;

impl App {
    /// Run one `:` line.
    pub fn run_command_line(&mut self, line: &str) {
        // Trimmed at the start only: a replacement may end in spaces.
        let bare = line.trim_start().trim_start_matches(':').trim_start();
        if let Some(rest) = bare.strip_prefix("bufdo") {
            if rest.starts_with(char::is_whitespace) {
                return self.bufdo(rest);
            }
        }
        match crate::substitute::parse(bare) {
            Some(Ok(sub)) => return self.substitute(sub),
            Some(Err(e)) => {
                self.message = e;
                return;
            }
            None => {}
        }
        let Some(parsed) = minibuffer::parse_command(line) else {
            return;
        };
        if let Some(row) = parsed.row {
            self.run("goto-visible-row", row.max(1));
            return;
        }
        match parsed.name.as_str() {
            "quit" => {
                if parsed.force {
                    self.quit = true;
                } else {
                    self.request_quit();
                }
            }
            "save-and-quit" => {
                self.save();
                // A failed save leaves `changed` set, and says why.
                if self.mini.is_none() && !self.outline().changed {
                    self.request_quit();
                }
            }
            // vim's `:w path` writes a copy; `:saveas path` moves the outline.
            // Both write the dirty external files too, as Leo's `save-to` and
            // `save-as` do.
            "save" | "save-to" if !parsed.arg.is_empty() => {
                if self.refuse_existing(&parsed.arg, parsed.force) {
                    return;
                }
                match self.doc.save_to(&parsed.arg) {
                    Ok(path) => {
                        let files = self.doc.write_external_files(true);
                        let leo = format!("wrote a copy: {}", leolib::util::short_file_name(&path));
                        self.report_save(leo, files);
                    }
                    Err(e) => self.message = self.held_back(format!("save failed: {e}")),
                }
            }
            "save-as" | "save-to" if parsed.arg.is_empty() => {
                self.message = format!("{}: needs a file name", parsed.name)
            }
            "save-as" => {
                if self.refuse_existing(&parsed.arg, parsed.force) {
                    return;
                }
                let result = self.doc.save_all(&parsed.arg);
                match result.leo {
                    Ok(path) => {
                        let leo = format!("saved {}", leolib::util::short_file_name(&path));
                        self.report_save(leo, result.files);
                        self.note_dropped_uas(result.dropped_descendent_uas);
                    }
                    Err(e) => self.message = self.held_back(format!("save failed: {e}")),
                }
            }
            "open" if parsed.force && parsed.arg.is_empty() => self.revert(),
            "open" if parsed.force => self.open_file_now(&parsed.arg),
            "open" => self.open_file(&parsed.arg),
            "import-at-file" => self.import_at_file(&parsed.arg),
            "goto-global-line" => match parsed.arg.trim().parse::<usize>() {
                Ok(n) => self.goto_global_line(n),
                Err(_) => self.message = "usage: :goto-global-line N".to_string(),
            },
            "lsp-rename" => match parsed.arg.trim() {
                "" => self.message = "usage: :lsp-rename NAME".to_string(),
                name => self.lsp_request(leolsp::Request::Rename(name.to_string())),
            },
            "set" => self.set_options(&parsed.arg),
            "nohlsearch" | "noh" => self.hlsearch = None,
            "clone-find-all" => self.clone_find_all(&parsed.arg, false),
            "clone-find-all-flattened" => self.clone_find_all(&parsed.arg, true),
            "bufdo" => self.message = "usage: :bufdo %s/pattern/replacement/[flags]".to_string(),
            "substitute" | "s" => {
                self.message = "usage: :[range]s/pattern/replacement/[flags]".to_string()
            }
            "theme" if parsed.arg.is_empty() => {
                self.message = format!("theme: {}", self.theme.name())
            }
            "theme" => {
                if self.set_theme(&parsed.arg) {
                    self.save_theme();
                }
            }
            "help" if !parsed.arg.is_empty() => match commands::find(&parsed.arg) {
                Some(c) => {
                    let keys = crate::bindings::keys_for(c.name).join(" ");
                    self.message = format!("{}: {}  [{keys}]", c.name, c.summary);
                }
                None => self.message = format!("no such command: {}", parsed.arg),
            },
            name => {
                if commands::find(name).is_some() {
                    self.run(name, 1);
                } else {
                    self.message = format!("no such command: {name}");
                }
            }
        }
    }

    /// Leo's `clone-find-all` and `clone-find-all-flattened`, matching as `/`
    /// does. No pattern reuses the last search; a pattern becomes it.
    pub fn clone_find_all(&mut self, pattern: &str, flatten: bool) {
        let pattern = match (pattern, &self.last_search) {
            ("", Some(last)) => last.pattern.clone(),
            ("", None) => {
                self.message = "no previous search".to_string();
                return;
            }
            (p, _) => p.to_string(),
        };
        let re = match search::compile(&pattern) {
            Ok(re) => re,
            Err(e) => {
                self.message = e;
                return;
            }
        };
        let bodies = self.options.search_scope == Scope::All;
        // Leo's status words, for the found node's body.
        let mut status = vec!["Regex"];
        if !search::has_capital(&pattern) {
            status.insert(0, "Ignore Case");
        }
        status.push("Head");
        if bodies {
            status.push("Body");
        }
        let matches =
            |o: &Outline, p: &Position| re.is_match(p.h(o)) || (bodies && re.is_match(p.b(o)));
        let result = self
            .doc
            .clone_find_all(&pattern, &status.join(", "), flatten, matches);
        self.last_search = Some(LastSearch {
            pattern: pattern.clone(),
            direction: Direction::Forward,
        });
        self.hlsearch = Some(re);
        match result {
            Some((found, n)) => {
                self.focus = Focus::Tree;
                self.select(found);
                self.message = format!("found {n} for {pattern}");
            }
            None => self.message = format!("found 0 for {pattern}"),
        }
    }

    /// True, with a message, if `path` is another file that exists and `!` was
    /// not given. vim's E13.
    fn refuse_existing(&mut self, path: &str, force: bool) -> bool {
        let full = leolib::util::finalize(path);
        let exists = std::path::Path::new(&full).exists();
        if force || !exists || full == self.outline().file_name {
            return false;
        }
        self.message = format!("{full} exists: add ! to overwrite it");
        true
    }

    /// `:e path` -- open another outline, refusing to lose unsaved work.
    fn open_file(&mut self, path: &str) {
        if path.is_empty() {
            self.message = "open: needs a file name".to_string();
            return;
        }
        let unsaved = self.unsaved_work();
        if !unsaved.is_empty() {
            self.message = format!("{unsaved}: write first, or :e! to discard");
            return;
        }
        self.open_file_now(path);
    }

    /// Open `path` in place of this outline, whatever is unsaved.
    pub(super) fn open_file_now(&mut self, path: &str) {
        match open_or_new(path, true) {
            Ok((doc, new)) => {
                // What belongs to the session, not to the outline, stays.
                let keep = std::mem::replace(self, App::new(doc));
                self.options = keep.options;
                self.command_history = keep.command_history;
                self.search_history = keep.search_history;
                self.last_search = keep.last_search;
                self.hlsearch = keep.hlsearch;
                self.tree_percent = keep.tree_percent;
                self.theme = keep.theme;
                self.depth = keep.depth;
                self.theme_names = keep.theme_names;
                self.config_path = keep.config_path;
                self.messages = keep.messages;
                self.editor.register = keep.editor.register;
                self.editor.last_change = keep.editor.last_change;
                self.editor.last_find = keep.editor.last_find;
                for line in read_report_lines(&self.doc.read_report) {
                    self.log(line);
                }
                self.message = match new {
                    true => format!("new outline: {}", self.outline().file_name),
                    false => read_report_message(&self.doc.read_report)
                        .unwrap_or_else(|| format!("opened: {path}")),
                };
            }
            Err(e) => self.message = format!("open failed: {e}"),
        }
    }

    /// `:import-at-file path` -- import a file as an `@file` tree, then ask
    /// before writing the sentinels into it.
    fn import_at_file(&mut self, path: &str) {
        if path.is_empty() {
            self.message = "import-at-file: needs a file name".to_string();
            return;
        }
        let p = self.current.clone();
        match self.doc.import_at_file(&p, path) {
            Ok((new, needs_write)) => {
                self.select(new.clone());
                self.message = format!("imported: {path}");
                if needs_write {
                    self.pending_overwrite = vec![new];
                    self.open_mini(MiniKind::ConfirmOverwrite, String::new());
                }
            }
            Err(e) => self.message = format!("import failed: {e}"),
        }
    }

    /// `:set`, as vim reads it: words separated by spaces, each `name`,
    /// `noname`, `name=value`, `name:value` or `name?`. The first error stops
    /// the rest, and `:set` alone shows every value.
    fn set_options(&mut self, arg: &str) {
        if arg.trim().is_empty() {
            let all = ["search", "split", "wrap", "number", "syntax", "colors"];
            self.message = all
                .iter()
                .filter_map(|name| self.option_value(name))
                .collect::<Vec<_>>()
                .join("  ");
            return;
        }
        for word in arg.split_whitespace() {
            if !self.set_option(word) {
                return;
            }
        }
    }

    /// Option `name` as `:set name?` shows it.
    fn option_value(&self, name: &str) -> Option<String> {
        let flag = |on: bool, name: &str| match on {
            true => name.to_string(),
            false => format!("no{name}"),
        };
        Some(match name {
            "search" => match self.options.search_scope {
                Scope::Headlines => "search=headlines".to_string(),
                Scope::All => "search=all".to_string(),
            },
            "split" => format!("split={}", self.tree_percent),
            "wrap" => flag(self.options.wrap, "wrap"),
            "number" | "nu" => flag(self.options.number, "number"),
            "syntax" => flag(self.options.syntax, "syntax"),
            "colors" | "colours" => match self.depth {
                crate::theme::Depth::True => "colors=true".to_string(),
                crate::theme::Depth::Indexed => "colors=256".to_string(),
                crate::theme::Depth::Ansi16 => "colors=16".to_string(),
                crate::theme::Depth::None => "colors=none".to_string(),
            },
            _ => return None,
        })
    }

    /// One `:set` word. False, with the message saying why, if it failed.
    fn set_option(&mut self, word: &str) -> bool {
        if let Some(name) = word.strip_suffix('?') {
            return match self.option_value(name) {
                Some(value) => {
                    self.message = value;
                    true
                }
                None => self.unknown_option(name),
            };
        }
        let (name, value) = match word.find(['=', ':']) {
            Some(i) => (&word[..i], Some(&word[i + 1..])),
            None => (word, None),
        };
        match (name, value) {
            ("search", Some("all")) => self.options.search_scope = Scope::All,
            ("search", Some("headlines")) => self.options.search_scope = Scope::Headlines,
            ("split", Some(v)) => match v.parse::<u16>() {
                Ok(n) => {
                    self.tree_percent = n.clamp(15, 85);
                    self.save_split_ratio();
                }
                Err(_) => {
                    self.message = format!("set: not a number: {v}");
                    return false;
                }
            },
            ("wrap", None) => self.options.wrap = true,
            ("nowrap", None) => self.options.wrap = false,
            ("number", None) | ("nu", None) => self.options.number = true,
            ("nonumber", None) | ("nonu", None) => self.options.number = false,
            ("syntax", None) => self.options.syntax = true,
            ("nosyntax", None) => self.options.syntax = false,
            ("colors", Some(v)) | ("colours", Some(v)) => match crate::theme::Depth::parse(v) {
                Some(depth) => self.depth = depth,
                None => {
                    self.message = format!("set: colors must be true, 256 or 16, not {v}");
                    return false;
                }
            },
            // A number or string option named alone shows its value, as in vim.
            ("search" | "split" | "colors" | "colours", None) => {
                self.message = self.option_value(name).unwrap_or_default()
            }
            _ => return self.unknown_option(word),
        }
        true
    }

    fn unknown_option(&mut self, word: &str) -> bool {
        self.message = format!(
            "set: unknown option: {word}. try search=all|headlines, split=N, wrap, \
             number, syntax, colors=true|256|16"
        );
        false
    }

    /// `:[range]s/pattern/replacement/[flags]` on the current node's body.
    fn substitute(&mut self, sub: crate::substitute::Substitute) {
        let mut lines = self.body_buffer();
        let last = self.last_search.as_ref().map(|s| s.pattern.clone());
        let cursor = self.editor.cursor.0;
        match crate::substitute::apply(&sub, &mut lines, cursor, last.as_deref()) {
            Err(e) => self.message = e,
            Ok(o) => {
                let what = if sub.count_only {
                    "match"
                } else {
                    "substitution"
                };
                self.message = format!("{} on {}", plural(o.count, what), plural(o.lines, "line"));
                if sub.count_only {
                    return;
                }
                self.commit_body(&lines);
                self.editor.cursor = (o.last_line, 0);
                self.editor.clamp(&lines);
            }
        }
    }

    /// `:bufdo [range]s/...`, vim's `:bufdo` with each node's body a buffer.
    ///
    /// One undo step for the whole outline. A clone's body is changed once:
    /// a second pass would apply a replacement such as `s/a/aa/` twice.
    fn bufdo(&mut self, arg: &str) {
        let sub = match crate::substitute::parse(arg.trim_start()) {
            Some(Ok(sub)) => sub,
            Some(Err(e)) => {
                self.message = e;
                return;
            }
            None => {
                self.message = "bufdo: only :s is supported, as in :bufdo %s/a/b/g".to_string();
                return;
            }
        };
        // Without a range, :s takes the cursor's line, which other nodes lack.
        if sub.range.is_none() {
            self.message = "bufdo: give :s a range, as in :bufdo %s/a/b/g".to_string();
            return;
        }
        let last = self.last_search.as_ref().map(|s| s.pattern.clone());
        let re = match crate::substitute::compile(&sub, last.as_deref()) {
            Ok(re) => re,
            Err(e) => {
                self.message = e;
                return;
            }
        };
        let mut seen = std::collections::HashSet::new();
        let (mut count, mut lines_changed, mut nodes) = (0, 0, 0);
        self.doc.begin_group("substitute");
        for p in self.outline().all_positions() {
            if !seen.insert(p.v) || !re.is_match(p.b(self.outline())) {
                continue;
            }
            let mut lines = editor::split(p.b(self.outline()));
            // A body too short for the range, or with no match in it, stays.
            let Ok(o) = crate::substitute::apply_with(&sub, &re, &mut lines, 0) else {
                continue;
            };
            count += o.count;
            lines_changed += o.lines;
            nodes += 1;
            if !sub.count_only {
                self.doc.set_body(&p, &editor::join(&lines));
            }
        }
        self.doc.end_group();
        if count == 0 {
            self.message = format!("pattern not found: {}", re.as_str());
            return;
        }
        let what = if sub.count_only {
            "match"
        } else {
            "substitution"
        };
        self.message = format!(
            "{} on {} in {}",
            plural(count, what),
            plural(lines_changed, "line"),
            plural(nodes, "node")
        );
        let lines = self.body_buffer();
        self.editor.clamp(&lines);
    }

    /// Widen the pane that has focus by `steps` of 5%, or narrow it.
    pub fn resize_pane(&mut self, steps: i32) {
        let outline = if self.focus == Focus::Tree {
            steps
        } else {
            -steps
        };
        self.tree_percent = (self.tree_percent as i32 + 5 * outline).clamp(15, 85) as u16;
    }

    /// Load a theme by name, keeping the current one if there is no such file.
    ///
    /// Returns false when nothing was found, which `main` uses to fall back
    /// without a message and `:theme` uses to report one.
    pub fn set_theme(&mut self, name: &str) -> bool {
        match crate::theme::Theme::load(name) {
            Some(theme) => {
                self.theme = theme;
                true
            }
            None => {
                self.message = format!("theme not found: {name}");
                false
            }
        }
    }
}
