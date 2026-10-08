//! The plugins at work through leoapp, as leotui and leogui run them.

use std::fs;
use std::sync::Arc;

use leoapp::app::{App, Mode};
use leoapp::keys::{self, KeyCode, KeyEvent, KeyModifiers};
use leoapp::rendered::{rendered, Rendered};
use leolib::{Document, Position};

fn press(app: &mut App, spec: &str) {
    for key in keys::parse(spec) {
        app.handle_key(KeyEvent::new(key.code, key.mods));
    }
}

fn shown(doc: &Document, p: &Position) -> Rendered {
    rendered(doc.outline(), p, p.b(doc.outline()))
}

#[test]
fn register_gives_leoapp_the_kinds_and_the_app_plugin() {
    leo_plugins::register();
    assert_eq!(
        leoapp::plugins::kinds().directives(),
        ["@entangled", "@qmd", "@rmd"]
    );
    // The plugin's commands are found, completed and listed as the core's.
    let tangle = leoapp::commands::find("entangled-tangle").expect("a plugin command");
    assert!(tangle.summary.contains("entangled tangle"));
    assert!(leoapp::minibuffer::completions("entangled-")
        .iter()
        .any(|c| c == "entangled-check"));
    assert!(leoapp::plugins::with_argument("entangled-tangle").is_some());
    assert!(leoapp::plugins::reads_setting("entangled"));
    let labels: Vec<&str> = leoapp::plugins::menu("Body").map(|e| e.label).collect();
    assert_eq!(labels, ["Tangle with entangled", "Check with entangled"]);
    // A new document takes the app's kinds.
    let app = App::new(Document::new_empty(""));
    assert_eq!(app.outline().kinds().directives().len(), 3);
}

#[test]
fn an_outline_opened_through_leoapp_reads_both_kinds() {
    leo_plugins::register();
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("doc.md"), "# From the file\n").unwrap();
    fs::write(dir.path().join("r.qmd"), "# From the file\n").unwrap();
    let leo = dir.path().join("x.leo");
    fs::write(
        &leo,
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<leo_file>\n<leo_header file_format=\"2\"/>\n\
         <vnodes>\n<v t=\"a.1\"><vh>@entangled doc.md</vh></v>\n\
         <v t=\"b.1\"><vh>@qmd r.qmd</vh></v>\n</vnodes>\n\
         <tnodes>\n<t tx=\"a.1\"></t>\n<t tx=\"b.1\"></t>\n</tnodes>\n</leo_file>\n",
    )
    .unwrap();
    let (doc, new) = leoapp::app::open_or_new(&leo.to_string_lossy(), true).unwrap();
    assert!(!new);
    let o = doc.outline();
    let heads: Vec<String> = o
        .all_positions()
        .iter()
        .map(|p| p.h(o).to_string())
        .collect();
    assert_eq!(
        heads,
        [
            "@entangled doc.md",
            "From the file",
            "@qmd r.qmd",
            "From the file"
        ]
    );
}

#[test]
fn a_key_a_plugin_reads_is_kept_for_it() {
    leo_plugins::register();
    let c = leoapp::config::parse(
        "entangled = \"/opt/bin/entangled\"\nentangle = x\nentangled = \"\"\n",
    );
    assert_eq!(c.plugin["entangled"], "/opt/bin/entangled");
    assert_eq!(
        c.warnings,
        [
            "config line 2: unknown setting entangle",
            "config line 3: entangled is empty"
        ]
    );
}

