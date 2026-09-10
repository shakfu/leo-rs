//! The binding table.
//!
//! One table, read by the dispatcher, by the help overlay and by `--keys`. A
//! binding names a command; a command with no binding is still reachable once
//! the `:` minibuffer exists. See `docs/dev/tui-design.md` section 7.

use crate::app::{Focus, Mode};

/// One entry: which mode and pane it applies to, the keys, and the command.
pub struct Binding {
    pub mode: Mode,
    /// None: the binding applies whichever pane has focus.
    pub focus: Option<Focus>,
    pub keys: &'static str,
    pub command: &'static str,
}

const fn b(mode: Mode, focus: Option<Focus>, keys: &'static str, command: &'static str) -> Binding {
    Binding {
        mode,
        focus,
        keys,
        command,
    }
}

const TREE: Option<Focus> = Some(Focus::Tree);
const BODY: Option<Focus> = Some(Focus::Body);
const BOTH: Option<Focus> = None;

/// Section 7.1 of the design, plus the pane and file bindings.
///
/// Where Leo binds the same command, its binding is listed too: the measured
/// key table in the design's section 2 says which of Leo's survive a terminal.
pub static BINDINGS: &[Binding] = &[
    // --- Tree: navigation. Leo's own arrow bindings, verbatim. ------------
    b(Mode::Normal, TREE, "j", "goto-next-visible"),
    b(Mode::Normal, TREE, "Down", "goto-next-visible"),
    b(Mode::Normal, TREE, "k", "goto-prev-visible"),
    b(Mode::Normal, TREE, "Up", "goto-prev-visible"),
    b(Mode::Normal, TREE, "h", "contract-or-go-left"),
    b(Mode::Normal, TREE, "Left", "contract-or-go-left"),
    b(Mode::Normal, TREE, "l", "expand-and-go-right"),
    b(Mode::Normal, TREE, "Right", "expand-and-go-right"),
    b(Mode::Normal, TREE, "Enter", "expand-and-go-right"),
    b(Mode::Normal, TREE, "gg", "goto-first-visible-node"),
    b(Mode::Normal, TREE, "Alt-Home", "goto-first-visible-node"),
    b(Mode::Normal, TREE, "G", "goto-last-visible-node"),
    b(Mode::Normal, TREE, "Alt-End", "goto-last-visible-node"),
    b(Mode::Normal, TREE, "gp", "goto-parent"),
    b(Mode::Normal, TREE, "{", "goto-prev-sibling"),
    b(Mode::Normal, TREE, "}", "goto-next-sibling"),
    b(Mode::Normal, TREE, "[m", "goto-prev-marked"),
    b(Mode::Normal, TREE, "]m", "goto-next-marked"),
    b(Mode::Normal, TREE, "]c", "goto-next-clone"),
    b(Mode::Normal, TREE, "Alt-n", "goto-next-clone"),
    // --- Tree: structure --------------------------------------------------
    b(Mode::Normal, TREE, "o", "insert-node"),
    b(Mode::Normal, TREE, "Insert", "insert-node"),
    b(Mode::Normal, TREE, "O", "insert-node-before"),
    b(Mode::Normal, TREE, "a", "insert-child"),
    b(Mode::Normal, TREE, "Ctrl-Insert", "insert-child"),
    // vim's dd cuts into the register; Leo's Delete does not copy. Both are
    // useful, so both are bound.
    b(Mode::Normal, TREE, "dd", "cut-node"),
    b(Mode::Normal, TREE, "Delete", "delete-node"),
    b(Mode::Normal, TREE, "Backspace", "delete-node"),
    b(Mode::Normal, TREE, "yy", "copy-node"),
    b(Mode::Normal, TREE, "p", "paste-node"),
    b(Mode::Normal, TREE, "`", "clone-node"),
    b(Mode::Normal, TREE, "m", "mark"),
    b(Mode::Normal, TREE, "M", "unmark-all"),
    b(Mode::Normal, TREE, "J", "move-outline-down"),
    b(Mode::Normal, TREE, "Shift-Down", "move-outline-down"),
    b(Mode::Normal, TREE, "K", "move-outline-up"),
    b(Mode::Normal, TREE, "Shift-Up", "move-outline-up"),
    b(Mode::Normal, TREE, "<<", "move-outline-left"),
    b(Mode::Normal, TREE, "Shift-Left", "move-outline-left"),
    b(Mode::Normal, TREE, ">>", "move-outline-right"),
    b(Mode::Normal, TREE, "Shift-Right", "move-outline-right"),
    b(Mode::Normal, TREE, "g<", "promote"),
    b(Mode::Normal, TREE, "g>", "demote"),
    b(Mode::Normal, TREE, "e", "edit-headline"),
    // Leo's literal bindings, which only a terminal speaking the keyboard
    // enhancement protocol can deliver: without it Ctrl-I is Tab, Ctrl-M is
    // Enter, Ctrl-[ is Escape and Ctrl-` is not a control code at all. Each
    // has a portable binding above; these cost one table entry.
    b(Mode::Normal, TREE, "Ctrl-i", "insert-node"),
    b(Mode::Normal, TREE, "Ctrl-m", "mark"),
    b(Mode::Normal, TREE, "Ctrl-[", "promote"),
    b(Mode::Normal, TREE, "Ctrl-]", "demote"),
    b(Mode::Normal, TREE, "Ctrl-`", "clone-node"),
    // `i` from the tree goes straight into the body, as the old TUI did.
    b(Mode::Normal, TREE, "i", "edit-body"),
    b(Mode::Normal, TREE, "Ctrl-h", "edit-headline"),
    // --- Tree: folding, on vim's z family ---------------------------------
    b(Mode::Normal, TREE, "Space", "toggle-node"),
    b(Mode::Normal, TREE, "za", "toggle-node"),
    b(Mode::Normal, TREE, "zo", "expand-node"),
    b(Mode::Normal, TREE, "Alt-]", "expand-node"),
    b(Mode::Normal, TREE, "zc", "contract-node"),
    b(Mode::Normal, TREE, "Alt-[", "contract-node"),
    b(Mode::Normal, TREE, "zR", "expand-all"),
    b(Mode::Normal, TREE, "zM", "contract-all"),
    b(Mode::Normal, TREE, "Alt--", "contract-all"),
    b(Mode::Normal, TREE, "zr", "expand-next-level"),
    b(Mode::Normal, TREE, "zm", "expand-prev-level"),
    b(Mode::Normal, TREE, "zx", "contract-all-other-nodes"),
    b(Mode::Normal, TREE, "z1", "expand-to-level-1"),
    b(Mode::Normal, TREE, "z2", "expand-to-level-2"),
    b(Mode::Normal, TREE, "z3", "expand-to-level-3"),
    b(Mode::Normal, TREE, "z4", "expand-to-level-4"),
    b(Mode::Normal, TREE, "z5", "expand-to-level-5"),
    b(Mode::Normal, TREE, "z6", "expand-to-level-6"),
    b(Mode::Normal, TREE, "z7", "expand-to-level-7"),
    b(Mode::Normal, TREE, "z8", "expand-to-level-8"),
    b(Mode::Normal, TREE, "z9", "expand-to-level-9"),
    // --- Body -------------------------------------------------------------
    //
    // The body has its own grammar -- counts, operators, motions and text
    // objects -- which `editor::parse` handles rather than this table. These
    // entries exist so the help screen lists the body's keys; the dispatcher
    // never reaches them.
    b(Mode::Normal, BODY, "h j k l", "body-motions"),
    b(Mode::Normal, BODY, "w W b B e E ge", "body-word-motions"),
    b(Mode::Normal, BODY, "0 ^ $ gg G { } %", "body-line-motions"),
    b(Mode::Normal, BODY, "f F t T ; ,", "body-find-char"),
    b(Mode::Normal, BODY, "H M L", "body-screen-motions"),
    b(Mode::Normal, BODY, "d c y > < gu gU g~", "body-operators"),
    b(Mode::Normal, BODY, "iw aw i\" a( ip", "body-text-objects"),
    b(
        Mode::Normal,
        BODY,
        "x X r s S D C Y J ~",
        "body-simple-edits",
    ),
    b(Mode::Normal, BODY, "i a I A o O", "body-insert"),
    b(Mode::Normal, BODY, "v V", "body-visual"),
    b(Mode::Normal, BODY, "p P", "body-put"),
    b(Mode::Normal, BODY, ".", "body-repeat"),
    b(Mode::Normal, BODY, "Escape", "focus-to-tree"),
    // Leo's own rule: Tab leaves the pane it is pressed in. Shift-Tab cycles
    // the other way, which with two panes lands in the same place.
    b(Mode::Normal, BODY, "Tab", "focus-to-tree"),
    b(Mode::Normal, BODY, "Shift-Tab", "focus-to-tree"),
    b(Mode::Normal, TREE, "Tab", "focus-to-body"),
    b(Mode::Normal, TREE, "Shift-Tab", "focus-to-body"),
    // --- Both panes -------------------------------------------------------
    b(Mode::Normal, BOTH, "Ctrl-f", "page-down"),
    b(Mode::Normal, BOTH, "PageDown", "page-down"),
    b(Mode::Normal, BOTH, "Ctrl-b", "page-up"),
    b(Mode::Normal, BOTH, "PageUp", "page-up"),
    b(Mode::Normal, BOTH, "Ctrl-d", "half-page-down"),
    b(Mode::Normal, BOTH, "Ctrl-u", "half-page-up"),
    b(Mode::Normal, BOTH, "u", "undo"),
    b(Mode::Normal, BOTH, "Ctrl-z", "undo"),
    b(Mode::Normal, BOTH, "Ctrl-r", "redo"),
    b(Mode::Normal, BOTH, "Ctrl-Shift-z", "redo"),
    b(Mode::Normal, BOTH, "Ctrl-s", "save"),
    b(Mode::Normal, BOTH, "Ctrl-Left", "shrink-outline-pane"),
    b(Mode::Normal, BOTH, "Ctrl-Right", "grow-outline-pane"),
    // vim's window-width keys. Unlike Ctrl-arrows, which macOS takes for
    // Mission Control, every terminal passes Ctrl-w through.
    b(Mode::Normal, BOTH, "Ctrl-w <", "shrink-pane"),
    b(Mode::Normal, BOTH, "Ctrl-w >", "grow-pane"),
    b(Mode::Normal, BOTH, "F1", "help"),
    b(Mode::Normal, BOTH, ":", "full-command"),
    b(Mode::Normal, BOTH, "/", "search-forward"),
    b(Mode::Normal, BOTH, "?", "search-backward"),
    b(Mode::Normal, BOTH, "n", "find-next"),
    b(Mode::Normal, BOTH, "N", "find-prev"),
    // Tree only: `w` and `q` are a vim motion and the macro key, which the
    // body will want in stage 3. Escape reaches the tree from the body.
    b(Mode::Normal, TREE, "w", "write-at-file-nodes"),
    b(Mode::Normal, TREE, "q", "quit"),
    // --- The help overlay -------------------------------------------------
    b(Mode::Help, BOTH, "Escape", "close-help"),
    b(Mode::Help, BOTH, "q", "close-help"),
    b(Mode::Help, BOTH, "F1", "close-help"),
    b(Mode::Help, BOTH, "j", "scroll-help-down"),
    b(Mode::Help, BOTH, "Down", "scroll-help-down"),
    b(Mode::Help, BOTH, "k", "scroll-help-up"),
    b(Mode::Help, BOTH, "Up", "scroll-help-up"),
    b(Mode::Help, BOTH, "Ctrl-f", "scroll-help-page-down"),
    b(Mode::Help, BOTH, "PageDown", "scroll-help-page-down"),
    b(Mode::Help, BOTH, "Ctrl-b", "scroll-help-page-up"),
    b(Mode::Help, BOTH, "PageUp", "scroll-help-page-up"),
];

