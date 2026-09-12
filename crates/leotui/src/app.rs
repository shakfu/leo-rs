//! The view's state, and the dispatcher that turns keys into commands.
//!
//! Everything that changes the outline goes through `leolib::Document`, so an
//! edit lands in the model and its undo history, never in a widget the model
//! then has to be told about. This file holds no copy of the outline.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use leolib::{Document, Outline, Position};

use crate::bindings;
use crate::commands;
use crate::editor::change::{Change, InsertAt, Operator, Range};
use crate::editor::motion::Kind;
use crate::editor::parse::{Action, Parser};
use crate::editor::{self, Editor};
use crate::keys::{self, Key, Pending};
use std::rc::Rc;

use crate::minibuffer::{self, MiniKind, Minibuffer};
use crate::search::{self, Direction, LastSearch, Scope};

/// Which pane a key acts on. Leo spells this `!tree` and `!body`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Focus {
    Tree,
    Body,
}

/// How keys are interpreted. See `docs/dev/tui-design.md` section 5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    Normal,
    /// A one-line edit of the current headline.
    Headline,
    /// Editing the body: keys are text.
    Insert,
    /// A charwise or linewise selection in the body.
    Visual,
    /// The help overlay, which takes the keyboard while it is up.
    Help,
    /// A yes/no question in the status line.
    Confirm,
    /// The `:` minibuffer.
    Command,
    /// An incremental `/` or `?` search.
    Search,
}

impl Mode {
    /// What the status line shows, vim-style.
    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Headline => "HEADLINE",
            Mode::Insert => "INSERT",
            Mode::Visual => "VISUAL",
            Mode::Help => "HELP",
            Mode::Confirm => "CONFIRM",
            Mode::Command => "COMMAND",
            Mode::Search => "SEARCH",
        }
    }
}

/// Options `:set` changes.
pub struct Options {
    pub search_scope: Scope,
    pub wrap: bool,
    pub number: bool,
    pub syntax: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            search_scope: Scope::All,
            wrap: false,
            number: false,
            syntax: true,
        }
    }
}

/// `n word`, with an `s` unless `n` is 1.
fn plural(n: usize, word: &str) -> String {
    match n {
        1 => format!("1 {word}"),
        n => format!("{n} {word}s"),
    }
}

/// Where a search started. Escape puts all of it back.
#[derive(Clone)]
struct SearchOrigin {
    start: search::Hit,
    focus: Focus,
    cursor: (usize, usize),
    scroll: usize,
    hlsearch: Option<regex::Regex>,
    last_hit: Option<search::Hit>,
}

pub struct App {
    pub doc: Document,
    pub current: Position,
    pub focus: Focus,
    pub mode: Mode,
    pub mini: Option<Minibuffer>,
    /// The body editor's cursor, registers and last change. The text is in
    /// the outline; this is everything else.
    pub editor: Editor,
    /// The body's working copy, live only while a change is being typed.
    pub buffer: Option<Vec<String>>,
    parser: Parser,
    pub options: Options,
    /// `:` lines, oldest first.
    pub command_history: Vec<String>,
    pub search_history: Vec<String>,
    pub last_search: Option<LastSearch>,
    /// Where a search started, so Escape can put the view back.
    search_origin: Option<SearchOrigin>,
    /// The pattern whose matches are highlighted, vim's hlsearch. `:noh`
    /// clears it; the next search or `n` sets it again.
    pub hlsearch: Option<regex::Regex>,
    /// Where a search last landed, so `n` in a headline starts after it.
    last_hit: Option<search::Hit>,
    pending: Pending,
    /// First visible row of the outline pane.
    pub top: usize,
    pub body_scroll: usize,
    pub help_scroll: usize,
    /// Width of the outline pane, as a percentage.
    pub tree_percent: u16,
    /// How deep `expand-next-level` has unfolded, and from which node.
    pub expansion_level: usize,
    expansion_node: Option<Position>,
    pub message: String,
    pub quit: bool,
    /// Rows and columns of the outline pane, for paging. Set while drawing.
    pub tree_height: usize,
    pub body_height: usize,
    /// The body pane's colouring, which survives a redraw that changed nothing.
    pub colouring: crate::highlight::Colouring,
    /// What each class is drawn as, and how many colours the terminal has.
    pub theme: crate::theme::Theme,
    pub depth: crate::theme::Depth,
    /// Every theme on disk, read when the `:` line first opens. Completion
    /// and the drop-down both walk it, and a redraw must not touch the disk.
    pub theme_names: Rc<Vec<String>>,
    /// The theme in force before `:theme` began previewing, so Escape can put
    /// it back. `search_origin` does the same for `/`.
    theme_origin: Option<String>,
    /// Where an accepted `:theme` records its choice. `main` sets it; it is
    /// None in a test, so a test never writes the user's settings file.
    pub config_path: Option<std::path::PathBuf>,
    /// Total positions, and the outline generation it was counted at.
    /// Counting is O(outline), and the status line asks on every keystroke.
    position_count: (u64, usize),
    /// Files `w` refused to overwrite, waiting on the y/n prompt.
    pending_overwrite: Vec<Position>,
}

/// The status line's account of external files that could not be read.
///
/// A failed read leaves an `@file` node empty. Saying nothing would present
/// that empty node as the file's contents.
pub fn read_report_message(report: &leolib::external::ReadResult) -> Option<String> {
    let first = report.errors.first()?;
    Some(match report.errors.len() {
        1 => format!("external file not read: {}", first.error),
        n => format!("{n} external files not read; first: {}", first.error),
    })
}

/// The theme a `:theme NAME` line names, if it names one.
fn theme_argument(line: &str) -> Option<String> {
    let parsed = minibuffer::parse_command(line)?;
    match parsed.name == "theme" && !parsed.arg.is_empty() {
        true => Some(parsed.arg),
        false => None,
    }
}

/// One row of the outline pane.
pub struct Row {
    pub position: Position,
    pub depth: usize,
    pub has_children: bool,
    pub expanded: bool,
    pub marked: bool,
    pub dirty: bool,
    pub cloned: bool,
    pub is_file: bool,
    pub headline: String,
}

impl App {
    pub fn new(doc: Document) -> Self {
        let current = doc
            .outline
            .root_position()
            .expect("an outline always has a root");
        let mut app = Self {
            doc,
            current,
            focus: Focus::Tree,
            mode: Mode::Normal,
            mini: None,
            editor: Editor::new(),
            buffer: None,
            parser: Parser::default(),
            options: Options::default(),
            command_history: Vec::new(),
            search_history: Vec::new(),
            last_search: None,
            search_origin: None,
            hlsearch: None,
            last_hit: None,
            pending: Pending::default(),
            top: 0,
            body_scroll: 0,
            help_scroll: 0,
            tree_percent: 35,
            expansion_level: 1,
            expansion_node: None,
            message: String::new(),
            quit: false,
            tree_height: 20,
            body_height: 20,
            colouring: Default::default(),
            // A theme is loaded by `main`, so a test's colours do not depend
            // on what is installed on the machine running it.
            theme: crate::theme::Theme::builtin(),
            depth: crate::theme::Depth::detect(),
            theme_names: Rc::new(Vec::new()),
            theme_origin: None,
            config_path: None,
            position_count: (u64::MAX, 0),
            pending_overwrite: Vec::new(),
        };
        app.expand_ancestors();
        app
    }

