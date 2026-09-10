//! End-to-end tests: outline -> disk -> outline, through real files.

use std::fs;

use leolib::{external, Document, Outline};

/// A digest of the whole outline: gnx, headline and body of every node.
fn digest(o: &Outline) -> Vec<(String, String, String)> {
    o.all_positions()
        .iter()
        .map(|p| (p.gnx(o).to_string(), p.h(o).to_string(), p.b(o).to_string()))
        .collect()
}

#[test]
fn a_leo_file_survives_save_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.leo").to_string_lossy().to_string();

    let mut o = Outline::new_empty();
    let root = o.root_position().unwrap();
    o.set_headline(&root, "root");
    o.set_body(&root, "body with & < > and \u{00e9}\n");
    let child = o.insert_as_last_child(&root);
    o.set_headline(&child, "child");
    o.clone_node(&child);
    let before = digest(&o);

    leolib::save(&mut o, &path).unwrap();
    let o2 = leolib::open_outline(&path, false).unwrap();
    assert_eq!(digest(&o2), before);
}

#[test]
fn an_at_file_tree_round_trips_through_disk() {
    let dir = tempfile::tempdir().unwrap();
    let leo_path = dir.path().join("test.leo").to_string_lossy().to_string();

    let mut o = Outline::new_empty();
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@file sample.py");
    o.set_body(&root, "\"\"\"A module.\"\"\"\n@others\n");
    let a = o.insert_as_last_child(&root);
    o.set_headline(&a, "def f");
    o.set_body(&a, "def f():\n    @others\n");
    let a1 = o.insert_as_last_child(&a);
    o.set_headline(&a1, "the body of f");
    o.set_body(&a1, "return 42\n");
    o.file_name = leo_path.clone();

    let before = digest(&o);
    let result = external::write_external_files(&mut o, false);
    assert_eq!(result.written.len(), 1, "{:?}", result.errors);

    let text = fs::read_to_string(dir.path().join("sample.py")).unwrap();
    // Sentinels sit between the two lines; @others supplies the indentation.
    assert!(text.contains("def f():\n"), "{text}");
    assert!(text.contains("\n    return 42\n"), "{text}");

    leolib::save(&mut o, &leo_path).unwrap();
    let o2 = leolib::open_outline(&leo_path, true).unwrap();
    assert_eq!(digest(&o2), before);
}

#[test]
fn editing_an_external_file_updates_only_the_node_it_belongs_to() {
    // The @clean update algorithm: the file has no sentinels, so the outline
    // is the only record of which node a line belongs to.
    let dir = tempfile::tempdir().unwrap();
    let leo_path = dir.path().join("test.leo").to_string_lossy().to_string();

    let mut o = Outline::new_empty();
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@clean sample.py");
    o.set_body(&root, "@others\n");
    let a = o.insert_as_last_child(&root);
    o.set_headline(&a, "one");
    o.set_body(&a, "a = 1\n");
    let b = o.insert_as_last_child(&root);
    o.set_headline(&b, "two");
    o.set_body(&b, "b = 2\n");
    o.file_name = leo_path.clone();
    external::write_external_files(&mut o, false);

    let file = dir.path().join("sample.py");
    assert_eq!(fs::read_to_string(&file).unwrap(), "a = 1\nb = 2\n");

    // Edit the file behind the outline's back, as an editor would.
    fs::write(&file, "a = 1\na2 = 1\nb = 2\n").unwrap();
    // The mod-time cache is what stops a re-read; the file is genuinely newer.
    o.mod_time_cache.clear();
    let result = external::read_external_files(&mut o);
    assert!(result.errors.is_empty(), "{:?}", result.errors);

    let root = o.root_position().unwrap();
    let kids = root.children(&o);
    assert_eq!(kids[0].b(&o), "a = 1\na2 = 1\n");
    assert_eq!(kids[1].b(&o), "b = 2\n");
}

