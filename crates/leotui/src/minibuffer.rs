//! The `:` command line and the `/` search prompt.
//!
//! One line of text at the bottom of the screen, with completion over the
//! command table for `:` and an incremental match for `/`. The vocabulary is
//! Leo's command names; Leo binds `full-command = :` for the same reason.

use crate::commands::COMMANDS;

/// What the line at the bottom is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MiniKind {
    Command,
    SearchForward,
    SearchBackward,
    Headline,
    SaveAs,
    ConfirmQuit,
    /// The label names the files, so `App::mini_label` draws it.
    ConfirmOverwrite,
}

impl MiniKind {
    pub fn label(self) -> &'static str {
        match self {
            MiniKind::Command => ":",
            MiniKind::SearchForward => "/",
            MiniKind::SearchBackward => "?",
            MiniKind::Headline => "headline: ",
            MiniKind::SaveAs => "save as: ",
            MiniKind::ConfirmQuit => "unsaved changes. quit anyway? (y/n) ",
            MiniKind::ConfirmOverwrite => "overwrite files this outline has not read? (y/n) ",
        }
    }

    pub fn is_search(self) -> bool {
        matches!(self, MiniKind::SearchForward | MiniKind::SearchBackward)
    }
}

/// The editable line, its cursor, and where completion and history are up to.
pub struct Minibuffer {
    pub kind: MiniKind,
    pub buffer: String,
    pub cursor: usize,
    /// Set while cycling completions, so Tab advances instead of restarting.
    completion: Option<Completion>,
    /// Where history browsing is, counting back from the newest entry.
    history_index: Option<usize>,
}

struct Completion {
    /// Where in the line the candidate starts.
    at: usize,
    /// What the user typed before Tab first replaced it.
    stem: String,
    matches: Vec<String>,
    index: usize,
}

/// What a drop-down should show.
pub struct Menu {
    pub items: Vec<String>,
    /// The match Tab has landed on, absent until one is chosen.
    pub selected: Option<usize>,
    /// Where in the line the items replace. Zero means a command name.
    pub at: usize,
}

impl Menu {
    /// True when there is a drop-down on screen.
    ///
    /// A command name completes in place, so only an argument opens one, and
    /// the arrow keys move through exactly what the eye can see.
    pub fn is_open(&self) -> bool {
        self.at > 0 && !self.items.is_empty()
    }
}

impl Minibuffer {
    pub fn new(kind: MiniKind, buffer: String) -> Self {
        Self {
            kind,
            cursor: buffer.chars().count(),
            buffer,
            completion: None,
            history_index: None,
        }
    }