    pub fn outline(&self) -> &Outline {
        &self.doc.outline
    }

    // --- Dispatch ---------------------------------------------------------

    /// Feed one key to the current mode.
    pub fn handle_key(&mut self, event: KeyEvent) {
        self.message.clear();
        match self.mode {
            Mode::Headline | Mode::Confirm | Mode::Command | Mode::Search => self.mini_key(event),
            Mode::Insert => self.insert_key(event),
            Mode::Visual => self.body_key(event),
            Mode::Normal if self.focus == Focus::Body => self.body_key(event),
            Mode::Normal | Mode::Help => self.command_key(event),
        }
    }

    /// NORMAL and HELP: accumulate a count and keys, then run a binding.
    fn command_key(&mut self, event: KeyEvent) {
        let key = Key::from_event(event);
        // A leading digit is a count, not a binding. `0` only continues one.
        if self.pending.keys.is_empty() {
            if let KeyCode::Char(ch) = key.code {
                if key.mods.is_empty()
                    && ch.is_ascii_digit()
                    && self.pending.push_digit(ch as usize - '0' as usize)
                {
                    return;
                }
            }
        }
        if key.code == KeyCode::Esc && !self.pending.is_empty() {
            self.pending.clear();
            return;
        }
        self.pending.keys.push(key);
        let (exact, prefix) = self.match_pending();
        if let Some(command) = exact {
            let count = self.pending.count();
            self.pending.clear();
            self.run(command, count);
        } else if !prefix {
            let typed = self.pending.describe();
            self.pending.clear();
            self.message = format!("no binding for {typed}");
        }
    }

    /// Whether the pending keys are exactly a binding, and whether they are a
    /// prefix of one.
    fn match_pending(&self) -> (Option<&'static str>, bool) {
        let mut exact = None;
        let mut prefix = false;
        for binding in bindings::for_context(self.mode, self.focus) {
            let want = keys::parse(binding.keys);
            if want == self.pending.keys {
                exact = Some(binding.command);
            } else if want.len() > self.pending.keys.len()
                && want[..self.pending.keys.len()] == self.pending.keys[..]
            {
                prefix = true;
            }
        }
        (exact, prefix)
    }

    /// Run a command by name.
    pub fn run(&mut self, name: &str, count: usize) {
        match commands::find(name) {
            Some(command) => (command.run)(self, count),
            None => self.message = format!("no such command: {name}"),
        }
    }

    /// What has been typed towards a binding, for the status line. The body
    /// has its own grammar, so it has its own pending keys.
    pub fn pending_keys(&self) -> String {
        if self.focus == Focus::Body && matches!(self.mode, Mode::Normal | Mode::Visual) {
            return self.parser.describe();
        }
        self.pending.describe()
    }

    // --- The outline pane -------------------------------------------------

    /// The rows the outline pane shows: the tree with folded subtrees skipped.
    pub fn rows(&self) -> Vec<Row> {
        let o = self.outline();
        let mut rows = Vec::new();
        let mut p = o.root_position();
        while let Some(cur) = p {
            let has_children = cur.has_children(o);
            let expanded = o.is_expanded(&cur);
            rows.push(Row {
                depth: cur.level(),
                has_children,
                expanded,
                marked: cur.is_marked(o),
                dirty: cur.is_dirty(o),
                cloned: cur.is_cloned(o),
                is_file: cur.is_any_at_file_node(o),
                headline: cur.h(o).to_string(),
                position: cur.clone(),
            });
            p = if has_children && expanded {
                cur.thread_next(o)
            } else {
                cur.node_after_tree(o)
            };
        }
        rows
    }

    /// The visible rows and the index of the selected one, in one walk.
    pub fn rows_and_current(&self) -> (Vec<Row>, usize) {
        let rows = self.rows();
        let current = row_of(&rows, &self.current);
        (rows, current)
    }

    pub fn current_row(&self) -> usize {
        row_of(&self.rows(), &self.current)
    }

    /// The current node's ancestors, outermost first: the breadcrumb.
    pub fn breadcrumb(&self) -> String {
        let o = self.outline();
        let mut parts: Vec<String> = self
            .current
            .self_and_parents(o)
            .iter()
            .rev()
            .map(|p| p.h(o).to_string())
            .collect();
        if parts.len() > 4 {
            let tail = parts.split_off(parts.len() - 3);
            parts = std::iter::once("...".to_string()).chain(tail).collect();
        }
        parts.join(" > ")
    }

    fn position_count(&mut self) -> usize {
        let generation = self.outline().generation;
        if self.position_count.0 != generation {
            self.position_count = (generation, self.outline().all_positions().len());
        }
        self.position_count.1
    }

    // --- Selection --------------------------------------------------------

    pub fn select(&mut self, p: Position) {
        self.current = p;
        self.body_scroll = 0;
        // The body is a different buffer now.
        self.buffer = None;
        self.editor.cursor = (0, 0);
        self.editor.desired_col = 0;
        self.editor.visual = None;
        self.expand_ancestors();
    }

    /// Unfold everything above the current node, so it can be seen.
    pub fn expand_ancestors(&mut self) {
        let p = self.current.clone();
        self.doc.outline.expand_all_ancestors(&p);
    }

    /// After an undo the current position may no longer exist.
    pub fn clamp_current(&mut self) {
        if !self.outline().position_exists(&self.current) {
            if let Some(root) = self.outline().root_position() {
                self.current = root;
            }
        }
    }

    /// Move by a fraction of the visible pane, in whichever pane has focus.
    pub fn page(&mut self, fraction: f32) {
        match self.focus {
            Focus::Tree => {
                let step = ((self.tree_height as f32) * fraction) as i32;
                self.move_rows(step);
            }
            Focus::Body => {
                let step = ((self.body_height as f32) * fraction) as i32;
                if step >= 0 {
                    self.body_scroll += step as usize;
                } else {
                    self.body_scroll = self.body_scroll.saturating_sub((-step) as usize);
                }
            }
        }
    }

    /// Select the visible row at `index`, clamped to the outline.
    pub fn move_to_row(&mut self, index: usize) {
        let rows = self.rows();
        if let Some(row) = rows.get(index.min(rows.len().saturating_sub(1))) {
            self.select(row.position.clone());
        }
    }

    /// Move `delta` visible rows in the outline.
    pub fn move_rows(&mut self, delta: i32) {
        let rows = self.rows();
        if rows.is_empty() {
            return;
        }
        let i = self.current_row() as i32 + delta;
        let i = i.clamp(0, rows.len() as i32 - 1) as usize;
        self.select(rows[i].position.clone());
    }

    // --- Folding ----------------------------------------------------------

    /// Leo's left arrow: fold this node, or step out to the parent.
    pub fn contract_or_go_left(&mut self) {
        let p = self.current.clone();
        if p.has_children(self.outline()) && self.outline().is_expanded(&p) {
            self.doc.outline.contract(&p);
        } else if let Some(parent) = p.parent(self.outline()) {
            self.select(parent);
        }
    }