#[test]
fn writing_an_unchanged_outline_touches_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let leo_path = dir.path().join("test.leo").to_string_lossy().to_string();

    let mut o = Outline::new_empty();
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@file sample.py");
    o.set_body(&root, "x = 1\n");
    o.file_name = leo_path;
    assert_eq!(
        external::write_external_files(&mut o, false).written.len(),
        1
    );

    let second = external::write_external_files(&mut o, false);
    assert!(second.written.is_empty());
    assert_eq!(second.unchanged, 1);
}

#[test]
fn an_edit_and_its_undo_leave_the_outline_as_it_was() {
    let mut d = Document::new_empty("");
    let root = d.outline.root_position().unwrap();
    d.set_headline(&root, "root");
    d.undoer.clear();
    let before = digest(&d.outline);

    let child = d.insert_node(&root);
    d.set_headline(&child, "child");
    d.set_body(&child, "text\n");
    d.move_right(&child);
    d.toggle_marked(&child);

    while d.undoer.can_undo() {
        d.undo();
    }
    assert_eq!(digest(&d.outline), before);
}

#[test]
fn writing_refuses_to_overwrite_a_file_the_outline_never_read() {
    // Issue #50: the outline holds no copy of what is in that file, so
    // writing it would discard the file. With no one to ask, refuse.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("sample.py");
    fs::write(&file, "work that is only on disk\n").unwrap();

    let mut o = Outline::new_empty();
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@file sample.py");
    o.set_body(&root, "x = 1\n");
    o.file_name = dir.path().join("test.leo").to_string_lossy().to_string();

    let result = external::write_external_files(&mut o, false);
    assert!(result.written.is_empty());
    assert_eq!(result.errors.len(), 1);
    assert!(result.errors[0].message.contains("has not read"));
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "work that is only on disk\n"
    );

    // A failed read is not a read. The file has no sentinels, so reading it
    // brings none of its work into the outline, and the write stays refused.
    // `a_file_that_read_cleanly_can_still_be_written` covers a read that works.
    let report = external::read_external_files(&mut o);
    assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
    let result = external::write_external_files(&mut o, false);
    assert!(result.written.is_empty(), "{:?}", result.written);
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "work that is only on disk\n"
    );
}

#[test]
fn an_at_auto_tree_round_trips_through_disk() {
    // An @auto file has no sentinels: the tree is the only record of its
    // structure, so the write must reproduce the file exactly.
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("sample.py");
    let text = "\
\"\"\"A module.\"\"\"
import os


class C:

    def a(self):
        return os


def top():
    pass
";
    fs::write(&source, text).unwrap();

    let mut o = Outline::new_empty();
    o.file_name = dir.path().join("test.leo").to_string_lossy().to_string();
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@auto sample.py");

    let result = external::read_external_files(&mut o);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.read, 1);

    let heads: Vec<String> = o
        .all_positions()
        .iter()
        .map(|p| format!("{}{}", "  ".repeat(p.level()), p.h(&o)))
        .collect();
    assert_eq!(
        heads,
        vec!["@auto sample.py", "  class C", "    C.a", "  function: top"]
    );

    // Writing an untouched @auto node must not change the file.
    let written = external::write_external_files(&mut o, false);
    assert!(written.written.is_empty(), "{:?}", written.written);
    assert_eq!(written.unchanged, 1);
    assert_eq!(fs::read_to_string(&source).unwrap(), text);

    // An edit lands in the file, in the right place.
    let node = o.all_positions()[2].clone();
    let body = node.b(&o).replace("return os", "return None");
    o.set_body(&node, &body);
    let written = external::write_external_files(&mut o, true);
    assert_eq!(written.written.len(), 1, "{:?}", written.errors);
    assert_eq!(
        fs::read_to_string(&source).unwrap(),
        text.replace("return os", "return None")
    );
}