    pub fn insert(&mut self, ch: char) {
        let i = char_index(&self.buffer, self.cursor);
        self.buffer.insert(i, ch);
        self.cursor += 1;
        self.completion = None;
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let i = char_index(&self.buffer, self.cursor - 1);
            self.buffer.remove(i);
            self.cursor -= 1;
        }
        self.completion = None;
    }

    pub fn delete(&mut self) {
        if self.cursor < self.buffer.chars().count() {
            let i = char_index(&self.buffer, self.cursor);
            self.buffer.remove(i);
        }
        self.completion = None;
    }

    pub fn move_cursor(&mut self, delta: i32) {
        let n = self.buffer.chars().count() as i32;
        self.cursor = (self.cursor as i32 + delta).clamp(0, n) as usize;
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.buffer.chars().count();
    }

    /// Complete what is at the end of the line, as vim does: the longest
    /// common prefix first, then each match in turn.
    pub fn complete(&mut self, backwards: bool, themes: &[String]) {
        if self.kind != MiniKind::Command {
            return;
        }
        if self.completion.is_some() {
            self.step(backwards);
            return;
        }
        if !self.begin(themes) {
            return;
        }
        let Some(c) = self.completion.as_ref() else {
            return;
        };
        let prefix = common_prefix(&c.matches);
        let (at, typed) = (c.at, c.stem.len());
        // The common prefix first: it is what the user meant more often than
        // the first match alphabetically.
        if prefix.len() > typed {
            self.buffer.truncate(at);
            self.buffer.push_str(&prefix);
            self.cursor = self.buffer.chars().count();
            return;
        }
        self.step(backwards);
    }

    /// Move the drop-down's selection, opening it if it is not open.
    ///
    /// Unlike Tab this never stops at the common prefix. An arrow key is
    /// aimed at a name in the list, not at the longest thing safe to type.
    pub fn select(&mut self, backwards: bool, themes: &[String]) {
        if self.kind != MiniKind::Command {
            return;
        }
        if self.completion.is_none() && !self.begin(themes) {
            return;
        }
        self.step(backwards);
    }

    /// Start a completion from what is typed. False when nothing matches.
    fn begin(&mut self, themes: &[String]) -> bool {
        let Candidates { at, items } = candidates(&self.buffer, themes);
        if items.is_empty() {
            return false;
        }
        self.completion = Some(Completion {
            at,
            stem: self.buffer[at..].to_string(),
            matches: items,
            index: usize::MAX,
        });
        true
    }

    /// Move to the next match, or to an end when none is chosen yet.
    fn step(&mut self, backwards: bool) {
        let Some(c) = self.completion.as_ref() else {
            return;
        };
        if c.matches.is_empty() {
            return;
        }
        let n = c.matches.len();
        // `usize::MAX` is "nothing chosen yet", which the common prefix
        // leaves behind. Stepping off it lands on an end, not on a wrap.
        let index = match (c.index == usize::MAX, backwards) {
            (true, false) => 0,
            (true, true) => n - 1,
            (false, false) => (c.index + 1) % n,
            (false, true) => (c.index + n - 1) % n,
        };
        self.choose(index);
    }

    /// Put match `i` in the line, keeping whatever comes before it.
    fn choose(&mut self, i: usize) {
        let Some(c) = self.completion.as_mut() else {
            return;
        };
        c.index = i;
        let (at, pick) = (c.at, c.matches[i].clone());
        self.buffer.truncate(at);
        self.buffer.push_str(&pick);
        self.cursor = self.buffer.chars().count();
    }

    /// The match Tab has landed on, if one has been chosen.
    pub fn selected(&self) -> Option<&str> {
        let c = self.completion.as_ref()?;
        c.matches.get(c.index).map(|s| s.as_str())
    }

    /// The candidates a drop-down should show, and which is selected.
    ///
    /// While cycling these come from the completion rather than from the
    /// line: the line has been replaced by the selection, and recomputing
    /// from it would leave a list of one.
    pub fn menu(&self, themes: &[String]) -> Menu {
        match self.completion.as_ref() {
            Some(c) => Menu {
                items: c.matches.clone(),
                selected: (c.index < c.matches.len()).then_some(c.index),
                at: c.at,
            },
            None => {
                let Candidates { at, items } = candidates(&self.buffer, themes);
                Menu {
                    items,
                    selected: None,
                    at,
                }
            }
        }
    }

    /// Abandon the completion, restoring what was typed.
    pub fn cancel_completion(&mut self) {
        if let Some(c) = self.completion.take() {
            self.buffer.truncate(c.at);
            self.buffer.push_str(&c.stem);
            self.cursor = self.buffer.chars().count();
        }
    }

    /// Walk the history. `back` is Up; forward is Down.
    pub fn history(&mut self, entries: &[String], back: bool) {
        if entries.is_empty() {
            return;
        }
        let i = match (self.history_index, back) {
            (None, true) => Some(entries.len() - 1),
            (None, false) => None,
            (Some(0), true) => Some(0),
            (Some(i), true) => Some(i - 1),
            (Some(i), false) if i + 1 < entries.len() => Some(i + 1),
            (Some(_), false) => None,
        };
        self.history_index = i;
        self.buffer = match i {
            Some(i) => entries[i].clone(),
            None => String::new(),
        };
        self.cursor = self.buffer.chars().count();
        self.completion = None;
    }
}

/// What a `:` line is completing: a command name, or one command's argument.
///
/// The offset is where in the line a candidate replaces, so `theme one` can
/// complete `one` without disturbing the command in front of it.
pub struct Candidates {
    pub at: usize,
    pub items: Vec<String>,
}

/// The commands whose argument is a theme name.
const TAKES_A_THEME: &str = "theme";

/// What would complete at the end of `line`.
///
/// `themes` is passed in rather than read here: the directories are the
/// caller's business, and a test needs a list it chose.
pub fn candidates(line: &str, themes: &[String]) -> Candidates {
    match line.split_once(char::is_whitespace) {
        // A command and its argument. Only `theme` has anything to offer.
        Some((head, rest)) => {
            let at = head.len() + 1;
            let stem = &line[at..];
            let resolved = ALIASES
                .iter()
                .find(|(a, _)| *a == head)
                .map(|(_, c)| *c)
                .unwrap_or(head);
            let items = match resolved == TAKES_A_THEME && !rest.contains(char::is_whitespace) {
                true => themes
                    .iter()
                    .filter(|n| n.starts_with(stem))
                    .cloned()
                    .collect(),
                false => Vec::new(),
            };
            Candidates { at, items }
        }
        None => Candidates {
            at: 0,
            items: completions(line),
        },
    }
}