    /// Leo's right arrow: unfold this node and step into it.
    pub fn expand_and_go_right(&mut self) {
        let p = self.current.clone();
        if !p.has_children(self.outline()) {
            return;
        }
        if !self.outline().is_expanded(&p) {
            self.doc.outline.expand(&p);
        } else if let Some(child) = p.first_child(self.outline()) {
            self.select(child);
        }
    }

    /// Unfold the current subtree to `level`, tracking where the count is from.
    ///
    /// Leo keeps the level per node, so moving to another node and pressing
    /// `zr` starts counting again rather than continuing from elsewhere.
    pub fn expand_to_level(&mut self, level: usize) {
        let p = self.current.clone();
        if self.expansion_node.as_ref() != Some(&p) {
            self.expansion_node = Some(p.clone());
        }
        let max = self.doc.outline.expand_to_level(&p, level.max(1));
        self.expansion_level = max + 1;
        self.message = format!("level: {}", max + 1);
    }

    // --- Files ------------------------------------------------------------

    pub fn save(&mut self) {
        if self.outline().file_name.is_empty() {
            self.open_mini(MiniKind::SaveAs, String::new());
            return;
        }
        match self.doc.save("") {
            Ok(path) => self.message = format!("saved: {path}"),
            Err(e) => self.message = format!("save failed: {e}"),
        }
    }

    /// Write the outline's external files. Only dirty trees, as Leo does.
    ///
    /// A file that exists but was never read is refused, then offered on a
    /// y/n prompt, as Leo asks before overwriting it (issue #50).
    pub fn write_external(&mut self) {
        let result = self.doc.write_external_files(true);
        self.report_write(result);
    }

    /// Say what a write did, and ask about any file it refused.
    fn report_write(&mut self, result: leolib::external::WriteResult) {
        let mut parts = vec![format!("wrote {}", result.written.len())];
        if result.unchanged > 0 {
            parts.push(format!("{} unchanged", result.unchanged));
        }
        if !result.errors.is_empty() {
            parts.push(format!(
                "{} failed: {}",
                result.errors.len(),
                result.errors[0].error
            ));
        }
        self.message = parts.join(", ");
        if !result.refused.is_empty() {
            self.pending_overwrite = result.refused;
            self.open_mini(MiniKind::ConfirmOverwrite, String::new());
        }
    }

    /// The prompt drawn before the minibuffer's text.
    pub fn mini_label(&self) -> String {
        let Some(mini) = &self.mini else {
            return String::new();
        };
        match (mini.kind, self.pending_overwrite.as_slice()) {
            (MiniKind::ConfirmOverwrite, [p]) => {
                let path = self.outline().full_path(p);
                let name = std::path::Path::new(&path)
                    .file_name()
                    .map_or(path.clone(), |n| n.to_string_lossy().to_string());
                format!("overwrite {name}, which this outline has not read? (y/n) ")
            }
            (MiniKind::ConfirmOverwrite, files) => format!(
                "overwrite {} files this outline has not read? (y/n) ",
                files.len()
            ),
            (kind, _) => kind.label().to_string(),
        }
    }

    pub fn request_quit(&mut self) {
        if !self.outline().changed {
            self.quit = true;
            return;
        }
        self.open_mini(MiniKind::ConfirmQuit, String::new());
    }

    // --- Prompts and the body editor --------------------------------------

    pub fn begin_headline_edit(&mut self) {
        let text = self.current.h(self.outline()).to_string();
        self.open_mini(MiniKind::Headline, text);
    }

