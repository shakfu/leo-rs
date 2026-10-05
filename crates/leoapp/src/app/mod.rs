//! The view's state, and the dispatcher that turns keys into commands.
//!
//! Everything that changes the outline goes through `leolib::Document`, so an
//! edit lands in the model and its undo history, never in a widget the model
//! then has to be told about. This file holds no copy of the outline.

use leolib::{Document, Outline, Position};

use crate::bindings;
use crate::commands;
use crate::editor::change::{Change, InsertAt, Operator, Range};
use crate::editor::motion::{Kind, Motion};
use crate::editor::parse::{Action, Parser};
use crate::editor::{self, Editor};
use crate::history::NodeHistory;
use crate::keys::{self, Key, KeyCode, KeyEvent, KeyModifiers, Pending};
use std::rc::Rc;

use crate::minibuffer::{self, MiniKind, Minibuffer};
use crate::search::{self, Direction, LastSearch, Scope};

mod body;
mod ex;
mod files;
mod find;
mod hoist;
mod lsp;
mod mcp;
pub use lsp::{diagnostic_line, CODE_ACTIONS};
pub use mcp::Access;
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
#[derive(Clone)]
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
    /// The unfolded nodes, since landing on a match unfolds its ancestors.
    expanded: std::collections::HashSet<String>,
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
    /// First screen column the body shows, when lines are not wrapped.
    pub body_hscroll: usize,
    /// The body cursor and scroll of each node visited, as Leo's `insertSpot`.
    body_spots: std::collections::HashMap<leolib::VnodeId, ((usize, usize), usize)>,
    pub help_scroll: usize,
    /// Width of the outline pane, as a percentage.
    pub tree_percent: u16,
    /// How deep `expand-next-level` has unfolded, and from which node.
    pub expansion_level: usize,
    expansion_node: Option<Position>,
    pub message: String,
    /// Every message shown, oldest first, for `:messages`.
    pub messages: Vec<String>,
    /// Lines the help overlay shows in place of the bindings, and its title.
    pub overlay: Option<(String, Vec<String>)>,
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
    /// Files to read over unwritten edits, waiting on the y/n prompt, and
    /// whether the read is a `refresh-from-disk`.
    pending_read: (Vec<Position>, bool),
    /// Whether the save waiting on the y/n prompt also writes external files.
    /// False only for `write-outline-only`.
    save_files: bool,
    /// Language servers, when the settings name any.
    pub lsp: Option<leolsp::Lsp>,
    /// The outline generation and node the servers last saw.
    lsp_synced: Option<(u64, leolib::VnodeId)>,
    /// When the outline first changed after the last sync.
    lsp_dirty_since: Option<std::time::Instant>,
    /// The current node's diagnostics, as of the last poll.
    pub diagnostics: Vec<leolsp::BodyDiagnostic>,
    row_cache: std::cell::RefCell<RowCache>,
    /// The settings key an accepted `:theme` is saved under: `theme`, or
    /// leoegui's `theme-light` while it is light.
    pub theme_setting: &'static str,
    /// The MCP server, when the settings turn it on, and what its clients
    /// may do.
    pub mcp: Option<leomcp::Server>,
    pub mcp_access: Access,
    /// The settings as read at launch and changed since, for a front end
    /// that shows them.
    pub settings: crate::config::Config,
    /// The code actions last offered, and the outline generation they were
    /// offered at: a later edit makes them stale.
    pub code_actions: Vec<leolsp::CodeAction>,
    code_actions_at: u64,
    /// The code action Enter applies; the arrows move it.
    pub code_action_selected: usize,
    /// External files whose last read failed, by full path, and why.
    pub unread: std::collections::HashMap<String, String>,
    /// External files `check_disk` found changed by another program, by
    /// full path. Each is checked again when asked about.
    changed_on_disk: Vec<String>,
}

/// Why an `@<file>` node's file needs attention. A node in several states
/// shows the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileState {
    /// The last read failed, so the node does not hold the file.
    Unread,
    /// Another program changed the file since the outline read or wrote it.
    ChangedOnDisk,
    /// The file exists but was never read, so a write is refused.
    Refused,
    /// The tree has edits the file does not.
    Unwritten,
}

