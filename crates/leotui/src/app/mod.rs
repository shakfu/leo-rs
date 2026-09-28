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
use crate::editor::motion::{Kind, Motion};
use crate::editor::parse::{Action, Parser};
use crate::editor::{self, Editor};
use crate::history::NodeHistory;
use crate::keys::{self, Key, Pending};
use std::rc::Rc;

use crate::minibuffer::{self, MiniKind, Minibuffer};
use crate::search::{self, Direction, LastSearch, Scope};

mod body;
mod ex;
mod files;
mod hoist;
mod prompt;
#[cfg(test)]
mod tests;

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
    history: NodeHistory,
    /// Leo's `hoistStack`: each hoisted node, and whether it was expanded.
    hoists: Vec<(Position, bool)>,
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
    /// Files to read over unwritten edits, waiting on the y/n prompt.
    pending_read: Vec<Position>,
    /// Whether the save waiting on the y/n prompt also writes external files.
    /// False only for `write-outline-only`.
    save_files: bool,
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
            history: NodeHistory::default(),
            hoists: Vec::new(),
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
            pending_read: Vec::new(),
            save_files: true,
        };
        app.expand_ancestors();
        app.history.update(&app.current);
        app
    }

    pub fn outline(&self) -> &Outline {
        &self.doc.outline
    }

    // --- Dispatch --------------------------------------------------------

    /// Feed one key to the current mode.
    pub fn handle_key(&mut self, event: KeyEvent) {
        self.message.clear();
        if event.code == KeyCode::Char('c') && event.modifiers.contains(KeyModifiers::CONTROL) {
            return self.interrupt();
        }
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

    // --- The outline pane ------------------------------------------------

    /// The rows the outline pane shows: the tree with folded subtrees
    /// skipped, or only the hoisted subtree, with the hoisted node at depth 0.
    pub fn rows(&self) -> Vec<Row> {
        let o = self.outline();
        let mut rows = Vec::new();
        let limit = self.hoist_limit();
        let base = limit.map_or(0, |h| h.level());
        let mut p = limit.cloned().or_else(|| o.root_position());
        while let Some(cur) = p.filter(|p| self.in_view(p)) {
            let has_children = cur.has_children(o);
            let expanded = o.is_expanded(&cur);
            rows.push(Row {
                depth: cur.level() - base,
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

    // --- Selection -------------------------------------------------------

    pub fn select(&mut self, p: Position) {
        self.dehoist_to_show(&p);
        self.history.update(&p);
        self.current = p;
        self.body_scroll = 0;
        // The body is a different buffer now.
        self.buffer = None;
        self.editor.cursor = (0, 0);
        self.editor.desired_col = 0;
        self.editor.visual = None;
        self.expand_ancestors();
    }

    /// Leo's `go-back` (`step` -1) and `go-forward` (+1), `count` times.
    pub fn go_history(&mut self, step: isize, count: usize) {
        for _ in 0..count {
            let Some(p) = self.history.step(&self.doc.outline, step) else {
                self.message = "no more history".to_string();
                return;
            };
            self.select(p);
        }
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
        let p = self.current.clone();
        self.dehoist_to_show(&p);
    }

    /// Move by a fraction of the visible pane, in whichever pane has focus.
    pub fn page(&mut self, fraction: f32) {
        match self.focus {
            Focus::Tree => {
                let step = ((self.tree_height as f32) * fraction) as i32;
                self.move_rows(step);
            }
            // As vim's Ctrl-d: the view and the cursor move together.
            Focus::Body => {
                let step = ((self.body_height as f32) * fraction) as isize;
                let lines = self.body_buffer();
                let last = lines.len().saturating_sub(1);
                self.body_scroll = self.body_scroll.saturating_add_signed(step).min(last);
                let row = self.editor.cursor.0;
                let (motion, n) = if step >= 0 {
                    (
                        Motion::Down,
                        step.unsigned_abs().min(last.saturating_sub(row)),
                    )
                } else {
                    (Motion::Up, step.unsigned_abs().min(row))
                };
                let screen = (self.body_scroll, self.body_height);
                self.editor.move_by(&lines, motion, n, screen);
                self.scroll_to_cursor();
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

    // --- Folding ---------------------------------------------------------

    /// Leo's left arrow: fold this node, or step out to the parent.
    pub fn contract_or_go_left(&mut self) {
        let p = self.current.clone();
        if p.has_children(self.outline()) && self.outline().is_expanded(&p) {
            self.doc.outline.contract(&p);
        } else if let Some(parent) = p.parent(self.outline()).filter(|q| self.in_view(q)) {
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

    /// Ctrl-c, in every mode: close what is open, keeping typed text, then
    /// ask to quit. Whoever presses it most likely wants out, not a lost edit.
    fn interrupt(&mut self) {
        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        match self.mode {
            // A question is open; Ctrl-c answers it as Escape does.
            Mode::Confirm => return self.mini_key(esc),
            Mode::Headline
                if self
                    .mini
                    .as_ref()
                    .is_some_and(|m| m.kind == MiniKind::Headline) =>
            {
                self.finish_mini(true)
            }
            Mode::Headline | Mode::Command | Mode::Search => self.mini_key(esc),
            Mode::Insert => self.insert_key(esc),
            Mode::Visual => self.body_key(esc),
            Mode::Help => self.mode = Mode::Normal,
            Mode::Normal => {}
        }
        self.request_quit();
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

/// Keep the newest entry once, at the end.
fn remember(history: &mut Vec<String>, entry: &str) {
    history.retain(|e| e != entry);
    history.push(entry.to_string());
}