/// The command names that start with `stem`, in table order.
///
/// Aliases are included so `:w` completes, but a name always beats an alias.
pub fn completions(stem: &str) -> Vec<String> {
    let mut names: Vec<String> = COMMANDS
        .iter()
        .map(|c| c.name.to_string())
        .filter(|n| n.starts_with(stem))
        .collect();
    names.sort();
    for (alias, _) in ALIASES {
        if alias.starts_with(stem) && !names.iter().any(|n| n == alias) {
            names.insert(0, alias.to_string());
        }
    }
    names
}

fn common_prefix(items: &[String]) -> String {
    let Some(first) = items.first() else {
        return String::new();
    };
    let mut n = first.len();
    for item in &items[1..] {
        n = n.min(
            first
                .chars()
                .zip(item.chars())
                .take_while(|(a, b)| a == b)
                .map(|(a, _)| a.len_utf8())
                .sum(),
        );
    }
    first[..n].to_string()
}

/// vim spellings a Leo user will type anyway, and what they mean here.
pub static ALIASES: &[(&str, &str)] = &[
    ("w", "save"),
    ("write", "save"),
    ("q", "quit"),
    ("wq", "save-and-quit"),
    ("x", "save-and-quit"),
    ("xit", "save-and-quit"),
    ("h", "help"),
    ("e", "open"),
    ("edit", "open"),
];

/// One parsed command line.
pub struct ParsedCommand {
    pub name: String,
    pub arg: String,
    /// `:12` means "go to the twelfth visible node".
    pub row: Option<usize>,
    pub force: bool,
}

/// Split a `:` line into a command, its argument and vim's `!` suffix.
pub fn parse_command(line: &str) -> Option<ParsedCommand> {
    // The `:` is the prompt's label, not part of the line, but a pasted or
    // scripted line will carry it.
    let line = line.trim().trim_start_matches(':').trim();
    if line.is_empty() {
        return None;
    }
    if let Ok(row) = line.parse::<usize>() {
        return Some(ParsedCommand {
            name: "goto-visible-row".to_string(),
            arg: String::new(),
            row: Some(row),
            force: false,
        });
    }
    let (head, arg) = match line.split_once(char::is_whitespace) {
        Some((h, a)) => (h, a.trim().to_string()),
        None => (line, String::new()),
    };
    let force = head.ends_with('!');
    let name = head.trim_end_matches('!').to_string();
    // An alias resolves to a command name; `!` is kept for the caller.
    let name = ALIASES
        .iter()
        .find(|(a, _)| *a == name)
        .map(|(_, c)| c.to_string())
        .unwrap_or(name);
    Some(ParsedCommand {
        name,
        arg,
        row: None,
        force,
    })
}