/// The bindings that apply in this mode and pane, most specific first.
pub fn for_context(mode: Mode, focus: Focus) -> impl Iterator<Item = &'static Binding> {
    BINDINGS
        .iter()
        .filter(move |x| x.mode == mode && (x.focus.is_none() || x.focus == Some(focus)))
}

/// Every key spec bound in NORMAL mode, once each.
///
/// `--key-specs` prints these, which is how `tests/readme.rs` checks the
/// README without a copy of the table: an integration test cannot import a
/// binary crate's modules, but it can run the binary.
pub fn normal_mode_specs() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = BINDINGS
        .iter()
        .filter(|b| b.mode == Mode::Normal)
        .map(|b| b.keys)
        .collect();
    out.sort();
    out.dedup();
    out
}

/// The keys bound to `command`, for the help screen.
pub fn keys_for(command: &str) -> Vec<&'static str> {
    BINDINGS
        .iter()
        .filter(|x| x.command == command)
        .map(|x| x.keys)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands;
    use crate::keys;
    use std::collections::HashMap;

    #[test]
    fn every_binding_parses_to_at_least_one_key() {
        for binding in BINDINGS {
            assert!(
                !keys::parse(binding.keys).is_empty(),
                "{:?} parses to nothing",
                binding.keys
            );
        }
    }

    #[test]
    fn every_binding_names_a_command_that_exists() {
        for binding in BINDINGS {
            assert!(
                commands::find(binding.command).is_some(),
                "no such command: {}",
                binding.command
            );
        }
    }

    /// Commands reached only through `:`, because they take an argument or
    /// are not worth a key.
    const COMMAND_LINE_ONLY: &[&str] = &[
        "goto-visible-row",
        "theme",
        "import-at-file",
        "substitute",
        "nohlsearch",
        "bufdo",
    ];

    #[test]
    fn every_command_is_reachable() {
        // A command with no key and no reason to be `:`-only is a command
        // nobody will find.
        for command in commands::COMMANDS {
            let bound = BINDINGS.iter().any(|b| b.command == command.name);
            assert!(
                bound || COMMAND_LINE_ONLY.contains(&command.name),
                "{} has no binding and is not listed as command-line only",
                command.name
            );
        }
    }

    #[test]
    fn command_names_are_unique() {
        let mut names: Vec<&str> = commands::COMMANDS.iter().map(|c| c.name).collect();
        names.sort();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "a command name is defined twice");
    }

    #[test]
    fn every_alias_resolves_to_something_runnable() {
        // Names `App::run_command_line` handles itself rather than through the
        // command table. Listed here so removing an arm fails this test.
        const HANDLED_BY_THE_COMMAND_LINE: &[&str] = &["save-and-quit", "open", "set"];
        for (alias, target) in crate::minibuffer::ALIASES {
            assert!(
                commands::find(target).is_some() || HANDLED_BY_THE_COMMAND_LINE.contains(target),
                "alias {alias} points at {target}, which nothing implements"
            );
        }
    }

    #[test]
    fn no_two_bindings_claim_the_same_keys_in_one_context() {
        let mut seen: HashMap<(usize, Focus, Vec<keys::Key>), &str> = HashMap::new();
        for focus in [Focus::Tree, Focus::Body] {
            for mode in [Mode::Normal, Mode::Help] {
                for binding in for_context(mode, focus) {
                    let k = (mode as usize, focus, keys::parse(binding.keys));
                    if let Some(other) = seen.insert(k, binding.command) {
                        assert_eq!(
                            other, binding.command,
                            "{:?} is bound to both {other} and {} in {mode:?}/{focus:?}",
                            binding.keys, binding.command
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_prefix_is_never_also_a_whole_binding() {
        // `g` must not act on its own, or `gg` could never be typed.
        for focus in [Focus::Tree, Focus::Body] {
            let all: Vec<Vec<keys::Key>> = for_context(Mode::Normal, focus)
                .map(|b| keys::parse(b.keys))
                .collect();
            for a in &all {
                for b in &all {
                    if a.len() < b.len() && b[..a.len()] == a[..] {
                        panic!("a binding is a prefix of another: {a:?} of {b:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn leos_literal_bindings_are_present_for_enhanced_terminals() {
        // These are unreachable on a legacy terminal -- Ctrl-I is Tab, Ctrl-M
        // is Enter, Ctrl-[ is Escape -- and reachable once the keyboard
        // enhancement flags are pushed. Each also has a portable binding, so
        // losing one is a regression only for Leo users' muscle memory.
        for (keys, command, portable) in [
            ("Ctrl-i", "insert-node", "o"),
            ("Ctrl-m", "mark", "m"),
            ("Ctrl-[", "promote", "g<"),
            ("Ctrl-]", "demote", "g>"),
            ("Ctrl-`", "clone-node", "`"),
            ("Ctrl-Shift-z", "redo", "Ctrl-r"),
        ] {
            assert!(
                BINDINGS
                    .iter()
                    .any(|b| b.keys == keys && b.command == command),
                "Leo binds {command} to {keys}, and this table does not"
            );
            assert!(
                BINDINGS
                    .iter()
                    .any(|b| b.keys == portable && b.command == command),
                "{command} has no binding a legacy terminal can deliver"
            );
        }
    }

    #[test]
    fn leos_own_arrow_bindings_are_present() {
        // Section 3 of the design: these are the ones a terminal delivers, so
        // they are the ones that must not drift.
        for (keys, command) in [
            ("Down", "goto-next-visible"),
            ("Up", "goto-prev-visible"),
            ("Left", "contract-or-go-left"),
            ("Right", "expand-and-go-right"),
            ("Shift-Down", "move-outline-down"),
            ("Shift-Up", "move-outline-up"),
            ("Shift-Left", "move-outline-left"),
            ("Shift-Right", "move-outline-right"),
            ("Insert", "insert-node"),
            ("Delete", "delete-node"),
            ("Backspace", "delete-node"),
            ("Ctrl-h", "edit-headline"),
            ("Alt-Home", "goto-first-visible-node"),
            ("Alt-End", "goto-last-visible-node"),
        ] {
            assert!(
                BINDINGS
                    .iter()
                    .any(|b| b.keys == keys && b.command == command),
                "Leo binds {command} to {keys}, and this table does not"
            );
        }
    }
}