    /// Open the line at the bottom of the screen, and enter its mode.
    pub fn open_mini(&mut self, kind: MiniKind, text: String) {
        self.mode = match kind {
            MiniKind::Command => Mode::Command,
            MiniKind::SearchForward | MiniKind::SearchBackward => Mode::Search,
            MiniKind::ConfirmQuit | MiniKind::ConfirmOverwrite => Mode::Confirm,
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
            });
        }
        if kind == MiniKind::Command && self.theme_names.is_empty() {
            self.theme_names = Rc::new(crate::theme::names());
        }
        self.mini = Some(Minibuffer::new(kind, text));
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

    /// Every mode whose keys are text: the headline, `:` and `/`.
    fn mini_key(&mut self, event: KeyEvent) {
        let Some(mini) = self.mini.as_mut() else {
            self.mode = Mode::Normal;
            return;
        };
        match event.code {
            KeyCode::Esc => {
                if mini.kind == MiniKind::Command {
                    mini.cancel_completion();
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
    fn save_theme(&mut self) {
        let Some(path) = self.config_path.clone() else {
            return;
        };
        let name = self.theme.name().to_string();
        self.message = match crate::config::save_theme(&path, &name) {
            Ok(()) => format!("theme: {name} (saved)"),
            Err(e) => format!("theme: {name} (not saved: {e})"),
        };
    }

    /// Record the split in the settings file, as `save_theme` does the theme.
    fn save_split_ratio(&mut self) {
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
    fn preview_search(&mut self) {
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
        let approved = accepted && text.trim().eq_ignore_ascii_case("y");
        if mini.kind == MiniKind::ConfirmOverwrite && !approved {
            self.message = format!("not overwritten: {} unread file(s)", refused.len());
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
                self.doc.set_headline(&p, &text);
            }
            MiniKind::SaveAs => match self.doc.save(&text) {
                Ok(path) => self.message = format!("saved: {path}"),
                Err(e) => self.message = format!("save failed: {e}"),
            },
            MiniKind::ConfirmQuit => {
                if text.trim().eq_ignore_ascii_case("y") {
                    self.quit = true;
                }
            }
            MiniKind::ConfirmOverwrite => {
                if approved {
                    for p in &refused {
                        let path = self.outline().full_path(p);
                        self.doc.outline.remember_read_path(p, &path);
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
            }
            MiniKind::SearchForward | MiniKind::SearchBackward => {
                self.search_origin = None;
                if text.is_empty() {
                    return;
                }
                remember(&mut self.search_history, &text);
                // The preview has already landed; only a bad pattern is news.
                match search::compile(&text) {
                    Ok(re) => self.hlsearch = Some(re),
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
                if self.mini.is_none() {
                    self.quit = true;
                }
            }
            "save" if !parsed.arg.is_empty() => match self.doc.save(&parsed.arg) {
                Ok(path) => self.message = format!("saved: {path}"),
                Err(e) => self.message = format!("save failed: {e}"),
            },
            "open" => self.open_file(&parsed.arg),
            "import-at-file" => self.import_at_file(&parsed.arg),
            "set" => self.set_options(&parsed.arg),
            "nohlsearch" | "noh" => self.hlsearch = None,
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

    /// `:e path` -- open another outline, refusing to lose unsaved work.
    fn open_file(&mut self, path: &str) {
        if path.is_empty() {
            self.message = "open: needs a file name".to_string();
            return;
        }
        if self.outline().changed {
            self.message = "unsaved changes: save first, or use :q! and reopen".to_string();
            return;
        }
        match Document::open(path, true) {
            Ok(doc) => {
                let keep = std::mem::replace(self, App::new(doc));
                self.options = keep.options;
                self.command_history = keep.command_history;
                self.search_history = keep.search_history;
                self.tree_percent = keep.tree_percent;
                self.message = read_report_message(&self.doc.read_report)
                    .unwrap_or_else(|| format!("opened: {path}"));
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
        self.doc.undoer.begin_group("substitute");
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
        self.doc.undoer.end_group();
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

    /// The body's own text: the working copy if one is live, else the model.
    pub fn body_buffer(&self) -> Vec<String> {
        match &self.buffer {
            Some(lines) => lines.clone(),
            None => editor::split(self.current.b(self.outline())),
        }
    }

    /// Write the body back as one change, which is one undo bead.
    fn commit_body(&mut self, lines: &[String]) {
        let p = self.current.clone();
        let text = editor::join(lines);
        self.doc.set_body(&p, &text);
        self.buffer = None;
    }

    /// NORMAL and VISUAL with body focus: the vim grammar.
    fn body_key(&mut self, event: KeyEvent) {
        let key = Key::from_event(event);
        self.parser.visual = self.mode == Mode::Visual;
        let action = self.parser.feed(key);
        let lines = self.body_buffer();
        let screen = (self.body_scroll, self.body_height);
        match action {
            Action::Pending => {}
            Action::Unknown => self.message = "no such command".to_string(),
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
                for _ in 0..count {
                    self.run_change(change.clone(), screen);
                }
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
    fn scroll_to_cursor(&mut self) {
        let row = self.editor.cursor.0;
        let height = self.body_height.max(1);
        if row < self.body_scroll {
            self.body_scroll = row;
        } else if row >= self.body_scroll + height {
            self.body_scroll = row + 1 - height;
        }
    }

    /// INSERT. Escape commits the change; Ctrl-c abandons it.
    fn insert_key(&mut self, event: KeyEvent) {
        let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
        let mut lines = self.buffer.clone().unwrap_or_else(|| self.body_buffer());
        match event.code {
            KeyCode::Esc => {
                self.editor.end_insert(&mut lines);
                self.commit_body(&lines);
                self.mode = Mode::Normal;
                self.editor.clamp(&lines);
                return;
            }
            KeyCode::Char('c') if ctrl => {
                self.editor.cancel_insert();
                self.buffer = None;
                self.mode = Mode::Normal;
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
            KeyCode::Char(ch) => self.editor.insert_char(&mut lines, ch),
            _ => {}
        }
        self.buffer = Some(lines);
        self.scroll_to_cursor();
    }

    /// The status line: what the outline is and what state it is in.
    pub fn status(&mut self) -> String {
        let positions = self.position_count();
        let (rows, current) = self.rows_and_current();
        let o = self.outline();
        let name = if o.file_name.is_empty() {
            "<unsaved>".to_string()
        } else {
            leolib::util::short_file_name(&o.file_name)
        };
        let changed = if o.changed { " *" } else { "" };
        format!(
            "{name}{changed}  {}/{}  {positions} positions",
            current + 1,
            rows.len()
        )
    }
}

/// The index of `current` among `rows`, or 0 if it is not shown.
fn row_of(rows: &[Row], current: &Position) -> usize {
    rows.iter()
        .position(|r| r.position == *current)
        .unwrap_or(0)
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

/// Keep the newest entry once, at the end.
fn remember(history: &mut Vec<String>, entry: &str) {
    history.retain(|e| e != entry);
    history.push(entry.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys;

    fn app() -> App {
        // a / a1 / a2, b, c
        let mut doc = Document::new_empty("");
        let root = doc.outline.root_position().unwrap();
        doc.set_headline(&root, "a");
        let a1 = doc.outline.insert_as_last_child(&root);
        doc.set_headline(&a1, "a1");
        let a2 = doc.outline.insert_as_last_child(&a1);
        doc.set_headline(&a2, "a2");
        let b = doc.outline.insert_after(&root);
        doc.set_headline(&b, "b");
        let c = doc.outline.insert_after(&b);
        doc.set_headline(&c, "c");
        doc.undoer.clear();
        App::new(doc)
    }

    /// Type a binding spec, one key at a time, as a terminal would.
    fn press(app: &mut App, spec: &str) {
        for key in keys::parse(spec) {
            app.handle_key(KeyEvent::new(key.code, key.mods));
        }
    }

    fn heads(app: &App) -> Vec<String> {
        app.rows()
            .iter()
            .map(|r| format!("{}{}", "  ".repeat(r.depth), r.headline))
            .collect()
    }

    #[test]
    fn a_folded_subtree_is_not_shown() {
        let mut app = app();
        assert_eq!(heads(&app), vec!["a", "b", "c"]);
        press(&mut app, "l");
        assert_eq!(heads(&app), vec!["a", "  a1", "b", "c"]);
    }

    #[test]
    fn a_two_key_sequence_waits_for_its_second_key() {
        let mut app = app();
        press(&mut app, "j");
        assert_eq!(app.current.h(app.outline()), "b");
        // `g` alone does nothing and waits.
        app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        assert_eq!(app.current.h(app.outline()), "b");
        assert_eq!(app.pending_keys(), "g");
        app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        assert_eq!(app.current.h(app.outline()), "a");
        assert_eq!(app.pending_keys(), "");
    }

    #[test]
    fn a_count_repeats_a_command() {
        let mut app = app();
        press(&mut app, "2j");
        assert_eq!(app.current.h(app.outline()), "c");
    }

    #[test]
    fn escape_abandons_a_half_typed_sequence() {
        let mut app = app();
        app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.pending_keys(), "");
        // The next `g` starts a fresh sequence rather than completing `gg`.
        app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        assert_eq!(app.pending_keys(), "g");
    }

    #[test]
    fn an_unbound_key_says_so_and_clears() {
        let mut app = app();
        press(&mut app, "Q");
        assert!(app.message.contains("no binding"), "{}", app.message);
        assert_eq!(app.pending_keys(), "");
    }

    #[test]
    fn the_same_key_means_different_things_in_each_pane() {
        let mut app = app();
        // `j` in the tree moves the selection.
        press(&mut app, "j");
        assert_eq!(app.current.h(app.outline()), "b");
        let p = app.current.clone();
        app.doc.set_body(&p, "one\ntwo\nthree\n");
        press(&mut app, "Tab");
        assert_eq!(app.focus, Focus::Body);
        // `j` in the body moves the text cursor, and leaves the outline alone.
        press(&mut app, "j");
        assert_eq!(app.current.h(app.outline()), "b");
        assert_eq!(app.editor.cursor.0, 1);
    }

    #[test]
    fn shift_tab_moves_between_the_panes_like_tab() {
        // A terminal sends BackTab+SHIFT for it; the table says Shift-Tab.
        let back_tab = KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT);
        let mut app = app();
        app.handle_key(back_tab);
        assert_eq!(app.focus, Focus::Body);
        app.handle_key(back_tab);
        assert_eq!(app.focus, Focus::Tree);
    }

    #[test]
    fn escape_in_the_body_returns_to_the_tree() {
        let mut app = app();
        press(&mut app, "Tab");
        assert_eq!(app.focus, Focus::Body);
        press(&mut app, "Escape");
        assert_eq!(app.focus, Focus::Tree);
    }

    #[test]
    fn leos_literal_chords_act_when_the_terminal_can_send_them() {
        // `examples/keyprobe.rs` shows crossterm decodes `ESC[109;5u` as
        // Char('m')+CONTROL once the enhancement flags are pushed. This is the
        // other half: that such a key reaches Leo's command.
        let ctrl = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);

        let mut marked = app();
        marked.handle_key(ctrl('m'));
        assert!(
            marked.current.is_marked(marked.outline()),
            "Ctrl-M did not mark"
        );

        let mut cloned = app();
        cloned.handle_key(ctrl('`'));
        assert_eq!(
            heads(&cloned),
            vec!["a", "a", "b", "c"],
            "Ctrl-` did not clone"
        );

        let mut inserted = app();
        inserted.handle_key(ctrl('i'));
        assert_eq!(
            inserted.mode,
            Mode::Headline,
            "Ctrl-I did not insert a node"
        );

        // Ctrl-] demotes the following siblings, Ctrl-[ promotes the
        // children back out. `a` already has one child, which comes with them.
        let mut moved = app();
        moved.handle_key(ctrl(']'));
        assert_eq!(heads(&moved), vec!["a", "  a1", "  b", "  c"]);
        moved.handle_key(ctrl('['));
        assert_eq!(heads(&moved), vec!["a", "a1", "b", "c"]);
    }

    #[test]
    fn leos_shift_ctrl_z_redoes_in_both_panes() {
        let redo = KeyEvent::new(
            KeyCode::Char('z'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        let mut app = app();
        press(&mut app, "J");
        assert_eq!(heads(&app), vec!["b", "a", "c"]);
        press(&mut app, "u");
        assert_eq!(heads(&app), vec!["a", "b", "c"]);
        app.handle_key(redo);
        assert_eq!(heads(&app), vec!["b", "a", "c"]);

        // The body has its own grammar, and must agree.
        let mut app = body_app("one\ntwo\n");
        press(&mut app, "dd");
        assert_eq!(body(&app), "two\n");
        press(&mut app, "u");
        assert_eq!(body(&app), "one\ntwo\n");
        app.handle_key(redo);
        assert_eq!(body(&app), "two\n");
    }

    #[test]
    fn leos_shift_arrows_move_the_node() {
        let mut app = app();
        press(&mut app, "Shift-Down");
        assert_eq!(heads(&app), vec!["b", "a", "c"]);
        press(&mut app, "Shift-Up");
        assert_eq!(heads(&app), vec!["a", "b", "c"]);
    }

    #[test]
    fn typing_a_headline_lands_in_the_model() {
        let mut app = app();
        press(&mut app, "e");
        assert_eq!(app.mode, Mode::Headline);
        app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        for ch in "xyz".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.current.h(app.outline()), "xyz");
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn escape_commits_a_body_edit() {
        // Design Q2: Escape commits, and undo is what takes it back.
        let mut app = app();
        press(&mut app, "Tab");
        press(&mut app, "i");
        assert_eq!(app.mode, Mode::Insert);
        for ch in "hello".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.current.b(app.outline()), "hello\n");
        assert_eq!(app.mode, Mode::Normal);
        press(&mut app, "u");
        assert_eq!(app.current.b(app.outline()), "");
    }

    #[test]
    fn ctrl_c_abandons_a_body_edit() {
        let mut app = app();
        press(&mut app, "Tab");
        press(&mut app, "i");
        for ch in "hello".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(app.current.b(app.outline()), "");
        assert!(!app.doc.undoer.can_undo());
    }

    #[test]
    fn the_z_family_folds() {
        let mut app = app();
        press(&mut app, "zR");
        assert_eq!(heads(&app), vec!["a", "  a1", "    a2", "b", "c"]);
        press(&mut app, "zM");
        assert_eq!(heads(&app), vec!["a", "b", "c"]);
        press(&mut app, "z2");
        assert_eq!(heads(&app), vec!["a", "  a1", "b", "c"]);
    }

    #[test]
    fn marks_can_be_walked_and_cleared() {
        let mut app = app();
        press(&mut app, "m");
        press(&mut app, "2j");
        press(&mut app, "m");
        press(&mut app, "]m");
        assert_eq!(app.current.h(app.outline()), "a");
        press(&mut app, "M");
        assert!(app.message.contains("unmarked 2"), "{}", app.message);
    }

    #[test]
    fn the_help_overlay_takes_the_keyboard_and_gives_it_back() {
        let mut app = app();
        press(&mut app, "F1");
        assert_eq!(app.mode, Mode::Help);
        // `j` scrolls the help, not the outline.
        press(&mut app, "j");
        assert_eq!(app.help_scroll, 1);
        assert_eq!(app.current.h(app.outline()), "a");
        press(&mut app, "q");
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn quitting_with_unsaved_changes_asks_first() {
        let mut app = app();
        let p = app.current.clone();
        app.doc.set_headline(&p, "changed");
        press(&mut app, "q");
        assert_eq!(app.mode, Mode::Confirm);
        assert!(!app.quit);
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.quit);
        press(&mut app, "q");
        app.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.quit);
    }

    #[test]
    fn writing_over_an_unread_file_asks_first() {
        let dir = std::env::temp_dir().join(format!("leotui-overwrite-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("plain.py");
        let mine = "print('mine')\n";
        std::fs::write(&file, mine).unwrap();
        let mut doc = Document::new_empty("");
        let root = doc.outline.root_position().unwrap();
        doc.set_headline(&root, &format!("@file {}", file.display()));
        doc.set_body(&root, "print('ours')\n");
        let mut app = App::new(doc);

        app.write_external();
        assert_eq!(app.mode, Mode::Confirm);
        assert!(
            app.mini_label().contains("plain.py"),
            "{}",
            app.mini_label()
        );
        type_text(&mut app, "n");
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), mine);
        assert_eq!(app.message, "not overwritten: 1 unread file(s)");

        app.write_external();
        type_text(&mut app, "y");
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let written = std::fs::read_to_string(&file).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(written.contains("print('ours')"), "{written}");
        assert!(written.contains("@+leo"), "{written}");
        assert_eq!(app.message, "wrote 1");
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn import_at_file_asks_before_adding_sentinels() {
        let dir = std::env::temp_dir().join(format!("leotui-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("x.py");
        std::fs::write(&file, "#!/bin/sh\nx = 1\n").unwrap();
        let mut app = app();
        press(&mut app, ":");
        type_text(&mut app, &format!("import-at-file {}", file.display()));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.mode, Mode::Confirm, "{}", app.message);
        assert!(app.current.h(app.outline()).starts_with("@file "));
        type_text(&mut app, "y");
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let written = std::fs::read_to_string(&file).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(
            written.starts_with("#!/bin/sh\n# @+leo-ver=5-thin\n"),
            "{written}"
        );
        assert_eq!(app.message, "wrote 1");
    }

    #[test]
    fn the_breadcrumb_names_the_path_to_the_node() {
        let mut app = app();
        press(&mut app, "l");
        press(&mut app, "l");
        assert_eq!(app.current.h(app.outline()), "a1");
        assert_eq!(app.breadcrumb(), "a > a1");
    }

    /// Type a literal string into the minibuffer.
    fn type_text(app: &mut App, text: &str) {
        for ch in text.chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
    }

    #[test]
    fn the_command_line_runs_a_leo_command_by_name() {
        let mut app = app();
        press(&mut app, ":");
        assert_eq!(app.mode, Mode::Command);
        type_text(&mut app, "move-outline-down");
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(heads(&app), vec!["b", "a", "c"]);
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn the_command_line_takes_vim_spellings() {
        let mut app = app();
        let p = app.current.clone();
        app.doc.set_headline(&p, "changed");
        press(&mut app, ":");
        type_text(&mut app, "q");
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        // `:q` refuses while there are unsaved changes.
        assert!(!app.quit);
        assert_eq!(app.mode, Mode::Confirm);
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        press(&mut app, ":");
        type_text(&mut app, "q!");
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.quit);
    }

    #[test]
    fn an_unknown_command_says_so() {
        let mut app = app();
        press(&mut app, ":");
        type_text(&mut app, "not-a-command");
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.message.contains("no such command"), "{}", app.message);
    }

    #[test]
    fn tab_completes_a_command_name() {
        let mut app = app();
        press(&mut app, ":");
        type_text(&mut app, "unmark");
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.mini.as_ref().unwrap().buffer, "unmark-all");
    }

    #[test]
    fn the_command_line_remembers_what_was_typed() {
        let mut app = app();
        press(&mut app, ":");
        type_text(&mut app, "undo");
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        press(&mut app, ":");
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.mini.as_ref().unwrap().buffer, "undo");
    }

    #[test]
    fn a_number_selects_that_visible_row() {
        let mut app = app();
        press(&mut app, ":");
        type_text(&mut app, "3");
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.current.h(app.outline()), "c");
    }

    #[test]
    fn search_moves_as_it_is_typed_and_escape_puts_it_back() {
        let mut app = app();
        press(&mut app, "/");
        assert_eq!(app.mode, Mode::Search);
        type_text(&mut app, "c");
        assert_eq!(app.current.h(app.outline()), "c");
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.current.h(app.outline()), "a");
    }

    #[test]
    fn enter_keeps_the_match_and_n_repeats_it() {
        let mut app = app();
        // Two nodes match "a": the root and a1.
        press(&mut app, "zR");
        press(&mut app, "/");
        type_text(&mut app, "a1");
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.current.h(app.outline()), "a1");
        press(&mut app, "n");
        // Only one node matches, so `n` wraps back to it.
        assert_eq!(app.current.h(app.outline()), "a1");
    }

    #[test]
    fn search_is_case_insensitive_until_a_capital_is_typed() {
        let mut app = app();
        let p = app.current.clone();
        app.doc.set_headline(&p, "Alpha");
        press(&mut app, "/");
        type_text(&mut app, "alpha");
        assert_eq!(app.current.h(app.outline()), "Alpha");
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        press(&mut app, "/");
        type_text(&mut app, "ALPHA");
        assert!(app.message.contains("not found"), "{}", app.message);
    }

    #[test]
    fn escape_puts_back_the_line_and_the_theme_a_preview_replaced() {
        let mut app = app();
        press(&mut app, ":");
        app.theme_names = std::rc::Rc::new(
            ["onedark", "onedarker"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        );
        type_text(&mut app, "theme one");
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.mini.as_ref().unwrap().buffer, "theme onedark");
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.mini.is_none());
        // Neither theme is on this machine's disk, so the built-in stands.
        assert_eq!(app.theme.name(), "builtin");
    }

    #[test]
    fn tab_completes_a_theme_name_without_disturbing_the_command() {
        let mut app = app();
        press(&mut app, ":");
        app.theme_names = std::rc::Rc::new(vec!["onelight".to_string()]);
        type_text(&mut app, "theme onel");
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.mini.as_ref().unwrap().buffer, "theme onelight");
    }

    #[test]
    fn the_arrows_move_the_drop_down_and_otherwise_walk_the_history() {
        let mut app = app();
        app.command_history = vec!["save".to_string()];
        // No drop-down: Up is the history, as it was.
        press(&mut app, ":");
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.mini.as_ref().unwrap().buffer, "save");
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        // A drop-down: Up and Down move through it instead.
        press(&mut app, ":");
        app.theme_names = std::rc::Rc::new(
            ["onedark", "onelight"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        );
        type_text(&mut app, "theme one");
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.mini.as_ref().unwrap().buffer, "theme onedark");
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.mini.as_ref().unwrap().buffer, "theme onelight");
        app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.mini.as_ref().unwrap().buffer, "theme onedark");
    }

    #[test]
    fn a_theme_that_does_not_load_is_not_saved() {
        // `main` sets the path; an `App` a test builds has none.
        assert!(app().config_path.is_none());
        let path = std::env::temp_dir()
            .join(format!("leotui-app-{}", std::process::id()))
            .join("config.toml");
        let mut app = app();
        app.config_path = Some(path.clone());
        app.run_command_line("theme no-such-theme-anywhere");
        assert!(app.message.contains("not found"), "{}", app.message);
        assert!(!path.exists(), "a failed :theme wrote the settings file");
    }

    #[test]
    fn a_file_that_was_not_read_is_reported() {
        use leolib::external::{FileReport, ReadResult};
        let failure = |path: &str| FileReport {
            headline: format!("@file {path}"),
            path: path.to_string(),
            error: leolib::Error::NotAnExternalFile {
                path: path.to_string(),
            },
        };
        assert_eq!(read_report_message(&ReadResult::default()), None);
        let one = ReadResult {
            errors: vec![failure("a.py")],
            ..Default::default()
        };
        assert_eq!(
            read_report_message(&one).as_deref(),
            Some("external file not read: not a valid external file: a.py")
        );
        let two = ReadResult {
            errors: vec![failure("a.py"), failure("b.py")],
            ..Default::default()
        };
        assert_eq!(
            read_report_message(&two).as_deref(),
            Some("2 external files not read; first: not a valid external file: a.py")
        );
    }

    #[test]
    fn set_colors_takes_the_three_depths_and_refuses_the_rest() {
        let mut app = app();
        app.run_command_line("set colors=16");
        assert_eq!(app.depth, crate::theme::Depth::Ansi16);
        app.run_command_line("set colors=true");
        assert_eq!(app.depth, crate::theme::Depth::True);
        app.run_command_line("set colors=lots");
        assert!(
            app.message.contains("must be true, 256 or 16"),
            "{}",
            app.message
        );
        // A refused value leaves the depth alone.
        assert_eq!(app.depth, crate::theme::Depth::True);
    }

    #[test]
    fn theme_names_the_current_one_and_reports_a_missing_one() {
        let mut app = app();
        app.run_command_line("theme");
        assert!(app.message.contains("builtin"), "{}", app.message);
        app.run_command_line("theme no-such-theme-anywhere");
        assert!(app.message.contains("not found"), "{}", app.message);
        // The theme that was working stays working.
        assert_eq!(app.theme.name(), "builtin");
    }

    #[test]
    fn set_search_headlines_leaves_bodies_out() {
        let mut app = app();
        let p = app.rows()[2].position.clone();
        app.doc.set_body(&p, "a needle\n");
        press(&mut app, "/");
        type_text(&mut app, "needle");
        assert_eq!(app.current.h(app.outline()), "c");
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.run_command_line("set search=headlines");
        press(&mut app, "/");
        type_text(&mut app, "needle");
        assert!(app.message.contains("not found"), "{}", app.message);
    }

    #[test]
    fn a_search_from_the_outline_lands_in_a_body_and_n_steps_through() {
        let mut app = app();
        let rows = app.rows();
        let (b, c) = (rows[1].position.clone(), rows[2].position.clone());
        app.doc.set_body(&b, "one needle\n");
        app.doc.set_body(&c, "needle\nno\nneedle needle\n");
        press(&mut app, "/");
        type_text(&mut app, "needle");
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.current.h(app.outline()), "b");
        assert_eq!(app.focus, Focus::Body);
        assert_eq!(app.editor.cursor, (0, 4));
        assert!(app.hlsearch.is_some());
        press(&mut app, "n");
        assert_eq!(app.current.h(app.outline()), "c");
        assert_eq!(app.editor.cursor, (0, 0));
        press(&mut app, "n");
        assert_eq!(app.editor.cursor, (2, 0));
        press(&mut app, "n");
        assert_eq!(app.editor.cursor, (2, 7));
        press(&mut app, "n");
        assert_eq!(app.current.h(app.outline()), "b");
        assert_eq!(app.message, "search hit BOTTOM, continuing at TOP");
        press(&mut app, "N");
        assert_eq!(app.current.h(app.outline()), "c");
        assert_eq!(app.editor.cursor, (2, 7));
        assert_eq!(app.message, "search hit TOP, continuing at BOTTOM");
        app.run_command_line("noh");
        assert!(app.hlsearch.is_none());
        press(&mut app, "n");
        assert!(app.hlsearch.is_some(), "n highlights again after :noh");
    }

    #[test]
    fn escape_puts_back_the_node_the_pane_and_the_highlight() {
        let mut app = app();
        let c = app.rows()[2].position.clone();
        app.doc.set_body(&c, "a needle\n");
        press(&mut app, "/");
        type_text(&mut app, "needle");
        assert_eq!(app.current.h(app.outline()), "c");
        assert_eq!(app.focus, Focus::Body);
        assert!(app.hlsearch.is_some());
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.current.h(app.outline()), "a");
        assert_eq!(app.focus, Focus::Tree);
        assert!(app.hlsearch.is_none());
    }

    #[test]
    fn set_changes_the_split_and_refuses_nonsense() {
        let mut app = app();
        app.run_command_line("set split=30");
        assert_eq!(app.tree_percent, 30);
        app.run_command_line("set split=nope");
        assert!(app.message.contains("not a number"), "{}", app.message);
        app.run_command_line("set frobnicate");
        assert!(app.message.contains("unknown option"), "{}", app.message);
    }

    #[test]
    fn set_takes_several_options_and_shows_values() {
        let mut app = app();
        app.run_command_line("set nowrap number split:40");
        assert!(app.options.number && !app.options.wrap);
        assert_eq!(app.tree_percent, 40);
        app.run_command_line("set split?");
        assert_eq!(app.message, "split=40");
        app.run_command_line("set split");
        assert_eq!(app.message, "split=40");
        app.run_command_line("set nonumber bogus wrap");
        assert!(!app.options.number);
        assert!(!app.options.wrap, "an error stops the rest");
        assert!(
            app.message.contains("unknown option: bogus"),
            "{}",
            app.message
        );
        app.run_command_line("set");
        assert!(
            app.message
                .starts_with("search=all  split=40  nowrap  nonumber"),
            "{}",
            app.message
        );
    }

    #[test]
    fn substitute_changes_the_body_as_one_undoable_step() {
        let mut app = app();
        let p = app.current.clone();
        app.doc.set_body(&p, "one fish\ntwo fish\n");
        app.doc.undoer.clear();
        // Typed, as a user would: `%` and `/` must reach the line intact.
        press(&mut app, ":");
        type_text(&mut app, "%s/fish/cat/");
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.current.b(app.outline()), "one cat\ntwo cat\n");
        assert_eq!(app.message, "2 substitutions on 2 lines");
        assert_eq!(app.editor.cursor.0, 1);
        app.run_command_line("undo");
        assert_eq!(app.current.b(app.outline()), "one fish\ntwo fish\n");
        app.run_command_line("s/nothing/x/");
        assert!(app.message.contains("pattern not found"), "{}", app.message);
        app.run_command_line("substitute");
        assert!(app.message.starts_with("usage:"), "{}", app.message);
    }

    #[test]
    fn bufdo_substitutes_in_every_body_as_one_undo_step() {
        let mut app = app();
        let rows = app.rows();
        let (a, b) = (rows[0].position.clone(), rows[1].position.clone());
        app.doc.set_body(&a, "old one\n");
        app.doc.set_body(&b, "old two old\n");
        app.doc.undoer.clear();
        app.run_command_line("bufdo %s/old/new/g");
        assert_eq!(a.b(app.outline()), "new one\n");
        assert_eq!(b.b(app.outline()), "new two new\n");
        assert_eq!(app.message, "3 substitutions on 2 lines in 2 nodes");
        app.run_command_line("undo");
        assert_eq!(a.b(app.outline()), "old one\n");
        assert_eq!(b.b(app.outline()), "old two old\n");
        app.run_command_line("bufdo s/old/new/");
        assert!(app.message.contains("range"), "{}", app.message);
        app.run_command_line("bufdo set wrap");
        assert!(app.message.contains("only :s"), "{}", app.message);
        app.run_command_line("bufdo %s/absent/x/");
        assert!(app.message.contains("pattern not found"), "{}", app.message);
    }

    #[test]
    fn bufdo_changes_a_cloned_body_once() {
        let mut app = app();
        let p = app.current.clone();
        app.doc.set_body(&p, "a\n");
        app.doc.clone_node(&p);
        app.run_command_line("bufdo %s/a/aa/");
        assert_eq!(p.b(app.outline()), "aa\n");
        assert_eq!(app.message, "1 substitution on 1 line in 1 node");
    }

    #[test]
    fn ctrl_w_resizes_the_pane_that_has_focus() {
        let mut app = app();
        // Relative to the default, which is a matter of taste.
        let start = app.tree_percent;
        press(&mut app, "Ctrl-w <");
        assert_eq!(app.tree_percent, start - 5, "the outline narrows");
        press(&mut app, "Tab");
        assert_eq!(app.focus, Focus::Body);
        press(&mut app, "Ctrl-w >");
        assert_eq!(app.tree_percent, start - 10, "the body widens");
        press(&mut app, "Ctrl-w <");
        assert_eq!(app.tree_percent, start - 5, "the body narrows");
    }

    #[test]
    fn set_split_saves_the_ratio_and_the_keys_do_not() {
        let dir = std::env::temp_dir().join(format!("leotui-split-{}", std::process::id()));
        let path = dir.join("config.toml");
        let mut app = app();
        app.config_path = Some(path.clone());
        app.run_command_line("set split=30");
        let saved = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(saved, "split-ratio = 30\n");
        assert_eq!(app.message, "split: 30% (saved)");
        press(&mut app, "Ctrl-Left");
        assert_eq!(app.tree_percent, 25);
        assert!(!path.exists(), "Ctrl-Left wrote the settings file");
    }

    #[test]
    fn help_for_one_command_names_its_keys() {
        let mut app = app();
        app.run_command_line("help move-outline-down");
        assert!(app.message.contains("move-outline-down"), "{}", app.message);
        assert!(app.message.contains("Shift-Down"), "{}", app.message);
    }

    /// An app whose current node has the given body, focused on it.
    fn body_app(text: &str) -> App {
        let mut app = app();
        let p = app.current.clone();
        app.doc.set_body(&p, text);
        app.doc.undoer.clear();
        press(&mut app, "Tab");
        app
    }

    fn body(app: &App) -> String {
        app.current.b(app.outline()).to_string()
    }

    #[test]
    fn an_operator_and_a_motion_delete_a_word() {
        let mut app = body_app("one two three\n");
        press(&mut app, "dw");
        assert_eq!(body(&app), "two three\n");
        assert_eq!(app.editor.register.text, "one ");
    }

    #[test]
    fn a_count_applies_to_the_operator() {
        let mut app = body_app("one two three four\n");
        press(&mut app, "d2w");
        assert_eq!(body(&app), "three four\n");
    }

    #[test]
    fn a_doubled_operator_takes_whole_lines() {
        let mut app = body_app("one\ntwo\nthree\n");
        press(&mut app, "dd");
        assert_eq!(body(&app), "two\nthree\n");
        press(&mut app, "2dd");
        assert_eq!(body(&app), "");
    }

    #[test]
    fn a_text_object_changes_the_word_under_the_cursor() {
        let mut app = body_app("alpha beta gamma\n");
        press(&mut app, "w");
        press(&mut app, "ciw");
        assert_eq!(app.mode, Mode::Insert);
        type_text(&mut app, "BETA");
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(body(&app), "alpha BETA gamma\n");
    }

    #[test]
    fn a_quoted_object_takes_what_is_inside() {
        let mut app = body_app("say \"hello there\" now\n");
        press(&mut app, "fh");
        press(&mut app, "di\"");
        assert_eq!(body(&app), "say \"\" now\n");
    }

    #[test]
    fn yank_and_put_move_text() {
        let mut app = body_app("one\ntwo\n");
        press(&mut app, "yy");
        press(&mut app, "p");
        assert_eq!(body(&app), "one\none\ntwo\n");
    }

    #[test]
    fn visual_selects_and_an_operator_takes_it() {
        let mut app = body_app("one\ntwo\nthree\n");
        press(&mut app, "V");
        assert_eq!(app.mode, Mode::Visual);
        press(&mut app, "j");
        press(&mut app, "d");
        assert_eq!(body(&app), "three\n");
        assert_eq!(app.mode, Mode::Normal);
    }

    #[test]
    fn escape_leaves_visual_before_it_leaves_the_pane() {
        let mut app = body_app("one\ntwo\n");
        press(&mut app, "v");
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.focus, Focus::Body);
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.focus, Focus::Tree);
    }

    #[test]
    fn dot_repeats_the_last_change() {
        let mut app = body_app("one two three four\n");
        press(&mut app, "dw");
        assert_eq!(body(&app), "two three four\n");
        press(&mut app, ".");
        assert_eq!(body(&app), "three four\n");
    }

    #[test]
    fn dot_repeats_a_change_operator_and_its_text() {
        let mut app = body_app("aa bb\ncc dd\n");
        press(&mut app, "cw");
        type_text(&mut app, "xx");
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(body(&app), "xx bb\ncc dd\n");
        press(&mut app, "j0");
        press(&mut app, ".");
        assert_eq!(body(&app), "xx bb\nxx dd\n");
    }

    #[test]
    fn one_insert_session_is_one_undo() {
        let mut app = body_app("x\n");
        press(&mut app, "A");
        type_text(&mut app, "yz");
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(body(&app), "xyz\n");
        press(&mut app, "u");
        assert_eq!(body(&app), "x\n");
    }

    #[test]
    fn a_count_repeats_an_insert() {
        let mut app = body_app("\n");
        press(&mut app, "3i");
        type_text(&mut app, "ab");
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(body(&app), "ababab\n");
    }

    #[test]
    fn simple_edits_work_and_undo() {
        let mut app = body_app("abc\n");
        press(&mut app, "x");
        assert_eq!(body(&app), "bc\n");
        press(&mut app, "rZ");
        assert_eq!(body(&app), "Zc\n");
        press(&mut app, "~");
        assert_eq!(body(&app), "zc\n");
        press(&mut app, "u");
        press(&mut app, "u");
        press(&mut app, "u");
        assert_eq!(body(&app), "abc\n");
    }

    #[test]
    fn join_puts_one_space_between_the_lines() {
        let mut app = body_app("one\n   two\n");
        press(&mut app, "J");
        assert_eq!(body(&app), "one two\n");
    }

    #[test]
    fn indent_operators_shift_whole_lines() {
        let mut app = body_app("a\nb\n");
        press(&mut app, ">>");
        assert_eq!(body(&app), "    a\nb\n");
        press(&mut app, "<<");
        assert_eq!(body(&app), "a\nb\n");
    }

    #[test]
    fn case_operators_take_a_motion() {
        let mut app = body_app("hello world\n");
        press(&mut app, "gUw");
        assert_eq!(body(&app), "HELLO world\n");
    }

    #[test]
    fn a_body_search_puts_the_cursor_on_the_match() {
        let mut app = body_app("alpha\nbeta\ngamma\n");
        press(&mut app, "/");
        type_text(&mut app, "mm");
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.editor.cursor, (2, 2));
        assert_eq!(app.focus, Focus::Body);
        // The outline did not move.
        assert_eq!(app.current.h(app.outline()), "a");
    }

    #[test]
    fn moving_to_another_node_resets_the_cursor() {
        let mut app = body_app("one\ntwo\n");
        press(&mut app, "j");
        assert_eq!(app.editor.cursor.0, 1);
        press(&mut app, "Tab");
        press(&mut app, "j");
        press(&mut app, "Tab");
        assert_eq!(app.editor.cursor, (0, 0));
    }

    #[test]
    fn the_pane_split_can_be_resized() {
        let mut app = app();
        let before = app.tree_percent;
        press(&mut app, "Ctrl-Right");
        assert!(app.tree_percent > before);
        press(&mut app, "Ctrl-Left");
        assert_eq!(app.tree_percent, before);
    }
}