fn char_index(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map(|(i, _)| i).unwrap_or(s.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vim_spellings_resolve_to_leo_names() {
        assert_eq!(parse_command(":w").unwrap().name, "save");
        assert_eq!(parse_command("w").unwrap().name, "save");
        assert_eq!(parse_command("wq").unwrap().name, "save-and-quit");
        assert_eq!(parse_command("x").unwrap().name, "save-and-quit");
        let q = parse_command("q!").unwrap();
        assert_eq!(q.name, "quit");
        assert!(q.force);
    }

    #[test]
    fn a_leo_command_name_passes_through() {
        let p = parse_command("move-outline-down").unwrap();
        assert_eq!(p.name, "move-outline-down");
        assert!(p.arg.is_empty());
    }

    #[test]
    fn an_argument_is_kept() {
        let p = parse_command("w /tmp/x.leo").unwrap();
        assert_eq!(p.name, "save");
        assert_eq!(p.arg, "/tmp/x.leo");
    }

    #[test]
    fn a_number_is_a_row() {
        assert_eq!(parse_command("12").unwrap().row, Some(12));
    }

    #[test]
    fn completion_offers_the_common_prefix_before_cycling() {
        let mut m = Minibuffer::new(MiniKind::Command, "move-outline-".to_string());
        m.complete(false, &[]);
        // All four move commands share the stem, so the stem stays.
        assert!(m.buffer.starts_with("move-outline-"), "{}", m.buffer);
        let first = m.buffer.clone();
        m.complete(false, &[]);
        assert_ne!(m.buffer, first);
        assert!(crate::commands::find(&m.buffer).is_some(), "{}", m.buffer);
    }

    #[test]
    fn completion_can_be_abandoned() {
        let mut m = Minibuffer::new(MiniKind::Command, "goto-p".to_string());
        m.complete(false, &[]);
        assert_ne!(m.buffer, "goto-p");
        m.cancel_completion();
        assert_eq!(m.buffer, "goto-p");
    }

    #[test]
    fn history_walks_back_and_forward() {
        let entries = vec!["save".to_string(), "undo".to_string()];
        let mut m = Minibuffer::new(MiniKind::Command, String::new());
        m.history(&entries, true);
        assert_eq!(m.buffer, "undo");
        m.history(&entries, true);
        assert_eq!(m.buffer, "save");
        m.history(&entries, false);
        assert_eq!(m.buffer, "undo");
        m.history(&entries, false);
        assert_eq!(m.buffer, "");
    }

    fn themes() -> Vec<String> {
        ["onedark", "onedarker", "onelight", "sonokai"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn only_theme_completes_its_argument() {
        let names = themes();
        let c = candidates("theme one", &names);
        assert_eq!(c.at, "theme ".len());
        assert_eq!(c.items, ["onedark", "onedarker", "onelight"]);
        // Every other command's argument is a path or a name nothing knows.
        assert!(candidates("save one", &names).items.is_empty());
        assert!(candidates("open one", &names).items.is_empty());
        // A second word is a path, not a theme.
        assert!(candidates("theme one two", &names).items.is_empty());
    }

    #[test]
    fn completing_an_argument_leaves_the_command_alone() {
        let mut m = Minibuffer::new(MiniKind::Command, "theme onel".to_string());
        m.complete(false, &themes());
        assert_eq!(m.buffer, "theme onelight");
        assert_eq!(m.cursor, m.buffer.chars().count());
    }

    #[test]
    fn the_menu_keeps_the_whole_list_while_tab_cycles_it() {
        // The line becomes the selection, so recomputing the menu from the
        // line would leave a list of one.
        let names = themes();
        let mut m = Minibuffer::new(MiniKind::Command, "theme o".to_string());
        assert_eq!(m.menu(&names).items.len(), 3);
        assert_eq!(m.menu(&names).selected, None);
        m.complete(false, &names); // the common prefix, `one`
        assert_eq!(m.buffer, "theme one");
        assert_eq!(m.menu(&names).selected, None, "the prefix is not a match");
        m.complete(false, &names); // the first match
        let menu = m.menu(&names);
        assert_eq!(menu.items.len(), 3, "the list collapsed: {:?}", menu.items);
        assert_eq!(menu.selected, Some(0));
        assert_eq!(m.selected(), Some("onedark"));
        assert_eq!(m.buffer, "theme onedark");
    }

    #[test]
    fn abandoning_an_argument_completion_restores_what_was_typed() {
        let names = themes();
        let mut m = Minibuffer::new(MiniKind::Command, "theme onel".to_string());
        m.complete(false, &names);
        assert_ne!(m.buffer, "theme onel");
        m.cancel_completion();
        assert_eq!(m.buffer, "theme onel");
    }

    #[test]
    fn a_command_name_still_completes_from_the_start_of_the_line() {
        let mut m = Minibuffer::new(MiniKind::Command, "the".to_string());
        m.complete(false, &themes());
        assert_eq!(m.buffer, "theme");
        assert_eq!(m.menu(&themes()).at, 0, "a command name is not an argument");
    }

    #[test]
    fn an_arrow_picks_a_name_where_tab_would_take_the_prefix() {
        let names = themes();
        let mut m = Minibuffer::new(MiniKind::Command, "theme o".to_string());
        m.select(false, &names);
        assert_eq!(m.buffer, "theme onedark", "Down stopped at the prefix");
        assert_eq!(m.menu(&names).selected, Some(0));
        // Up from the first wraps to the last.
        m.select(true, &names);
        assert_eq!(m.selected(), Some("onelight"));
        m.select(true, &names);
        assert_eq!(m.selected(), Some("onedarker"));
    }

    #[test]
    fn up_from_nothing_chosen_lands_on_the_last_name() {
        let names = themes();
        let mut m = Minibuffer::new(MiniKind::Command, "theme o".to_string());
        m.select(true, &names);
        assert_eq!(m.selected(), Some("onelight"));
    }

    #[test]
    fn the_menu_is_open_only_for_an_argument() {
        let names = themes();
        let m = Minibuffer::new(MiniKind::Command, "theme o".to_string());
        assert!(m.menu(&names).is_open());
        let m = Minibuffer::new(MiniKind::Command, "goto".to_string());
        assert!(!m.menu(&names).is_open());
        let m = Minibuffer::new(MiniKind::Command, "theme zzz".to_string());
        assert!(
            !m.menu(&names).is_open(),
            "nothing matches, nothing to show"
        );
    }
}