/// The status line's account of external files that could not be read.
///
/// A failed read leaves an `@file` node empty. Saying nothing would present
/// that empty node as the file's contents.
pub fn read_report_message(report: &leolib::external::ReadResult) -> Option<String> {
    if let Some(first) = report.errors.first() {
        return Some(match report.errors.len() {
            1 => format!("external file not read: {}", first.error),
            n => format!("{n} external files not read; first: {}", first.error),
        });
    }
    let first = report.warnings.first()?;
    Some(match report.warnings.len() {
        1 => format!("{}: {}", first.headline, first.message),
        n => format!("{n} external files will change on the next write; :messages lists them"),
    })
}

/// Open the outline at `path`, or start a new one there if no file exists,
/// as vim does. The flag says which.
pub fn open_or_new(path: &str, read_external: bool) -> leolib::Result<(Document, bool)> {
    match Document::open(path, read_external) {
        Err(leolib::Error::NotFound { path }) => Ok((Document::new_empty(&path), true)),
        other => other.map(|doc| (doc, false)),
    }
}

/// The theme loaded when neither `--theme` nor the config file names one.
///
/// A Helix theme name: leoapp reads their files and vendors none, so this is
/// only a default and holds whatever the user has on disk under that name.
pub const DEFAULT_THEME: &str = "sonokai";

/// Open `path`, or an unsaved outline without one, as a front end starts:
/// settings and theme applied, the top level unfolded, and the most urgent
/// message on the status line.
pub fn launch(
    path: Option<&str>,
    read_external: bool,
    theme: Option<&str>,
) -> leolib::Result<(App, crate::config::Config)> {
    let (doc, new) = match path {
        Some(path) => open_or_new(path, read_external)?,
        None => (Document::new_empty(""), false),
    };
    let mut app = App::new(doc);
    app.config_path = crate::config::path();
    // A theme asked for by name, on the command line or in the config file,
    // reports when it is missing. The default does not: the built-in sixteen
    // colours stand, and saying so on every start would be noise.
    let settings = crate::config::load();
    let (name, asked) = crate::config::chosen_theme(theme, &settings, DEFAULT_THEME);
    if let Some(percent) = settings.split_ratio {
        app.tree_percent = percent;
    }
    if !app.set_theme(name) && !asked {
        app.message.clear();
    }
    if app.message.is_empty() {
        if let Some(warning) = settings.warnings.first() {
            app.message = warning.clone();
        }
    }
    if new {
        app.message = format!("new outline: {}", app.outline().file_name);
    }
    // A file that could not be read outranks a theme or a settings message:
    // its node is empty, and would otherwise pass for the file's contents.
    if let Some(report) = read_report_message(&app.doc.read_report) {
        app.message = report;
    }
    for line in read_report_lines(&app.doc.read_report) {
        app.log(line);
    }
    app.log_message();

    if let Some(b) = settings.number {
        app.options.number = b;
    }
    if let Some(b) = settings.wrap {
        app.options.wrap = b;
    }
    if let Some(b) = settings.syntax {
        app.options.syntax = b;
    }
    let mut settings = settings;
    app.start_mcp(&mut settings);
    app.set_lsp(&settings, std::sync::Arc::new(|| {}));

    // Unfold the top level, so an outline opens showing something.
    if let Some(root) = app.outline().root_position() {
        for p in root.self_and_siblings(app.outline()) {
            app.doc.outline_mut_untracked().expand(&p);
        }
    }
    app.settings = settings.clone();
    Ok((app, settings))
}

impl App {
    /// Start the MCP server if the settings turn it on, making and saving a
    /// token the first time. A failure is said on the status line.
    pub fn start_mcp(&mut self, settings: &mut crate::config::Config) {
        if settings.mcp.enabled && settings.mcp.token.is_none() {
            let token = leomcp::new_token();
            if let Some(path) = &self.config_path {
                let change = ("mcp-token".to_string(), Some(format!("\"{token}\"")));
                if let Err(e) = crate::config::update(path, &[change]) {
                    self.message = format!("MCP token not saved: {e}");
                }
            }
            settings.mcp.token = Some(token);
        }
        if let Err(e) = self.set_mcp(&settings.mcp) {
            self.message = e;
        }
    }
}

