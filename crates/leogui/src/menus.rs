//! The menu bar, and the command list the palette searches.
//!
//! An item runs a `:` line through `App::run_chosen`, so a menu is one more
//! way to type a command, and its shortcut is read from the binding table.

use eframe::egui;
use leoapp::app::App;
use leoapp::bindings;
use leoapp::config::Appearance;

/// What a menu item does beyond running a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Palette,
    GoToNode,
    CodeActions,
    Find,
    New,
    Open,
    OpenRecent(usize),
    SaveAs,
    SaveCopy,
    Import,
    CloseOutline,
    Quit,
    Servers,
    Problems,
    Log,
    About,
    Appearance(Appearance),
    Themes,
    Settings,
}

/// An option a menu shows as a checkbox, and the `:set` lines that change it.
#[derive(Clone, Copy)]
pub enum Opt {
    Wrap,
    Number,
    Syntax,
}

pub enum Item {
    /// A label and the `:` line it runs.
    Run(&'static str, &'static str),
    /// A label and the `:` line it opens for the rest to be typed.
    Ask(&'static str, &'static str),
    Gui(&'static str, Action, &'static str),
    Toggle(&'static str, Opt),
    /// Dark, light, or as the system is.
    Looks,
    /// The recent outlines, as a submenu.
    Recent,
    Sep,
}

use Item::{Gui, Looks, Recent, Run, Sep, Toggle};

pub const MENUS: &[(&str, &[Item])] = &[
    (
        "File",
        &[
            Gui("New Outline", Action::New, ""),
            Gui("Open...", Action::Open, ""),
            Recent,
            Gui("Close Outline", Action::CloseOutline, ""),
            Sep,
            Run("Save", "save"),
            Gui("Save As...", Action::SaveAs, ""),
            Gui("Save a Copy...", Action::SaveCopy, ""),
            Sep,
            Run("Write Changed Files", "write-dirty-at-file-nodes"),
            Run("Write All Files Here", "write-at-file-nodes"),
            Run("Write Outline Only", "write-outline-only"),
            Sep,
            Run("Refresh From Disk", "refresh-from-disk"),
            Run("Read Files Here", "read-at-file-nodes"),
            Gui("Import File...", Action::Import, ""),
            Run("Revert", "revert"),
            Sep,
            Gui("Settings...", Action::Settings, "Cmd-,"),
            Sep,
            Gui("Quit", Action::Quit, "Ctrl-q"),
        ],
    ),
    (
        "Edit",
        &[
            Run("Undo", "undo"),
            Run("Redo", "redo"),
            Sep,
            Run("Cut Node", "cut-node"),
            Run("Copy Node", "copy-node"),
            Run("Paste Node", "paste-node"),
            Run("Delete Node", "delete-node"),
            Sep,
            Gui("Find Panel...", Action::Find, "Ctrl-Shift-f"),
            Run("Find...", "search-forward"),
            Run("Find Backwards...", "search-backward"),
            Run("Find Next", "find-next"),
            Run("Find Previous", "find-prev"),
            Item::Ask("Replace...", "s/"),
            Item::Ask("Clone Find All...", "clone-find-all "),
            Run("Clear Highlight", "nohlsearch"),
        ],
    ),
    (
        "Outline",
        &[
            Run("Insert Node", "insert-node"),
            Run("Insert Before", "insert-node-before"),
            Run("Insert Child", "insert-child"),
            Run("Clone Node", "clone-node"),
            Run("Rename Headline", "edit-headline"),
            Sep,
            Run("Move Up", "move-outline-up"),
            Run("Move Down", "move-outline-down"),
            Run("Move Left", "move-outline-left"),
            Run("Move Right", "move-outline-right"),
            Run("Promote", "promote"),
            Run("Demote", "demote"),
            Run("Sort Siblings", "sort-siblings"),
            Sep,
            Run("Mark", "mark"),
            Run("Unmark All", "unmark-all"),
            Run("Next Marked", "goto-next-marked"),
            Sep,
            Run("Hoist", "hoist"),
            Run("De-hoist", "dehoist"),
            Run("Expand All", "expand-all"),
            Run("Contract All", "contract-all"),
            Run("Go Back", "go-back"),
            Run("Go Forward", "go-forward"),
        ],
    ),
    (
        "Body",
        &[
            Run("Edit Body", "edit-body"),
            Run("Reformat Paragraph", "reformat-paragraph"),
            Run("Show File Line", "show-file-line"),
            Run("Go to File Line...", "goto-global-line"),
            Sep,
            Run("Hover", "lsp-hover"),
            Run("Go to Definition", "lsp-definition"),
            Run("Rename Symbol...", "lsp-rename"),
            Gui("Code Actions...", Action::CodeActions, "Ctrl-."),
            Run("Next Problem", "lsp-next-diagnostic"),
            Run("Previous Problem", "lsp-prev-diagnostic"),
        ],
    ),
    (
        "View",
        &[
            Gui("Command Palette...", Action::Palette, "Ctrl-Shift-p"),
            Gui("Go to Node...", Action::GoToNode, "Ctrl-p"),
            Gui("Problems", Action::Problems, ""),
            Gui("Log", Action::Log, ""),
            Gui("Language Servers", Action::Servers, ""),
            Sep,
            Toggle("Wrap Lines", Opt::Wrap),
            Toggle("Line Numbers", Opt::Number),
            Toggle("Syntax Colouring", Opt::Syntax),
            Sep,
            Looks,
            Gui("Theme...", Action::Themes, ""),
            Sep,
            Run("Wider Outline", "grow-outline-pane"),
            Run("Narrower Outline", "shrink-outline-pane"),
        ],
    ),
    (
        "Help",
        &[
            Run("Key Bindings", "help"),
            Run("Messages", "messages"),
            Gui("About leogui", Action::About, ""),
        ],
    ),
];

/// Commands that need an argument: the palette opens their `:` line.
pub const ASKS: &[&str] = &[
    "theme",
    "import-at-file",
    "import-auto",
    "clone-find-all",
    "clone-find-all-flattened",
    "save-as",
    "save-to",
    "open",
];

/// The first key bound to the command a `:` line runs, as the menu shows it.
pub fn shortcut(line: &str) -> String {
    let name = line.split_whitespace().next().unwrap_or("");
    bindings::keys_for(name)
        .into_iter()
        .next()
        .unwrap_or("")
        .to_string()
}

fn checked(app: &App, opt: Opt) -> (bool, &'static str, &'static str) {
    match opt {
        Opt::Wrap => (app.options.wrap, "set nowrap", "set wrap"),
        Opt::Number => (app.options.number, "set nonumber", "set number"),
        Opt::Syntax => (app.options.syntax, "set nosyntax", "set syntax"),
    }
}

/// The menu bar. Returns an action only the window can take.
pub fn bar(
    ui: &mut egui::Ui,
    app: &mut App,
    appearance: Appearance,
    recent: &[String],
) -> Option<Action> {
    let mut action = None;
    egui::MenuBar::new().ui(ui, |ui| {
        for (title, items) in MENUS {
            ui.menu_button(*title, |ui| {
                ui.set_min_width(220.0);
                for item in *items {
                    match item {
                        Sep => {
                            ui.separator();
                        }
                        Run(label, line) => {
                            let b = egui::Button::new(*label).shortcut_text(shortcut(line));
                            if ui.add(b).clicked() {
                                app.run_chosen(line);
                                ui.close();
                            }
                        }
                        Item::Ask(label, line) => {
                            if ui.button(*label).clicked() {
                                ask(app, line);
                                ui.close();
                            }
                        }
                        Gui(label, a, keys) => {
                            let b = egui::Button::new(*label).shortcut_text(*keys);
                            if ui.add(b).clicked() {
                                action = Some(*a);
                                ui.close();
                            }
                        }
                        Recent => {
                            ui.menu_button("Open Recent", |ui| {
                                let existing = recent
                                    .iter()
                                    .enumerate()
                                    .filter(|(_, f)| std::path::Path::new(f).exists());
                                let mut none = true;
                                for (i, file) in existing {
                                    none = false;
                                    let name = leolib::util::short_file_name(file);
                                    let b = ui.button(name).on_hover_text(file.as_str());
                                    if b.clicked() {
                                        action = Some(Action::OpenRecent(i));
                                        ui.close();
                                    }
                                }
                                if none {
                                    ui.label("No recent outlines");
                                }
                            });
                        }
                        Looks => {
                            for (a, label) in [
                                (Appearance::Dark, "Dark"),
                                (Appearance::Light, "Light"),
                                (Appearance::System, "Match System"),
                            ] {
                                if ui.radio(appearance == a, label).clicked() {
                                    action = Some(Action::Appearance(a));
                                    ui.close();
                                }
                            }
                        }
                        Toggle(label, opt) => {
                            let (mut on, off_line, on_line) = checked(app, *opt);
                            if ui.checkbox(&mut on, *label).clicked() {
                                app.run_chosen(if on { on_line } else { off_line });
                                ui.close();
                            }
                        }
                    }
                }
            });
        }
    });
    action
}

/// Open the `:` line with `line` typed, for the user to finish.
pub fn ask(app: &mut App, line: &str) {
    if app.run_chosen("") {
        app.open_mini(leoapp::minibuffer::MiniKind::Command, line.to_string());
    }
}

/// A command the palette offers: its name, summary and keys.
pub struct Entry {
    pub name: &'static str,
    pub summary: &'static str,
    pub keys: String,
}

/// Every command a person can run by name. The table's entries that only
/// document keys the body grammar handles (`body-motions`) are left out.
pub fn entries() -> Vec<Entry> {
    leoapp::commands::COMMANDS
        .iter()
        .filter(|c| {
            !c.name.starts_with("body-") && !c.name.contains("help-") && c.name != "close-help"
        })
        .map(|c| Entry {
            name: c.name,
            summary: c.summary,
            keys: bindings::keys_for(c.name).join("  "),
        })
        .collect()
}

/// How well `query` matches `text`: its characters in order, a run and an
/// early start scoring higher. None if they are not all there.
pub fn score(query: &str, text: &str) -> Option<i32> {
    let text = text.to_lowercase();
    let mut chars = text.char_indices().peekable();
    let mut score = 0;
    let mut last: Option<usize> = None;
    for q in query.to_lowercase().chars().filter(|c| !c.is_whitespace()) {
        let (i, _) = chars.find(|&(_, c)| c == q)?;
        score += match last {
            Some(l) if i == l + 1 => 5,
            _ => 1,
        };
        if i == 0 {
            score += 3;
        }
        last = Some(i);
    }
    Some(score * 100 - text.len() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_menu_line_names_a_command_the_app_runs() {
        // `open`, `set` and `s/` are command-line forms, not table names.
        for (_, items) in MENUS {
            for item in *items {
                if let Run(label, line) = item {
                    let name = line.split_whitespace().next().unwrap();
                    assert!(
                        leoapp::commands::find(name).is_some() || ["set", "help"].contains(&name),
                        "{label}: {line} runs nothing"
                    );
                }
            }
        }
    }

    #[test]
    fn a_shortcut_is_the_first_binding() {
        assert_eq!(shortcut("save"), "Ctrl-s");
        assert_eq!(shortcut("write-outline-only"), "");
    }

    #[test]
    fn the_palette_matches_in_order_and_prefers_runs() {
        assert!(score("ins ch", "insert-child").is_some());
        assert!(score("xz", "insert-child").is_none());
        assert!(score("save", "save").unwrap() > score("save", "save-as").unwrap());
        assert!(
            score("ins", "insert-node").unwrap()
                > score("ins", "clone-find-all-flattened").unwrap_or(i32::MIN)
        );
        assert!(!entries().iter().any(|e| e.name == "body-motions"));
    }
}
