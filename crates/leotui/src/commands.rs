//! The command table.
//!
//! Every name is Leo's, where Leo has one. The table is the vocabulary the
//! binding table names, the help overlay lists, and the `:` minibuffer will
//! complete over.

use leolib::Position;

use crate::app::{App, Focus, Mode};
use crate::minibuffer::MiniKind;

pub struct Command {
    pub name: &'static str,
    pub summary: &'static str,
    pub run: fn(&mut App, usize),
}

const fn c(name: &'static str, summary: &'static str, run: fn(&mut App, usize)) -> Command {
    Command { name, summary, run }
}

/// A command that documents keys the editor handles itself.
fn noop(_app: &mut App, _count: usize) {}

pub fn find(name: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|x| x.name == name)
}

/// Repeat `f` `count` times.
fn repeat(app: &mut App, count: usize, f: impl Fn(&mut App)) {
    for _ in 0..count {
        f(app);
    }
}

/// Move the selection, if the motion has anywhere to go.
fn go(app: &mut App, count: usize, f: impl Fn(&App, &Position) -> Option<Position>) {
    for _ in 0..count {
        let Some(next) = f(app, &app.current) else {
            break;
        };
        app.select(next);
    }
}

pub static COMMANDS: &[Command] = &[
    // --- Navigation -------------------------------------------------------
    c(
        "goto-next-visible",
        "select the next visible node",
        |app, n| go(app, n, |app, p| p.vis_next(app.outline())),
    ),
    c(
        "goto-prev-visible",
        "select the previous visible node",
        |app, n| go(app, n, |app, p| p.vis_back(app.outline())),
    ),
    c("goto-parent", "select the parent", |app, n| {
        go(app, n, |app, p| p.parent(app.outline()))
    }),
    c("goto-next-sibling", "select the next sibling", |app, n| {
        go(app, n, |app, p| p.next(app.outline()))
    }),
    c(
        "goto-prev-sibling",
        "select the previous sibling",
        |app, n| go(app, n, |app, p| p.back(app.outline())),
    ),
    c(
        "goto-first-visible-node",
        "select the first node",
        |app, _| {
            if let Some(p) = app.outline().root_position() {
                app.select(p);
            }
        },
    ),
    c(
        "goto-last-visible-node",
        "select the last visible node",
        |app, _| {
            if let Some(p) = app.outline().last_visible_position() {
                app.select(p);
            }
        },
    ),
    c(
        "goto-next-marked",
        "select the next marked node",
        |app, n| go(app, n, |app, p| app.outline().next_marked(p)),
    ),
    c(
        "goto-prev-marked",
        "select the previous marked node",
        |app, n| go(app, n, |app, p| app.outline().prev_marked(p)),
    ),
    c(
        "goto-next-clone",
        "select the next clone of this node",
        |app, n| go(app, n, |app, p| app.outline().next_clone(p)),
    ),
    c(
        "contract-or-go-left",
        "fold this node, or select its parent",
        |app, n| repeat(app, n, App::contract_or_go_left),
    ),
    c(
        "expand-and-go-right",
        "unfold this node and select its first child",
        |app, n| repeat(app, n, App::expand_and_go_right),
    ),
    // --- Structure --------------------------------------------------------
    c("insert-node", "insert a node after this one", |app, _| {
        let p = app.current.clone();
        let new = app.doc.insert_node(&p);
        app.select(new);
        app.begin_headline_edit();
    }),
    c(
        "insert-node-before",
        "insert a node before this one",
        |app, _| {
            let p = app.current.clone();
            let new = app.doc.insert_node_before(&p);
            app.select(new);
            app.begin_headline_edit();
        },
    ),
    c(
        "insert-child",
        "insert a node as the first child",
        |app, _| {
            let p = app.current.clone();
            let new = app.doc.insert_child(&p);
            app.select(new);
            app.begin_headline_edit();
        },
    ),
    c("delete-node", "delete this node", |app, n| {
        repeat(app, n, |app| {
            let p = app.current.clone();
            match app.doc.delete_node(&p) {
                Some(next) => app.select(next),
                None => app.message = "cannot delete the last node".to_string(),
            }
        })
    }),
    c("cut-node", "copy this node, then delete it", |app, _| {
        let p = app.current.clone();
        app.doc.copy_node(&p);
        match app.doc.delete_node(&p) {
            Some(next) => app.select(next),
            None => app.message = "cannot delete the last node".to_string(),
        }
    }),
    c("copy-node", "copy this node to the clipboard", |app, _| {
        let p = app.current.clone();
        app.doc.copy_node(&p);
        app.message = format!("copied: {}", p.h(app.outline()));
    }),
    c(
        "paste-node",
        "paste the clipboard after this node",
        |app, n| {
            repeat(app, n, |app| {
                let p = app.current.clone();
                match app.doc.paste_node(&p) {
                    Some(new) => app.select(new),
                    None => app.message = "nothing copied".to_string(),
                }
            })
        },
    ),
    c("clone-node", "clone this node", |app, _| {
        let p = app.current.clone();
        let new = app.doc.clone_node(&p);
        app.select(new);
    }),
    c("mark", "mark or unmark this node", |app, _| {
        let p = app.current.clone();
        app.doc.toggle_marked(&p);
    }),
    c("unmark-all", "clear every mark in the outline", |app, _| {
        let n = app.doc.unmark_all();
        app.message = format!("unmarked {n}");
    }),
    c("move-outline-up", "move this node up", |app, n| {
        repeat(app, n, |app| {
            let p = app.current.clone();
            match app.doc.move_up(&p) {
                Some(new) => app.select(new),
                None => app.message = "cannot move up".to_string(),
            }
        })
    }),
    c("move-outline-down", "move this node down", |app, n| {
        repeat(app, n, |app| {
            let p = app.current.clone();
            match app.doc.move_down(&p) {
                Some(new) => app.select(new),
                None => app.message = "cannot move down".to_string(),
            }
        })
    }),
    c(
        "move-outline-left",
        "move this node out one level",
        |app, n| {
            repeat(app, n, |app| {
                let p = app.current.clone();
                match app.doc.move_left(&p) {
                    Some(new) => app.select(new),
                    None => app.message = "cannot move left".to_string(),
                }
            })
        },
    ),
    c(
        "move-outline-right",
        "make this node a child of the one above",
        |app, n| {
            repeat(app, n, |app| {
                let p = app.current.clone();
                match app.doc.move_right(&p) {
                    Some(new) => app.select(new),
                    None => app.message = "cannot move right".to_string(),
                }
            })
        },
    ),
    c(
        "promote",
        "make this node's children its siblings",
        |app, _| {
            let p = app.current.clone();
            app.doc.outline.promote(&p);
        },
    ),
    c(
        "demote",
        "make the following siblings this node's children",
        |app, _| {
            let p = app.current.clone();
            if !app.doc.demote(&p) {
                app.message = "no following siblings".to_string();
            }
        },
    ),
    c("edit-headline", "edit this node's headline", |app, _| {
        app.begin_headline_edit()
    }),
    // --- Folding ----------------------------------------------------------
    c("toggle-node", "fold or unfold this node", |app, _| {
        let p = app.current.clone();
        if p.has_children(app.outline()) {
            if app.outline().is_expanded(&p) {
                app.doc.outline.contract(&p);
            } else {
                app.doc.outline.expand(&p);
            }
        }
    }),
    c("expand-node", "unfold this node", |app, _| {
        let p = app.current.clone();
        app.doc.outline.expand(&p);
    }),
    c("contract-node", "fold this node", |app, _| {
        let p = app.current.clone();
        app.doc.outline.contract(&p);
    }),
    c("expand-all", "unfold every node", |app, _| {
        app.doc.outline.expand_all();
        app.expansion_level = 0;
    }),
    c("contract-all", "fold every node", |app, _| {
        app.doc.outline.contract_all();
        app.expansion_level = 1;
        if let Some(root) = app.outline().root_position() {
            app.current = root;
        }
    }),
    c("expand-next-level", "unfold one level further", |app, _| {
        app.expand_to_level(app.expansion_level + 1)
    }),
    c("expand-prev-level", "fold one level back", |app, _| {
        app.expand_to_level(app.expansion_level.saturating_sub(1).max(1))
    }),
    c(
        "contract-all-other-nodes",
        "fold everything except the path to this node",
        |app, _| {
            let p = app.current.clone();
            app.doc.outline.contract_all_other_nodes(&p);
        },
    ),
    c("expand-to-level-1", "unfold to level 1", |app, _| {
        app.expand_to_level(1)
    }),
    c("expand-to-level-2", "unfold to level 2", |app, _| {
        app.expand_to_level(2)
    }),
    c("expand-to-level-3", "unfold to level 3", |app, _| {
        app.expand_to_level(3)
    }),
    c("expand-to-level-4", "unfold to level 4", |app, _| {
        app.expand_to_level(4)
    }),
    c("expand-to-level-5", "unfold to level 5", |app, _| {
        app.expand_to_level(5)
    }),
    c("expand-to-level-6", "unfold to level 6", |app, _| {
        app.expand_to_level(6)
    }),
    c("expand-to-level-7", "unfold to level 7", |app, _| {
        app.expand_to_level(7)
    }),
    c("expand-to-level-8", "unfold to level 8", |app, _| {
        app.expand_to_level(8)
    }),
    c("expand-to-level-9", "unfold to level 9", |app, _| {
        app.expand_to_level(9)
    }),
    // --- The body pane ----------------------------------------------------
    //
    // The body's grammar lives in `editor::parse`. These entries document it
    // for the help screen; running one is a no-op, because the dispatcher
    // hands body keys to the editor before it reaches the binding table.
    c("edit-body", "edit this node's body", |app, _| {
        app.begin_body_edit()
    }),
    c("body-motions", "left, down, up, right", noop),
    c(
        "body-word-motions",
        "by word: forwards, back, to the end",
        noop,
    ),
    c(
        "body-line-motions",
        "line start and end, file, paragraph, bracket",
        noop,
    ),
    c(
        "body-find-char",
        "to a character on the line, and repeat",
        noop,
    ),
    c(
        "body-screen-motions",
        "top, middle, bottom of the pane",
        noop,
    ),
    c("body-operators", "delete, change, yank, indent, case", noop),
    c(
        "body-text-objects",
        "word, quoted, bracketed, paragraph",
        noop,
    ),
    c(
        "body-simple-edits",
        "delete, replace, substitute, join, case",
        noop,
    ),
    c("body-insert", "enter INSERT at the usual vim place", noop),
    c("body-visual", "select charwise, linewise", noop),
    c("body-put", "put the text register after, before", noop),
    c("body-repeat", "repeat the last change", noop),
    // --- Panes, files and history ----------------------------------------
    c("focus-to-tree", "focus the outline", |app, _| {
        app.focus = Focus::Tree
    }),
    c("focus-to-body", "focus the body", |app, _| {
        app.focus = Focus::Body
    }),
    c("page-down", "a screen down", |app, n| {
        repeat(app, n, |app| app.page(1.0))
    }),
    c("page-up", "a screen up", |app, n| {
        repeat(app, n, |app| app.page(-1.0))
    }),
    c("half-page-down", "half a screen down", |app, n| {
        repeat(app, n, |app| app.page(0.5))
    }),
    c("half-page-up", "half a screen up", |app, n| {
        repeat(app, n, |app| app.page(-0.5))
    }),
    c(
        "shrink-outline-pane",
        "give the body more room",
        |app, n| app.tree_percent = app.tree_percent.saturating_sub(5 * n as u16).max(15),
    ),
    c(
        "grow-outline-pane",
        "give the outline more room",
        |app, n| app.tree_percent = (app.tree_percent + 5 * n as u16).min(85),
    ),
    c("undo", "undo the last change", |app, n| {
        repeat(app, n, |app| {
            let name = app.doc.undoer.undo_name().unwrap_or("nothing").to_string();
            match app.doc.undo() {
                Some(p) => {
                    app.select(p);
                    app.message = format!("undo: {name}");
                }
                None => app.message = "nothing to undo".to_string(),
            }
            app.clamp_current();
        })
    }),
    c("redo", "redo the last undone change", |app, n| {
        repeat(app, n, |app| {
            let name = app.doc.undoer.redo_name().unwrap_or("nothing").to_string();
            match app.doc.redo() {
                Some(p) => {
                    app.select(p);
                    app.message = format!("redo: {name}");
                }
                None => app.message = "nothing to redo".to_string(),
            }
            app.clamp_current();
        })
    }),
    c("save", "write the .leo file", |app, _| app.save()),
    c(
        "write-at-file-nodes",
        "write the changed external files",
        |app, _| app.write_external(),
    ),
    // The command line runs this one: it takes an argument, and Tab completes
    // over the themes on disk. The entry is here so `:the` completes to it.
    c("theme", "change the theme, or name the current one", noop),
    // The command line runs this one too: it takes a file name.
    c("import-at-file", "import a file as an @file tree", noop),
    c("help", "show the key bindings", |app, _| {
        app.mode = Mode::Help;
        app.help_scroll = 0;
    }),
    c("close-help", "close the help screen", |app, _| {
        app.mode = Mode::Normal
    }),
    c("scroll-help-down", "scroll the help down", |app, n| {
        app.help_scroll += n
    }),
    c("scroll-help-up", "scroll the help up", |app, n| {
        app.help_scroll = app.help_scroll.saturating_sub(n)
    }),
    c(
        "scroll-help-page-down",
        "a screen of help down",
        |app, _| app.help_scroll += 10,
    ),
    c("scroll-help-page-up", "a screen of help up", |app, _| {
        app.help_scroll = app.help_scroll.saturating_sub(10)
    }),
    c("quit", "leave leotui", |app, _| app.request_quit()),
    // --- The command line and search --------------------------------------
    c("full-command", "open the : command line", |app, _| {
        app.open_mini(MiniKind::Command, String::new())
    }),
    c("search-forward", "search forwards", |app, _| {
        app.open_mini(MiniKind::SearchForward, String::new())
    }),
    c("search-backward", "search backwards", |app, _| {
        app.open_mini(MiniKind::SearchBackward, String::new())
    }),
    c("find-next", "repeat the search", |app, n| {
        app.repeat_search(true, n)
    }),
    c("find-prev", "repeat the search, the other way", |app, n| {
        app.repeat_search(false, n)
    }),
    c(
        "goto-visible-row",
        "select the Nth visible row",
        |app, n| app.move_to_row(n.saturating_sub(1)),
    ),
];
