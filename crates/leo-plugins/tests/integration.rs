//! The plugins at work through leoapp, as leotui and leogui run them.
#![cfg(feature = "markdown")]

use std::fs;
use std::sync::Arc;

use leoapp::app::App;
use leoapp::rendered::{rendered, Rendered};
use leolib::{Document, Position};

fn shown(doc: &Document, p: &Position) -> Rendered {
    rendered(doc.outline(), p, p.b(doc.outline()))
}

#[test]
fn register_gives_leoapp_the_kinds() {
    leo_plugins::register();
    assert_eq!(leoapp::plugins::kinds().directives(), ["@qmd", "@rmd"]);
    // A new document takes the app's kinds.
    let app = App::new(Document::new_empty(""));
    assert_eq!(app.outline().kinds().directives().len(), 2);
}

#[test]
fn an_outline_opened_through_leoapp_reads_its_kinds() {
    leo_plugins::register();
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("r.qmd"), "# From the file\n").unwrap();
    let leo = dir.path().join("x.leo");
    fs::write(
        &leo,
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<leo_file>\n<leo_header file_format=\"2\"/>\n\
         <vnodes>\n<v t=\"b.1\"><vh>@qmd r.qmd</vh></v>\n</vnodes>\n\
         <tnodes>\n<t tx=\"b.1\"></t>\n</tnodes>\n</leo_file>\n",
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
    assert_eq!(heads, ["@qmd r.qmd", "From the file"]);
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