#[test]
fn editing_a_fence_nodes_headline_renames_its_block() {
    leo_plugins::register();
    let mut doc = Document::new_empty("");
    let root = doc.outline().root_position().unwrap();
    doc.set_headline(&root, "@entangled doc.md");
    doc.set_body(&root, "```python #add\n<< add >>\n```\n");
    let add = doc.outline_mut_untracked().insert_as_last_child(&root);
    doc.set_headline(&add, "<< add >>");
    doc.set_body(&add, "x = 1\n");
    doc.clear_undo();
    let mut app = App::new(doc);
    app.select(add.clone());
    press(&mut app, "e");
    // Ctrl-u in a headline is Leo's move-node-up, so clear it key by key.
    for _ in 0.."<< add >>".len() {
        app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    }
    app.handle_paste("<< sum >>");
    press(&mut app, "Enter");
    assert!(
        app.message.starts_with("renamed add to sum: 1 fence"),
        "{}",
        app.message
    );
    let o = app.outline();
    assert_eq!(add.h(o), "<< sum >>");
    assert_eq!(root.b(o), "```python #sum\n<< sum >>\n```\n");
    // One undo step puts both back.
    press(&mut app, "u");
    assert_eq!(app.outline().node(add.v).h, "<< add >>");
    assert_eq!(root.b(app.outline()), "```python #add\n<< add >>\n```\n");
}

#[test]
fn an_include_fence_node_is_read_only() {
    leo_plugins::register();
    let dir = std::env::temp_dir().join(format!("leoapp-include-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("lib.py"), "x = 1\n").unwrap();
    std::fs::write(
        dir.join("doc.md"),
        "```python #lib include=lib.py\nx = 1\n```\n",
    )
    .unwrap();
    let leo = dir.join("doc.leo");
    std::fs::write(
        &leo,
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<leo_file>\n<leo_header file_format=\"2\"/>\n\
         <vnodes>\n<v t=\"a.1\"><vh>@entangled doc.md</vh></v>\n</vnodes>\n\
         <tnodes>\n<t tx=\"a.1\"></t>\n</tnodes>\n</leo_file>\n",
    )
    .unwrap();
    let mut app =
        App::new(Document::open_with(&leo.to_string_lossy(), true, leo_plugins::kinds()).unwrap());
    let lib = app
        .outline()
        .all_positions()
        .into_iter()
        .find(|p| p.h(app.outline()) == "<< lib >>")
        .unwrap();
    app.select(lib.clone());
    press(&mut app, "Tab");
    press(&mut app, "i");
    assert_eq!(app.mode, Mode::Normal);
    assert!(
        app.message
            .starts_with("read-only: filled from include=lib.py"),
        "{}",
        app.message
    );
    press(&mut app, "x");
    assert_eq!(lib.b(app.outline()), "x = 1\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_entangled_heading_is_its_markdown_with_the_code_in_place() {
    let dir = std::env::temp_dir();
    let mut doc = Document::new_empty(&dir.join("x.leo").to_string_lossy());
    doc.outline_mut_untracked()
        .set_kinds(Arc::new(leo_plugins::kinds()));
    let root = doc.outline().root_position().unwrap();
    doc.set_headline(&root, "@entangled doc.md");
    doc.set_body(&root, "Intro.\n\n```python #add\n<< add >>\n```\n");
    let add = doc.outline_mut_untracked().insert_as_last_child(&root);
    doc.set_headline(&add, "<< add >>");
    doc.set_body(&add, "x = 1\n");
    let r = shown(&doc, &root);
    assert!(
        matches!(&r, Rendered::Markdown { text, .. } if text == "Intro.\n\n```python\nx = 1\n```\n"),
        "{r:?}"
    );
}
#[test]
fn a_qmd_heading_shows_its_cells_with_plain_fences() {
    let dir = std::env::temp_dir();
    let mut doc = Document::new_empty(&dir.join("x.leo").to_string_lossy());
    doc.outline_mut_untracked()
        .set_kinds(Arc::new(leo_plugins::kinds()));
    let root = doc.outline().root_position().unwrap();
    doc.set_headline(&root, "@qmd report.qmd");
    doc.set_body(&root, "Intro.\n\n```{python}\n<< python cell 1 >>\n```\n");
    let cell = doc.outline_mut_untracked().insert_as_last_child(&root);
    doc.set_headline(&cell, "<< python cell 1 >>");
    doc.set_body(&cell, "x = 1\n");
    let r = shown(&doc, &root);
    assert!(
        matches!(&r, Rendered::Markdown { text, .. } if text == "Intro.\n\n```python\nx = 1\n```\n"),
        "{r:?}"
    );
}