#[test]
fn an_at_auto_node_whose_importer_loses_text_keeps_the_whole_file() {
    // The file's own text contains @others, which the writer would read as a
    // directive. Rather than write a different file, the reader keeps the
    // file in the node's body and says so.
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("sample.py");
    let text = "x = 1\n@others\ny = 2\n";
    fs::write(&source, text).unwrap();

    let mut o = Outline::new_empty();
    o.file_name = dir.path().join("test.leo").to_string_lossy().to_string();
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@auto sample.py");

    let result = external::read_external_files(&mut o);
    assert_eq!(result.errors.len(), 1);
    assert!(result.errors[0].message.contains("did not reproduce"));
    assert_eq!(o.root_position().unwrap().b(&o), text);
}

/// A `.leo` file in `dir` holding one node, `headline`, over an existing
/// file `name` with `contents`. Returns the `.leo` path.
fn outline_over_existing_file(
    dir: &std::path::Path,
    headline: &str,
    name: &str,
    contents: &str,
) -> String {
    fs::write(dir.join(name), contents).unwrap();
    let leo_path = dir.join("test.leo").to_string_lossy().to_string();
    let mut o = Outline::new_empty();
    let root = o.root_position().unwrap();
    o.set_headline(&root, headline);
    o.file_name = leo_path.clone();
    leolib::save(&mut o, &leo_path).unwrap();
    leo_path
}

#[test]
fn a_file_that_failed_to_read_is_reported_and_never_overwritten() {
    // An @file node over a file with no sentinels reads as nothing. Writing
    // it after an edit would replace the file with sentinels around the edit.
    let dir = tempfile::tempdir().unwrap();
    let mine = "print('mine')\n";
    let leo_path = outline_over_existing_file(dir.path(), "@file plain.py", "plain.py", mine);

    let (mut o, report) = leolib::open_outline_with_report(&leo_path, true).unwrap();
    assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
    assert!(
        report.errors[0]
            .message
            .contains("not a valid external file"),
        "{:?}",
        report.errors
    );

    let root = o.root_position().unwrap();
    o.set_body(&root, "edited\n");
    o.set_dirty(&root);
    let result = external::write_external_files(&mut o, false);
    assert!(result.written.is_empty(), "{:?}", result.written);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.message.contains("refusing to overwrite")),
        "{:?}",
        result.errors
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("plain.py")).unwrap(),
        mine
    );
}

#[test]
fn an_auto_file_with_no_importer_is_reported_and_never_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let mine = "Title\n=====\n";
    let leo_path = outline_over_existing_file(dir.path(), "@auto notes.rst", "notes.rst", mine);

    let (mut o, report) = leolib::open_outline_with_report(&leo_path, true).unwrap();
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.message.contains("no @auto importer")),
        "{:?}",
        report.errors
    );

    let root = o.root_position().unwrap();
    o.set_body(&root, "edited\n");
    o.set_dirty(&root);
    let result = external::write_external_files(&mut o, false);
    assert!(result.written.is_empty(), "{:?}", result.written);
    assert_eq!(
        fs::read_to_string(dir.path().join("notes.rst")).unwrap(),
        mine
    );
}

#[test]
fn a_file_that_read_cleanly_can_still_be_written() {
    // The other side of the guard: moving the record after the read must not
    // stop an ordinary edit from reaching the file.
    let dir = tempfile::tempdir().unwrap();
    let leo_path = dir.path().join("test.leo").to_string_lossy().to_string();
    let mut o = Outline::new_empty();
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@file sample.py");
    o.set_body(&root, "x = 1\n");
    o.file_name = leo_path.clone();
    external::write_external_files(&mut o, false);
    leolib::save(&mut o, &leo_path).unwrap();

    let (mut o, report) = leolib::open_outline_with_report(&leo_path, true).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let root = o.root_position().unwrap();
    o.set_body(&root, "x = 2\n");
    o.set_dirty(&root);
    let result = external::write_external_files(&mut o, true);
    assert_eq!(result.written.len(), 1, "{:?}", result.errors);
    let text = fs::read_to_string(dir.path().join("sample.py")).unwrap();
    assert!(text.contains("x = 2\n"), "{text}");
}