/// One line per file a read reported, for the message log.
pub fn read_report_lines(report: &leolib::external::ReadResult) -> Vec<String> {
    let errors = report
        .errors
        .iter()
        .map(|e| format!("not read: {}: {}", e.headline, e.error));
    let warnings = report
        .warnings
        .iter()
        .map(|w| format!("{}: {}", w.headline, w.message));
    errors.chain(warnings).collect()
}

/// How many messages `:messages` keeps, as vim's default.
const MESSAGE_LOG: usize = 200;

/// The theme a `:theme NAME` line names, if it names one.
fn theme_argument(line: &str) -> Option<String> {
    let parsed = minibuffer::parse_command(line)?;
    match parsed.name == "theme" && !parsed.arg.is_empty() {
        true => Some(parsed.arg),
        false => None,
    }
}

/// `App::row_positions`' cache: the rows, the outline state they were
/// walked for, and where the selection was found in them.
#[derive(Default)]
struct RowCache {
    key: Option<(u64, u64, Option<Position>)>,
    rows: Rc<Vec<Position>>,
    current: Option<(Position, usize)>,
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
    /// Whether the body holds any text.
    pub has_body: bool,
    pub headline: String,
    /// What needs attention in an `@<file>` node's file.
    pub file_state: Option<FileState>,
}
impl App {
    pub fn new(doc: Document) -> Self {
        let current = doc
            .outline()
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
            body_hscroll: 0,
            body_spots: Default::default(),
            help_scroll: 0,
            tree_percent: 35,
            expansion_level: 1,
            expansion_node: None,
            message: String::new(),
            messages: Vec::new(),
            overlay: None,
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
            pending_read: (Vec::new(), false),
            save_files: true,
            lsp: None,
            lsp_synced: None,
            lsp_dirty_since: None,
            diagnostics: Vec::new(),
            row_cache: Default::default(),
            theme_setting: "theme",
            mcp: None,
            mcp_access: Access::default(),
            settings: crate::config::Config::default(),
            code_actions: Vec::new(),
            code_actions_at: 0,
            code_action_selected: 0,
            unread: Default::default(),
            changed_on_disk: Vec::new(),
        };
        app.unread = app
            .doc
            .read_report
            .errors
            .iter()
            .map(|e| (e.path.clone(), e.error.to_string()))
            .collect();
        app.expand_ancestors();
        app.history.update(&app.current);
        app
    }

    pub fn outline(&self) -> &Outline {
        self.doc.outline()
    }

    // --- Dispatch --------------------------------------------------------

    /// Feed one key to the current mode.
    pub fn handle_key(&mut self, event: KeyEvent) {
        self.message.clear();
        let Some(mut event) = self.leo_chord(event) else {
            self.log_message();
            return;
        };
        // Leo's keyboard-quit is Escape for whatever is being typed.
        let ctrl = |e: &KeyEvent, c: char| {
            e.code == KeyCode::Char(c) && e.modifiers == KeyModifiers::CONTROL
        };
        if ctrl(&event, 'g') && self.mode != Mode::Normal {
            event = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        }
        if ctrl(&event, 'c') {
            self.interrupt();
        } else {
            match self.mode {
                Mode::Headline | Mode::Confirm | Mode::Command | Mode::Search => {
                    self.mini_key(event)
                }
                Mode::Insert => self.insert_key(event),
                Mode::Visual => self.body_key(event),
                Mode::Normal if self.focus == Focus::Body => self.body_key(event),
                Mode::Help => {
                    if !self.code_action_key(&event) {
                        self.command_key(event)
                    }
                }
                Mode::Normal => self.command_key(event),
            }
        }
        self.log_message();
    }

    /// Leo's outline chords where a mode would otherwise eat them.
    ///
    /// A Cmd chord, which leoegui sends on macOS as SUPER, is Leo's Ctrl:
    /// on a Mac Leo's Ctrl is the Cmd key. A chord the outline binds runs its
    /// outline command from either pane, after INSERT commits; Cmd-R moves
    /// the node right even in the body, where Control-r is vim's redo. Any
    /// other Cmd chord goes on as Ctrl.
    ///
    /// While a headline is typed, a Ctrl, Alt or Cmd chord the outline binds
    /// keeps the headline as typed and then runs, as in Leo's headline
    /// editor: a new node is indented with Ctrl-R before it has a name, and
    /// Ctrl-I after a name starts the next. Ctrl-g stays keyboard-quit.
    ///
    /// None if the chord was run.
    fn leo_chord(&mut self, event: KeyEvent) -> Option<KeyEvent> {
        let cmd = event.modifiers.contains(KeyModifiers::SUPER);
        let event = match cmd {
            true => {
                let mut mods = event.modifiers;
                mods.remove(KeyModifiers::SUPER);
                KeyEvent::new(event.code, mods | KeyModifiers::CONTROL)
            }
            false => event,
        };
        let chord = event.modifiers.contains(KeyModifiers::CONTROL)
            || event.modifiers.contains(KeyModifiers::ALT);
        let quit = event.code == KeyCode::Char('g') && event.modifiers == KeyModifiers::CONTROL;
        let headline = self.mode == Mode::Headline && chord && !quit;
        if !cmd && !headline {
            return Some(event);
        }
        let key = Key::from_event(event);
        let command = bindings::for_context(Mode::Normal, Focus::Tree)
            .find(|b| keys::parse(b.keys) == [key])
            .map(|b| b.command);
        match (command, self.mode) {
            (Some(command), Mode::Headline) if !quit => {
                self.finish_mini(true);
                self.run(command, 1);
                None
            }
            (Some(command), Mode::Normal | Mode::Insert | Mode::Visual) if cmd => {
                if self.mode != Mode::Normal {
                    self.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
                }
                self.run(command, 1);
                None
            }
            _ => Some(event),
        }
    }

    /// Keep the status line's message for `:messages`, since the next key
    /// clears it. Run once per event.
    pub fn log_message(&mut self) {
        if !self.message.is_empty() {
            self.log(self.message.clone());
        }
    }

    pub fn log(&mut self, line: String) {
        self.messages.push(line);
        let over = self.messages.len().saturating_sub(MESSAGE_LOG);
        self.messages.drain(..over);
    }

    /// vim's `:messages`: the message log, in the help overlay.
    pub fn show_messages(&mut self) {
        let lines = match self.messages.is_empty() {
            true => vec!["no messages".to_string()],
            false => self.messages.clone(),
        };
        self.overlay = Some(("messages".to_string(), lines));
        self.help_scroll = self.messages.len().saturating_sub(1);
        self.mode = Mode::Help;
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
    /// Every row is built; `rows_in` builds only the ones drawn.
    pub fn rows(&self) -> Vec<Row> {
        self.rows_in(0..self.row_count())
    }

    /// Rows `range` of `rows()`, clamped to what there is.
    pub fn rows_in(&self, range: std::ops::Range<usize>) -> Vec<Row> {
        let positions = self.row_positions();
        let base = self.hoist_limit().map_or(0, |h| h.level());
        let end = range.end.min(positions.len());
        positions[range.start.min(end)..end]
            .iter()
            .map(|p| self.row(p, base))
            .collect()
    }

    fn row(&self, p: &Position, base: usize) -> Row {
        let o = self.outline();
        Row {
            depth: p.level() - base,
            has_children: p.has_children(o),
            expanded: o.is_expanded(p),
            marked: p.is_marked(o),
            dirty: p.is_dirty(o),
            cloned: p.is_cloned(o),
            is_file: p.is_any_at_file_node(o),
            has_body: !p.b(o).is_empty(),
            headline: p.h(o).to_string(),
            file_state: self.file_state(p),
            position: p.clone(),
        }
    }

    /// How many rows the outline pane has.
    pub fn row_count(&self) -> usize {
        self.row_positions().len()
    }

    /// The position at row `i`, if there is one.
    pub fn row_position(&self, i: usize) -> Option<Position> {
        self.row_positions().get(i).cloned()
    }

    /// The visible rows' positions, walked again only when the tree's shape,
    /// its folds or the hoist changed. A frame asks several times, and the
    /// walk is O(outline): 2.5 ms at 21,000 rows.
    fn row_positions(&self) -> Rc<Vec<Position>> {
        let o = self.outline();
        let key = (o.generation, o.expansion, self.hoist_limit().cloned());
        let mut cache = self.row_cache.borrow_mut();
        if cache.key.as_ref() == Some(&key) {
            return cache.rows.clone();
        }
        let mut rows = Vec::new();
        let mut p = self.hoist_limit().cloned().or_else(|| o.root_position());
        while let Some(cur) = p.filter(|p| self.in_view(p)) {
            p = if cur.has_children(o) && o.is_expanded(&cur) {
                cur.thread_next(o)
            } else {
                cur.node_after_tree(o)
            };
            rows.push(cur);
        }
        *cache = RowCache {
            key: Some(key),
            rows: Rc::new(rows),
            current: None,
        };
        cache.rows.clone()
    }

    /// The visible rows and the index of the selected one.
    pub fn rows_and_current(&self) -> (Vec<Row>, usize) {
        (self.rows(), self.current_row())
    }

    /// The selected row's index, or 0 if it is not shown.
    pub fn current_row(&self) -> usize {
        let positions = self.row_positions();
        let mut cache = self.row_cache.borrow_mut();
        if let Some((p, i)) = &cache.current {
            if *p == self.current {
                return *i;
            }
        }
        let i = positions
            .iter()
            .position(|p| *p == self.current)
            .unwrap_or(0);
        cache.current = Some((self.current.clone(), i));
        i
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
        // Each node keeps its cursor and scroll, as Leo's `v.insertSpot`.
        let here = (self.editor.cursor, self.body_scroll);
        self.body_spots.insert(self.current.v, here);
        let (cursor, scroll) = self.body_spots.get(&p.v).copied().unwrap_or_default();
        self.current = p;
        // The body is a different buffer now.
        self.buffer = None;
        self.editor.cursor = cursor;
        self.editor.visual = None;
        let lines = self.body_buffer();
        self.editor.clamp(&lines);
        self.editor.desired_col = self.editor.cursor.1;
        self.body_scroll = scroll.min(lines.len().saturating_sub(1));
        self.body_hscroll = 0;
        self.expand_ancestors();
    }

    /// Select node `gnx` with the body cursor at `cursor`, clamped to the
    /// body, as a restored session left it. False if no node has the gnx.
    pub fn restore_selection(&mut self, gnx: &str, cursor: (usize, usize)) -> bool {
        let o = self.outline();
        let Some(p) = o
            .all_unique_positions()
            .into_iter()
            .find(|p| p.gnx(o) == gnx)
        else {
            return false;
        };
        self.select(p);
        self.editor.cursor = cursor;
        let lines = self.body_buffer();
        self.editor.clamp(&lines);
        self.editor.desired_col = self.editor.cursor.1;
        self.scroll_to_cursor();
        true
    }

    /// Leo's `go-back` (`step` -1) and `go-forward` (+1), `count` times.
    pub fn go_history(&mut self, step: isize, count: usize) {
        for _ in 0..count {
            let Some(p) = self.history.step(self.doc.outline(), step) else {
                self.message = "no more history".to_string();
                return;
            };
            self.select(p);
        }
    }

    /// Unfold everything above the current node, so it can be seen.
    pub fn expand_ancestors(&mut self) {
        let p = self.current.clone();
        self.doc.outline_mut_untracked().expand_all_ancestors(&p);
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
        let n = self.row_count();
        if let Some(p) = self.row_position(index.min(n.saturating_sub(1))) {
            self.select(p);
        }
    }

    /// Move `delta` visible rows in the outline.
    pub fn move_rows(&mut self, delta: i32) {
        let n = self.row_count();
        if n == 0 {
            return;
        }
        let i = self.current_row() as i32 + delta;
        let i = i.clamp(0, n as i32 - 1) as usize;
        if let Some(p) = self.row_position(i) {
            self.select(p);
        }
    }

    // --- Folding ---------------------------------------------------------

    /// Leo's left arrow: fold this node, or step out to the parent.
    pub fn contract_or_go_left(&mut self) {
        let p = self.current.clone();
        if p.has_children(self.outline()) && self.outline().is_expanded(&p) {
            self.doc.outline_mut_untracked().contract(&p);
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
            self.doc.outline_mut_untracked().expand(&p);
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
        let max = self
            .doc
            .outline_mut_untracked()
            .expand_to_level(&p, level.max(1));
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
        let (rows, current) = (self.row_count(), self.current_row());
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
            rows
        )
    }
}

/// Keep the newest entry once, at the end.
fn remember(history: &mut Vec<String>, entry: &str) {
    history.retain(|e| e != entry);
    history.push(entry.to_string());
}
