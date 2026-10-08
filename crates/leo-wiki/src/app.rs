//! The wiki plugin: following `[[links]]`, completing them, keeping the
//! rules, and `:export-wiki`. Built with the `leoapp` feature.

use leolib::{Outline, Position};

use leoapp::app::App;
use leoapp::commands::Command;
use leoapp::plugins::{AppPlugin, MenuEntry};

use crate::{check, escape, export, export_path, links, resolve, root_of, wikilinks_to_unls};

/// The plugin.
pub struct WikiPlugin;

impl AppPlugin for WikiPlugin {
    fn name(&self) -> &'static str {
        "wiki"
    }
    fn commands(&self) -> &'static [Command] {
        &COMMANDS
    }
    fn menu(&self) -> &'static [MenuEntry] {
        &MENU
    }
    fn open_url(&self, app: &mut App) -> bool {
        follow(app)
    }
    fn complete(&self, app: &App) -> Option<(usize, Vec<String>)> {
        complete(app)
    }
    fn violations(&self, o: &Outline) -> Vec<String> {
        check(o)
    }
}

static COMMANDS: [Command; 3] = [
    Command {
        name: "export-wiki",
        summary: "write the @wiki at or above this node to NAME.md, links as markdown links",
        run: |app, _| export_wiki(app),
    },
    Command {
        name: "convert-wikilinks-to-unls",
        summary: "rewrite this wiki's [[links]] as Leo's unl:gnx:// links",
        run: |app, _| convert(app),
    },
    Command {
        name: "check-wiki",
        summary: "list the @wiki rules the outline breaks",
        run: |app, _| {
            let broken = check(app.outline());
            app.message = match broken.len() {
                0 => "every wiki keeps the rules".to_string(),
                n => format!("{n} broken: {}", broken.join("; ")),
            };
        },
    },
];

static MENU: [MenuEntry; 1] = [MenuEntry {
    menu: "Outline",
    label: "Export Wiki",
    command: "export-wiki",
}];

/// The wiki root at or above the current node, or a message saying there is none.
fn current_root(app: &mut App) -> Option<Position> {
    let root = root_of(app.outline(), &app.current);
    if root.is_none() {
        app.message = "not in a wiki: select an @wiki node or a page under one".into();
    }
    root
}

fn export_wiki(app: &mut App) {
    let Some(root) = current_root(app) else {
        return;
    };
    let o = app.outline();
    let path = export_path(o, &root);
    // A file an `@<file>` node writes is that node's, not the wiki's.
    let owned = o
        .all_positions()
        .into_iter()
        .any(|p| p.is_any_at_file_node(o) && o.full_path(&p) == path);
    if owned {
        app.message = format!("not exported: {path} is an @<file> node's file");
        return;
    }
    app.message = match export(o, &root) {
        Ok(text) => match leolib::external::replace_file(&path, &text, false) {
            Ok(_) => format!("exported {path}"),
            Err(e) => format!("not exported: {e}"),
        },
        Err(problems) => format!("not exported: {}", problems.join("; ")),
    };
}

fn convert(app: &mut App) {
    let Some(root) = current_root(app) else {
        return;
    };
    match wikilinks_to_unls(app.outline(), &root) {
        Ok(edits) => {
            let n = edits.len();
            app.doc.begin_group("convert-wikilinks-to-unls");
            for (p, body) in edits {
                app.doc.set_body(&p, &body);
            }
            app.doc.end_group();
            app.message = format!("converted the links of {n} pages");
        }
        Err(problems) => app.message = format!("not converted: {}", problems.join("; ")),
    }
}

/// The byte offset of the body cursor in its line, and the line.
fn cursor(app: &App) -> Option<(String, usize, usize)> {
    let lines = app.body_buffer();
    let (row, col) = app.editor.cursor;
    let line = lines.get(row)?.clone();
    let at = line.char_indices().nth(col).map_or(line.len(), |(i, _)| i);
    Some((line, row, at))
}

/// Follow the `[[link]]` under the cursor in a page.
fn follow(app: &mut App) -> bool {
    let Some(root) = root_of(app.outline(), &app.current) else {
        return false;
    };
    let Some((line, _, at)) = cursor(app) else {
        return false;
    };
    let Some(link) = links(&line)
        .into_iter()
        .find(|l| l.range.start <= at && at < l.range.end)
    else {
        return false;
    };
    let spelled = line[link.range.clone()].to_string();
    let targets = resolve(app.outline(), &root, &link);
    match targets.as_slice() {
        [] => app.message = format!("{spelled} names no page"),
        [p] => {
            let p = p.clone();
            app.focus = leoapp::app::Focus::Tree;
            app.select(p);
        }
        many => {
            app.message = format!(
                "{spelled} names {} pages; add a parent to choose: [[Parent/Page]]",
                many.len()
            )
        }
    }
    true
}

/// After `[[` in a page: the headlines of its wiki's pages.
fn complete(app: &App) -> Option<(usize, Vec<String>)> {
    let o = app.outline();
    let root = root_of(o, &app.current)?;
    let (line, _, at) = cursor(app)?;
    let before = &line[..at];
    let open = before.rfind("[[")?;
    // Inside a link still being typed: no `]]` since its `[[`.
    if before[open..].contains("]]") {
        return None;
    }
    let start = before[..open + 2].chars().count();
    let mut names: Vec<String> = root
        .subtree(o)
        .into_iter()
        .filter(|q| *q != root)
        .map(|q| escape(q.h(o).trim()))
        .collect();
    names.sort();
    names.dedup();
    Some((start, names))
}
