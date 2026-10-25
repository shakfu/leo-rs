//! Entry points for the fuzz targets in `fuzz/`, behind the `fuzzing` feature.
//!
//! The parsers they reach are crate-private; this module is not an API.

use crate::outline::Outline;

/// Read `contents` as an `@file` external file into a fresh outline.
pub fn read_into_root(contents: &str) {
    let mut o = Outline::new_empty();
    let root = o.root_position().expect("a new outline has a node");
    o.set_headline(&root, "@file fuzz.py");
    let _ = crate::atfile_read::read_into_root(&mut o, contents, "fuzz.py", &root);
}

/// Read `contents` as a `.leo` file into a fresh outline. One that reads must
/// save to XML that reads again.
pub fn read_leo(contents: &str) {
    let mut o = Outline::new("");
    if crate::leofile::read_leo_string(&mut o, contents).is_ok() {
        let xml = crate::leofile::outline_to_xml_string(&mut o);
        let mut again = Outline::new("");
        crate::leofile::read_leo_string(&mut again, &xml).expect("a saved outline reads");
    }
}

/// Unpickle `bytes` as a uA blob. A value that reads must write back to bytes
/// that read as the same value.
pub fn pickle(bytes: &[u8]) {
    if let Ok(value) = crate::pickle::loads(bytes) {
        let again = crate::pickle::loads(&crate::pickle::dumps(&value));
        assert_eq!(again.ok(), Some(value));
    }
}

/// Import `text` with each line importer. The tree is at most one node per
/// byte, its parent links agree, and it writes.
pub fn line_import(text: &str) {
    for name in ["a.org", "a.otl", "a.md", "a.hjt"] {
        let mut o = Outline::new_empty();
        let root = o.root_position().expect("a new outline has a node");
        o.set_headline(&root, &format!("@auto {name}"));
        let _ = crate::importers::import_string(&mut o, &root, text, name);
        let nodes = o.all_positions().len();
        assert!(nodes <= text.len() + 1, "{name}: {nodes} nodes");
        o.links_agree().unwrap();
        crate::importers::write_string(&o, &root, name).expect("a line importer writes");
    }
}

/// Every position's gnx, level, headline, body and mark, in outline order.
fn digest(o: &Outline) -> Vec<(String, usize, String, String, bool)> {
    o.all_positions()
        .iter()
        .map(|p| {
            let (h, b) = (p.h(o).to_string(), p.b(o).to_string());
            (p.gnx(o).to_string(), p.level(), h, b, p.is_marked(o))
        })
        .collect()
}

/// Edit an outline as `bytes` say, two bytes an edit: which, and where. Parent
/// links agree after every step, undoing all gives the start back, and redoing
/// all the end.
pub fn undo(bytes: &[u8]) {
    use crate::document::{Document, Place};
    let mut doc = Document::new_empty("");
    let root = doc
        .outline()
        .root_position()
        .expect("a new outline has a node");
    doc.set_headline(&root, "root");
    doc.clear_undo();
    let start = digest(doc.outline());
    for (step, pair) in bytes.chunks(2).take(64).enumerate() {
        let all = doc.outline().all_positions();
        let at = |i: usize| all[i % all.len()].clone();
        let p = at(pair.get(1).copied().unwrap_or(0).into());
        let q = at(step);
        match pair[0] % 20 {
            0 => doc.set_headline(&p, &format!("h{step}")),
            1 => doc.set_body(&p, &format!("body {step}\n")),
            2 => drop(doc.insert_node(&p)),
            3 => drop(doc.insert_child(&p)),
            4 => drop(doc.delete_node(&p)),
            5 => drop(doc.clone_node(&p)),
            6 => drop(doc.demote(&p)),
            7 => drop(doc.promote(&p)),
            8 => drop(doc.move_up(&p)),
            9 => drop(doc.move_down(&p)),
            10 => drop(doc.move_left(&p)),
            11 => drop(doc.move_right(&p)),
            12 => doc.toggle_marked(&p),
            13 => drop(doc.clone_marked(&p)),
            14 => drop(doc.move_marked(&p)),
            15 => drop(doc.clone_find_all("h", "", false, |o, r| r.h(o).starts_with('h'))),
            16 => drop(doc.sort_children(&p)),
            17 => drop(doc.move_node(&p, &q, Place::Inside)),
            18 => drop(doc.move_node(&p, &q, Place::Before)),
            _ => drop(doc.delete_nodes(&[p, q])),
        }
        doc.outline().links_agree().unwrap();
    }
    let end = digest(doc.outline());
    while doc.undoer().can_undo() {
        doc.undo();
        doc.outline().links_agree().unwrap();
    }
    assert_eq!(digest(doc.outline()), start, "undo all");
    while doc.undoer().can_redo() {
        doc.redo();
        doc.outline().links_agree().unwrap();
    }
    assert_eq!(digest(doc.outline()), end, "redo all");
}

/// Convert `contents` as a notebook. The text of one that converts must write
/// back; any text must convert to a notebook or be refused.
pub fn jupytext(contents: &str) {
    if let Ok(t) = crate::jupytext::notebook_to_text(contents) {
        crate::jupytext::text_to_notebook(&t.text, Some(contents)).expect("its text writes back");
    }
    let _ = crate::jupytext::text_to_notebook(contents, None);
}
