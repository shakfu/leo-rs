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
    /// What the user typed before Tab first replaced it.
    stem: String,
    matches: Vec<String>,
    index: usize,
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

    /// Complete the command name, as vim does: the longest common prefix
    /// first, then each match in turn.
    pub fn complete(&mut self, backwards: bool) {
        if self.kind != MiniKind::Command {
            return;
        }
        if let Some(c) = self.completion.as_mut() {
            if c.matches.is_empty() {
                return;
            }
            let n = c.matches.len();
            c.index = if backwards {
                (c.index + n - 1) % n
            } else {
                (c.index + 1) % n
            };
            self.buffer = c.matches[c.index].clone();
            self.cursor = self.buffer.chars().count();
            return;
        }
        let stem = self.buffer.clone();
        let matches = completions(&stem);
        if matches.is_empty() {
            return;
        }
        let prefix = common_prefix(&matches);
        // The common prefix first: it is what the user meant more often than
        // the first match alphabetically.
        if prefix.len() > stem.len() {
            self.buffer = prefix;
            self.cursor = self.buffer.chars().count();
            self.completion = Some(Completion {
                stem,
                matches,
                index: usize::MAX,
            });
            // Next Tab starts the cycle at 0.
            if let Some(c) = self.completion.as_mut() {
                c.index = c.matches.len() - 1;
            }
            return;
        }
        self.buffer = matches[0].clone();
        self.cursor = self.buffer.chars().count();
        self.completion = Some(Completion {
            stem,
            matches,
            index: 0,
        });
    }

    /// Abandon the completion, restoring what was typed.
    pub fn cancel_completion(&mut self) {
        if let Some(c) = self.completion.take() {
            self.buffer = c.stem;
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
        m.complete(false);
        // All four move commands share the stem, so the stem stays.
        assert!(m.buffer.starts_with("move-outline-"), "{}", m.buffer);
        let first = m.buffer.clone();
        m.complete(false);
        assert_ne!(m.buffer, first);
        assert!(crate::commands::find(&m.buffer).is_some(), "{}", m.buffer);
    }

    #[test]
    fn completion_can_be_abandoned() {
        let mut m = Minibuffer::new(MiniKind::Command, "goto-p".to_string());
        m.complete(false);
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
}
