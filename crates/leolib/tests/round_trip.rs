//! End-to-end tests: outline -> disk -> outline, through real files.

use std::fs;

use leolib::{external, Document, Error, Outline};

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
    let root = o.root_position().unwrap();
    let result = external::refresh_files(&mut o, vec![root]);
    assert!(result.errors.is_empty(), "{:?}", result.errors);

    let root = o.root_position().unwrap();
    let kids = root.children(&o);
    assert_eq!(kids[0].b(&o), "a = 1\na2 = 1\n");
    assert_eq!(kids[1].b(&o), "b = 2\n");
}

#[test]
fn only_a_refresh_reads_an_at_clean_file_unchanged_since_the_write() {
    let dir = tempfile::tempdir().unwrap();
    let mut o = Outline::new_empty();
    o.file_name = dir.path().join("test.leo").to_string_lossy().to_string();
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@clean sample.py");
    o.set_body(&root, "a = 1\n");
    external::write_external_files(&mut o, false);

    // An edit within the mtime's resolution leaves the mtime as written.
    let path = dir.path().join("sample.py");
    let mtime = fs::metadata(&path).unwrap().modified().unwrap();
    fs::write(&path, "a = 2\n").unwrap();
    let file = fs::File::options().write(true).open(&path).unwrap();
    file.set_modified(mtime).unwrap();

    let result = external::read_files(&mut o, vec![root.clone()]);
    assert_eq!(result.read, 0, "{:?}", result.errors);
    assert_eq!(root.b(&o), "a = 1\n");
    let result = external::refresh_files(&mut o, vec![root.clone()]);
    assert_eq!(result.read, 1, "{:?}", result.errors);
    assert_eq!(root.b(&o), "a = 2\n");
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
    let root = d.outline_mut_untracked().root_position().unwrap();
    d.set_headline(&root, "root");
    d.clear_undo();
    let before = digest(d.outline());

    let child = d.insert_node(&root);
    d.set_headline(&child, "child");
    d.set_body(&child, "text\n");
    d.move_right(&child);
    d.toggle_marked(&child);

    while d.undoer().can_undo() {
        d.undo();
    }
    assert_eq!(digest(d.outline()), before);
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
    assert!(matches!(
        result.errors[0].error,
        Error::RefusedOverwrite { .. }
    ));
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

/// An outline at `dir/x.leo` with one node, `headline`, saved and reopened.
fn saved_outline(dir: &std::path::Path, headline: &str, body: &str, read: bool) -> Document {
    let leo = dir.join("x.leo").to_string_lossy().to_string();
    let mut o = leolib::new_outline(&leo);
    let root = o.root_position().unwrap();
    o.set_headline(&root, headline);
    o.set_body(&root, body);
    leolib::save(&mut o, "").unwrap();
    Document::open(&leo, read).unwrap()
}

#[test]
fn a_new_clean_or_nosent_node_does_not_overwrite_a_file_it_never_read() {
    for kind in ["@clean", "@nosent", "@asis"] {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("b.py");
        fs::write(&file, "x = 1\ny = 2\n").unwrap();
        let mut o = leolib::new_outline(&dir.path().join("x.leo").to_string_lossy());
        let root = o.root_position().unwrap();
        o.set_headline(&root, &format!("{kind} b.py"));
        let result = external::write_external_files(&mut o, false);
        assert!(result.written.is_empty(), "{kind}: {:?}", result.written);
        assert!(
            matches!(result.errors[0].error, Error::RefusedOverwrite { .. }),
            "{kind}: {:?}",
            result.errors
        );
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "x = 1\ny = 2\n",
            "{kind}"
        );
    }
}

#[test]
fn a_write_only_node_the_leo_file_had_is_written_on_the_next_open() {
    for kind in ["@nosent", "@asis"] {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("w.txt"), "old\n").unwrap();
        let mut doc = saved_outline(dir.path(), &format!("{kind} w.txt"), "new\n", true);
        let root = doc.outline().root_position().unwrap();
        doc.set_body(&root, "newer\n");
        let result = doc.write_external_files(false);
        assert!(result.errors.is_empty(), "{kind}: {:?}", result.errors);
        assert_eq!(
            fs::read_to_string(dir.path().join("w.txt")).unwrap(),
            "newer\n"
        );
    }
}

#[test]
fn an_edit_to_an_unchanged_clean_file_is_seen_and_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("c.txt");
    fs::write(&file, "line\n").unwrap();
    let mut doc = saved_outline(dir.path(), "@clean c.txt", "line\n", true);
    let path = file.to_string_lossy().to_string();
    // Another program appends; the size alone changes the stamp.
    fs::write(&file, "line\nTHEIRS\n").unwrap();
    assert_eq!(doc.outline().changed_files(), [path]);
    let root = doc.outline().root_position().unwrap();
    doc.set_body(&root, "line\nours\n");
    let result = doc.write_external_files(true);
    assert!(result.written.is_empty(), "{:?}", result.written);
    assert_eq!(fs::read_to_string(&file).unwrap(), "line\nTHEIRS\n");
}

#[test]
fn a_clean_file_not_read_at_open_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("c.txt");
    fs::write(&file, "old line\n").unwrap();
    let mut doc = saved_outline(dir.path(), "@clean c.txt", "old line\n", true);
    drop(doc);
    fs::write(&file, "old line\nEXTERNAL EDIT\n").unwrap();
    doc = Document::open(&dir.path().join("x.leo").to_string_lossy(), false).unwrap();
    let root = doc.outline().root_position().unwrap();
    doc.set_body(&root, "old line\nours\n");
    let result = doc.write_external_files(true);
    assert!(result.written.is_empty(), "{:?}", result.written);
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "old line\nEXTERNAL EDIT\n"
    );
}

#[test]
fn an_at_file_cut_short_before_its_end_sentinel_is_not_read_or_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = saved_outline(dir.path(), "@file a.py", "import os\nprint(1)\n", false);
    doc.write_external_files(false);
    let file = dir.path().join("a.py");
    let full = fs::read_to_string(&file).unwrap();
    let cut: String = full
        .lines()
        .take_while(|l| !l.contains("@-leo"))
        .map(|l| format!("{l}\n"))
        .collect();
    assert_ne!(cut, full);
    fs::write(&file, &cut).unwrap();
    let mut doc = Document::open(&dir.path().join("x.leo").to_string_lossy(), true).unwrap();
    let report = format!("{:?}", doc.read_report.errors);
    assert!(report.contains("no @-leo line"), "{report}");
    let root = doc.outline().root_position().unwrap();
    doc.set_body(&root, "# added\n");
    let result = doc.write_external_files(false);
    assert!(result.written.is_empty(), "{:?}", result.written);
    assert_eq!(fs::read_to_string(&file).unwrap(), cut);
}

#[test]
fn a_file_that_fails_to_read_keeps_its_descendants_uas() {
    let dir = tempfile::tempdir().unwrap();
    let leo = dir.path().join("x.leo").to_string_lossy().to_string();
    let mut o = leolib::new_outline(&leo);
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@file x.py");
    o.set_body(&root, "@others\n");
    let child = o.insert_as_last_child(&root);
    o.set_headline(&child, "f");
    o.set_body(&child, "def f(): pass\n");
    o.node_mut(child.v)
        .uas
        .insert("str_tag".into(), leolib::node::Ua::Text("kept".into()));
    external::write_external_files(&mut o, false);
    leolib::save(&mut o, "").unwrap();
    let py = dir.path().join("x.py");
    let good = fs::read(&py).unwrap();
    // The file becomes unreadable; the outline is opened and saved.
    fs::write(&py, b"caf\xe9\n").unwrap();
    let mut doc = Document::open(&leo, true).unwrap();
    assert_eq!(doc.read_report.errors.len(), 1);
    doc.save(&leo).unwrap();
    // With the file back, the child has its uA again.
    fs::write(&py, good).unwrap();
    let doc = Document::open(&leo, true).unwrap();
    let o = doc.outline();
    let f = o
        .all_positions()
        .into_iter()
        .find(|p| p.h(o) == "f")
        .unwrap();
    assert_eq!(
        o.node(f.v).uas.get("str_tag"),
        Some(&leolib::node::Ua::Text("kept".into()))
    );
}

#[test]
fn an_outlines_settings_shape_what_it_writes() {
    let dir = tempfile::tempdir().unwrap();
    let leo = dir.path().join("x.leo").to_string_lossy().to_string();
    let mut o = leolib::new_outline(&leo);
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@settings");
    let s = o.insert_as_last_child(&root);
    o.set_headline(&s, "@string output-newline = crlf");
    let f = o.insert_after(&root);
    o.set_headline(&f, "@nosent n.py");
    o.set_body(&f, "x = 1\n");
    leolib::save(&mut o, "").unwrap();
    let mut doc = Document::open(&leo, true).unwrap();
    assert_eq!(doc.outline().config.output_newline, "crlf");
    let result = doc.write_external_files(false);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(fs::read(dir.path().join("n.py")).unwrap(), b"x = 1\r\n");
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
    assert!(matches!(result.errors[0].error, Error::Import { .. }));
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
        matches!(report.errors[0].error, Error::NotAnExternalFile { .. }),
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
            .any(|e| matches!(e.error, Error::RefusedOverwrite { .. })),
        "{:?}",
        result.errors
    );
    assert_eq!(result.refused, vec![root.clone()]);
    assert_eq!(
        fs::read_to_string(dir.path().join("plain.py")).unwrap(),
        mine
    );

    // Approval is the caller's to record; the next write then goes through.
    let path = o.full_path(&root);
    o.remember_read_path(&root, &path);
    let result = external::write_external_files(&mut o, false);
    assert_eq!(result.written, vec![path]);
    assert!(result.refused.is_empty());
}

/// An empty outline that would be saved in `dir`, so imports go relative to it.
fn outline_in(dir: &std::path::Path) -> leolib::Document {
    leolib::Document::new_empty(&dir.join("test.leo").to_string_lossy())
}

/// Each body in p's tree, p first.
fn bodies(o: &Outline, p: &leolib::Position) -> Vec<String> {
    let mut out = vec![p.b(o).to_string()];
    for child in p.children(o) {
        out.extend(bodies(o, &child));
    }
    out
}

#[test]
fn import_at_file_splits_a_plain_file_and_keeps_its_shebang_first() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.py").to_string_lossy().to_string();
    let text =
        "#!/usr/bin/env python3\nimport os\n\ndef f():\n    return 1\n\ndef g():\n    return 2\n";
    fs::write(&path, text).unwrap();
    let mut doc = outline_in(dir.path());
    let root = doc.outline().root_position().unwrap();

    let (p, needs_write) = doc.import_at_file(&root, &path).unwrap();
    assert!(needs_write);
    assert_eq!(p.h(doc.outline()), "@file x.py");
    assert!(
        p.b(doc.outline())
            .starts_with("@first #!/usr/bin/env python3\n"),
        "{}",
        p.b(doc.outline())
    );
    assert_eq!(p.children(doc.outline()).len(), 2);

    // The sentinels wait for the caller's approval.
    let result = doc.write_external_files(true);
    assert_eq!(result.refused, vec![p.clone()]);
    assert_eq!(fs::read_to_string(&path).unwrap(), text);

    doc.outline_mut_untracked().remember_read_path(&p, &path);
    assert_eq!(doc.write_files(vec![p.clone()]).written, vec![path.clone()]);
    let written = fs::read_to_string(&path).unwrap();
    assert!(
        written.starts_with("#!/usr/bin/env python3\n# @+leo-ver=5-thin\n"),
        "{written}"
    );

    // Reading the written file gives back the imported tree.
    let before = bodies(doc.outline(), &p);
    external::read_file_at_position(doc.outline_mut_untracked(), &p).unwrap();
    assert_eq!(bodies(doc.outline(), &p), before);
}

#[test]
fn import_at_file_reads_a_file_that_has_sentinels() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("y.py").to_string_lossy().to_string();
    let mut src = outline_in(dir.path());
    let root = src.outline_mut_untracked().root_position().unwrap();
    src.outline_mut_untracked()
        .set_headline(&root, "@file y.py");
    src.outline_mut_untracked().set_body(&root, "@others\n");
    let child = src.outline_mut_untracked().insert_as_last_child(&root);
    src.outline_mut_untracked().set_headline(&child, "f");
    src.outline_mut_untracked()
        .set_body(&child, "def f():\n    return 1\n");
    assert_eq!(src.write_external_files(false).written, vec![path.clone()]);

    let mut doc = outline_in(dir.path());
    let at = doc.outline().root_position().unwrap();
    let (p, needs_write) = doc.import_at_file(&at, &path).unwrap();
    assert!(!needs_write);
    let kids = p.children(doc.outline());
    assert_eq!(kids.len(), 1);
    assert_eq!(kids[0].h(doc.outline()), "f");
}

#[test]
fn import_at_file_keeps_a_file_whole_when_its_tree_would_not_write_it_back() {
    // The markdown importer turns `#` lines into headlines, which @file does
    // not write as text.
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in [
        ("notes.md", "# Title\n\ntext\n\n## Sub\n\nmore\n"),
        ("data.zzz", "no importer\n"),
    ] {
        let path = dir.path().join(name).to_string_lossy().to_string();
        fs::write(&path, text).unwrap();
        let mut doc = outline_in(dir.path());
        let root = doc.outline().root_position().unwrap();
        let (p, _) = doc.import_at_file(&root, &path).unwrap();
        assert_eq!(p.b(doc.outline()), text, "{name}");
        assert!(p.children(doc.outline()).is_empty(), "{name}");
    }
}

#[test]
fn import_at_file_ends_the_body_with_the_newline_the_write_adds() {
    // Without it, the tree read back from the written file has one more byte.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("n.zzz").to_string_lossy().to_string();
    fs::write(&path, "no final newline").unwrap();
    let mut doc = outline_in(dir.path());
    let root = doc.outline().root_position().unwrap();
    let (p, _) = doc.import_at_file(&root, &path).unwrap();
    assert_eq!(p.b(doc.outline()), "no final newline\n");
}

#[test]
fn import_at_file_refusals_leave_no_trace_and_an_import_undoes() {
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("b.py").to_string_lossy().to_string();
    fs::write(&binary, [0xff, 0xfe, 0x00]).unwrap();
    let plain = dir.path().join("p.py").to_string_lossy().to_string();
    fs::write(&plain, "x = 1\n").unwrap();
    let mut doc = outline_in(dir.path());
    let root = doc.outline().root_position().unwrap();
    let count = doc.outline().all_positions().len();

    let err = doc.import_at_file(&root, &binary).unwrap_err();
    assert!(matches!(err, Error::NotUtf8 { .. }), "{err}");
    assert_eq!(doc.outline().all_positions().len(), count);
    assert!(!doc.outline().changed);
    assert!(!doc.undoer().can_undo());

    let (p, _) = doc.import_at_file(&root, &plain).unwrap();
    let err = doc.import_at_file(&root, &plain).unwrap_err();
    assert!(matches!(err, Error::Import { .. }), "{err}");
    assert!(err.to_string().contains("already in the outline"), "{err}");

    // Importing from inside an @file tree puts the new node beside it.
    let child = doc.outline_mut_untracked().insert_as_last_child(&p);
    let other = dir.path().join("q.py").to_string_lossy().to_string();
    fs::write(&other, "y = 2\n").unwrap();
    let (q, _) = doc.import_at_file(&child, &other).unwrap();
    assert_eq!(q.parent(doc.outline()), p.parent(doc.outline()));

    doc.undo();
    assert!(!doc.outline().position_exists(&q));
}

#[test]
fn an_auto_file_with_no_importer_is_read_whole_into_its_body() {
    let dir = tempfile::tempdir().unwrap();
    let mine = "[a]\nx = 1\n";
    let leo_path = outline_over_existing_file(dir.path(), "@auto a.toml", "a.toml", mine);

    let (mut o, report) = leolib::open_outline_with_report(&leo_path, true).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let root = o.root_position().unwrap();
    assert_eq!(root.b(&o), format!("@language toml\n{mine}"));
    assert!(!root.has_children(&o));

    let result = external::write_external_files(&mut o, false);
    assert_eq!(result.unchanged, 1, "{:?}", result.errors);
    o.set_body(&root, "@language toml\nedited\n");
    o.set_dirty(&root);
    let result = external::write_external_files(&mut o, false);
    assert_eq!(result.written.len(), 1, "{:?}", result.errors);
    let text = fs::read_to_string(dir.path().join("a.toml")).unwrap();
    assert_eq!(text, "edited\n");
}

#[test]
fn an_auto_rst_file_is_reported_and_never_overwritten() {
    // Leo splits it with an importer this port lacks, so it is not read whole.
    let dir = tempfile::tempdir().unwrap();
    let mine = "Title\n=====\n";
    let leo_path = outline_over_existing_file(dir.path(), "@auto notes.rst", "notes.rst", mine);

    let (mut o, report) = leolib::open_outline_with_report(&leo_path, true).unwrap();
    assert!(
        report
            .errors
            .iter()
            .any(|e| matches!(e.error, Error::Import { .. })),
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
fn an_at_jupytext_notebook_is_neither_read_nor_written() {
    let dir = tempfile::tempdir().unwrap();
    let mine = "{\"cells\": []}\n";
    let leo_path = outline_over_existing_file(dir.path(), "@jupytext nb.ipynb", "nb.ipynb", mine);

    let (mut o, report) = leolib::open_outline_with_report(&leo_path, true).unwrap();
    assert!(
        matches!(report.errors[..], [ref e] if matches!(e.error, Error::Unsupported { .. })),
        "{:?}",
        report.errors
    );
    let root = o.root_position().unwrap();
    o.set_body(&root, "x = 1\n");
    o.set_dirty(&root);
    // Approval does not help: the kind itself is refused.
    o.remember_read_path(&root, &dir.path().join("nb.ipynb").to_string_lossy());
    let result = external::write_external_files(&mut o, false);
    assert!(result.written.is_empty(), "{:?}", result.written);
    assert_eq!(
        fs::read_to_string(dir.path().join("nb.ipynb")).unwrap(),
        mine
    );
}

#[test]
fn an_auto_file_with_no_importer_that_would_not_write_back_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let mine = "a\n@others\n";
    let leo_path = outline_over_existing_file(dir.path(), "@auto notes.txt", "notes.txt", mine);

    let (mut o, report) = leolib::open_outline_with_report(&leo_path, true).unwrap();
    assert!(
        matches!(report.errors[..], [ref e] if matches!(e.error, Error::Import { .. })),
        "{:?}",
        report.errors
    );
    let root = o.root_position().unwrap();
    assert_eq!(root.b(&o), mine);
    o.set_dirty(&root);
    external::write_external_files(&mut o, false);
    assert_eq!(
        fs::read_to_string(dir.path().join("notes.txt")).unwrap(),
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

/// Rewriting a file keeps its permissions. The temporary file it is renamed
/// from used to take the default mode, so an executable script lost `+x`.
#[cfg(unix)]
#[test]
fn replacing_a_file_keeps_its_mode() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("run.sh");
    fs::write(&path, "echo 0\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    let p = path.to_string_lossy().to_string();
    assert!(external::replace_file(&p, "echo 1\n", false).unwrap());
    assert_eq!(fs::read_to_string(&path).unwrap(), "echo 1\n");
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755, "mode is {mode:o}");
    let names: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, vec!["run.sh"], "a temporary file was left behind");
}

#[test]
fn a_file_that_is_not_utf8_is_reported_unread_and_never_rewritten() {
    // Leo decodes with the file's own encoding and encodes with it again.
    // This port writes UTF-8 only, so it leaves such a file alone: writing it
    // would replace the latin-1 bytes with UTF-8 and lose every character the
    // two spell differently.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("latin.py");
    let disk = b"# @+leo-ver=5-thin-encoding=latin-1,.\n# @+node:x.1: * @file latin.py\nname = 'caf\xe9'\n# @-leo\n";
    fs::write(&file, disk).unwrap();

    let mut o = Outline::new_empty();
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@file latin.py");
    o.file_name = dir.path().join("test.leo").to_string_lossy().to_string();

    let report = external::read_external_files(&mut o);
    assert_eq!(report.read, 0);
    assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
    assert!(matches!(report.errors[0].error, Error::NotUtf8 { .. }));

    let result = external::write_external_files(&mut o, false);
    assert!(result.written.is_empty(), "{:?}", result.written);
    assert!(matches!(result.errors[0].error, Error::NotUtf8 { .. }));
    // Not offered for approval: approving it would write UTF-8 over the file.
    assert!(result.refused.is_empty());
    assert_eq!(fs::read(&file).unwrap(), disk);
}

#[test]
fn an_at_nosent_node_declaring_another_encoding_is_not_written() {
    // @nosent is never read, so once the user approves the overwrite the
    // write needs its own guard.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("nosent.py");
    fs::write(&file, "old = 1\n").unwrap();

    let mut o = Outline::new_empty();
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@nosent nosent.py");
    o.set_body(&root, "@encoding latin-1\nname = 'caf\u{e9}'\n");
    o.file_name = dir.path().join("test.leo").to_string_lossy().to_string();
    // The user approves overwriting the file the outline never read.
    o.remember_read_path(&root, &o.full_path(&root));

    let result = external::write_external_files(&mut o, false);
    assert!(result.written.is_empty(), "{:?}", result.written);
    assert_eq!(result.errors.len(), 1);
    let Error::UnsupportedEncoding { encoding } = &result.errors[0].error else {
        panic!("{:?}", result.errors[0].error);
    };
    assert_eq!(encoding, "latin-1");
    assert_eq!(fs::read_to_string(&file).unwrap(), "old = 1\n");
}

/// An outline with one `@file` node, written and reopened from `dir`.
fn written_at_file(dir: &std::path::Path) -> (Document, String) {
    let leo = dir.join("x.leo").to_string_lossy().to_string();
    let mut o = leolib::new_outline(&leo);
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@file x.py");
    o.set_body(&root, "x = 1\n");
    assert_eq!(
        external::write_external_files(&mut o, false).written.len(),
        1
    );
    leolib::save(&mut o, "").unwrap();
    let doc = Document::open(&leo, true).unwrap();
    (doc, dir.join("x.py").to_string_lossy().to_string())
}

#[test]
fn a_file_changed_on_disk_since_the_read_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let (mut doc, py) = written_at_file(dir.path());
    let theirs = fs::read_to_string(&py)
        .unwrap()
        .replace("x = 1", "x = 1  # theirs");
    fs::write(&py, &theirs).unwrap();
    assert!(doc.outline().changed_on_disk(&py));

    let root = doc.outline().root_position().unwrap();
    doc.set_body(&root, "x = 2\n");
    let result = doc.write_external_files(true);
    assert!(result.written.is_empty());
    assert_eq!(result.changed_on_disk.len(), 1);
    assert!(matches!(
        result.errors[0].error,
        Error::ChangedOnDisk { .. }
    ));
    assert_eq!(fs::read_to_string(&py).unwrap(), theirs);

    // Recording what is on disk now is the approval.
    doc.outline_mut_untracked()
        .record_file_stamp(&py, leolib::util::file_stamp(&py));
    let result = doc.write_external_files(true);
    assert_eq!(result.written.len(), 1, "{:?}", result.errors);
    assert!(fs::read_to_string(&py).unwrap().contains("x = 2"));
}

#[test]
fn read_files_takes_in_a_file_changed_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let (mut doc, py) = written_at_file(dir.path());
    let theirs = fs::read_to_string(&py).unwrap().replace("x = 1", "x = 3");
    fs::write(&py, theirs).unwrap();
    let root = doc.outline().root_position().unwrap();
    let result = doc.read_files(vec![root.clone()]);
    assert_eq!(result.read, 1, "{:?}", result.errors);
    assert_eq!(root.b(doc.outline()), "x = 3\n");
    assert!(!doc.outline().changed_on_disk(&py));
    assert!(!doc.undoer().can_undo());
}

#[test]
fn save_to_writes_a_copy_and_keeps_the_outline_where_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let (mut doc, _) = written_at_file(dir.path());
    let name = doc.outline().file_name.clone();
    let root = doc.outline().root_position().unwrap();
    doc.set_headline(&root, "@file x.py");
    doc.outline_mut_untracked().changed = true;
    let copy = dir.path().join("copy.leo").to_string_lossy().to_string();
    doc.save_to(&copy).unwrap();
    assert_eq!(doc.outline().file_name, name);
    assert!(doc.outline().changed);
    assert!(leolib::open_outline(&copy, false).is_ok());
}

#[test]
fn a_leo_file_changed_on_disk_is_noticed() {
    let dir = tempfile::tempdir().unwrap();
    let (doc, _) = written_at_file(dir.path());
    let leo = doc.outline().file_name.clone();
    assert!(!doc.outline().changed_on_disk(&leo));
    let mut text = fs::read_to_string(&leo).unwrap();
    text.push('\n');
    fs::write(&leo, text).unwrap();
    assert!(doc.outline().changed_on_disk(&leo));
}

#[test]
fn a_file_in_a_missing_directory_is_not_written_unless_the_config_allows() {
    let dir = tempfile::tempdir().unwrap();
    let leo = dir.path().join("x.leo").to_string_lossy().to_string();
    let mut o = leolib::new_outline(&leo);
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@file sub/x.py");
    o.set_body(&root, "x = 1\n");
    let result = external::write_external_files(&mut o, false);
    assert!(result.written.is_empty());
    assert!(matches!(result.errors[0].error, Error::NotFound { .. }));
    assert!(!dir.path().join("sub").exists());

    o.config.create_nonexistent_directories = true;
    let result = external::write_external_files(&mut o, false);
    assert_eq!(result.written.len(), 1, "{:?}", result.errors);
}

#[cfg(unix)]
#[test]
fn a_symlinked_file_is_written_through_the_link() {
    let dir = tempfile::tempdir().unwrap();
    let (mut doc, py) = written_at_file(dir.path());
    let target = dir.path().join("target.py");
    fs::rename(&py, &target).unwrap();
    std::os::unix::fs::symlink(&target, &py).unwrap();
    let root = doc.outline().root_position().unwrap();
    doc.set_body(&root, "x = 2\n");
    let result = doc.write_external_files(true);
    assert_eq!(result.written.len(), 1, "{:?}", result.errors);
    assert!(fs::symlink_metadata(&py).unwrap().file_type().is_symlink());
    assert!(fs::read_to_string(&target).unwrap().contains("x = 2"));
}

#[cfg(unix)]
#[test]
fn a_read_only_file_is_not_overwritten() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let (mut doc, py) = written_at_file(dir.path());
    let before = fs::read_to_string(&py).unwrap();
    fs::set_permissions(&py, fs::Permissions::from_mode(0o444)).unwrap();
    let root = doc.outline().root_position().unwrap();
    doc.set_body(&root, "x = 2\n");
    let result = doc.write_external_files(true);
    assert!(result.written.is_empty());
    assert_eq!(result.errors.len(), 1);
    assert_eq!(fs::read_to_string(&py).unwrap(), before);
}

#[test]
fn save_all_writes_the_outline_and_every_file_that_can_be_written() {
    let dir = tempfile::tempdir().unwrap();
    let leo = dir.path().join("s.leo").to_string_lossy().to_string();
    let mut o = leolib::new_outline(&leo);
    let bad = o.root_position().unwrap();
    o.set_headline(&bad, "@file bad.py");
    o.set_body(&bad, "x = 1\n");
    let orphan = o.insert_as_last_child(&bad);
    o.set_body(&orphan, "y = 2\n");
    let good = o.insert_after(&bad);
    o.set_headline(&good, "@file good.py");
    o.set_body(&good, "z = 3\n");

    let result = leolib::save_all(&mut o, "");
    assert!(result.leo.is_ok());
    assert_eq!(result.files.written.len(), 1);
    assert_eq!(result.files.errors.len(), 1);
    assert!(bad.is_dirty(&o));
    assert!(!good.is_dirty(&o));
    assert!(!dir.path().join("bad.py").exists());

    // A `.leo` file that cannot be written holds back every file.
    o.set_body(&bad, "x = 1\n@others\n");
    let missing = dir.path().join("no-such-dir/s.leo");
    let result = leolib::save_all(&mut o, &missing.to_string_lossy());
    assert!(result.leo.is_err());
    assert!(result.files.written.is_empty() && result.files.errors.is_empty());
    assert!(!dir.path().join("bad.py").exists());
    assert!(bad.is_dirty(&o));

    let result = leolib::save_all(&mut o, "");
    assert!(result.leo.is_ok());
    assert_eq!(result.files.written.len(), 1, "{:?}", result.files.errors);
    assert!(dir.path().join("bad.py").exists());
}

#[test]
fn an_at_clean_edit_in_the_same_second_as_the_write_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let leo_path = dir.path().join("test.leo").to_string_lossy().to_string();
    let py_path = dir.path().join("sample.py");

    let mut o = Outline::new_empty();
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@clean sample.py");
    o.set_body(&root, "@others\n");
    let a = o.insert_as_last_child(&root);
    o.set_headline(&a, "one");
    o.set_body(&a, "a = 1\n");
    o.file_name = leo_path.clone();

    let result = external::write_external_files(&mut o, false);
    assert_eq!(result.written.len(), 1, "{:?}", result.errors);

    // An outside edit whose mtime is in the same second as our write. The
    // read used to compare whole seconds and skip it.
    fs::write(&py_path, "a = 111\n").unwrap();
    let result = external::read_external_files(&mut o);
    assert!(result.errors.is_empty(), "{:?}", result.errors);

    let root = o.root_position().unwrap();
    assert_eq!(root.children(&o)[0].b(&o), "a = 111\n");
}

#[test]
fn an_encoding_this_port_cannot_write_is_refused_whatever_its_name() {
    // cp1252 was not in the list of names `is_valid_encoding` knew, so
    // `get_encoding` fell back to utf-8 and the write replaced the file's
    // bytes with UTF-8 without a word.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("nosent.py");
    let disk = b"name = 'caf\xe9'\n";
    fs::write(&file, disk).unwrap();

    let mut o = Outline::new_empty();
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@nosent nosent.py");
    o.set_body(&root, "@encoding cp1252\nname = 'caf\u{e9}'\n");
    o.file_name = dir.path().join("test.leo").to_string_lossy().to_string();
    // The user approves overwriting the file the outline never read.
    o.remember_read_path(&root, &o.full_path(&root));

    let result = external::write_external_files(&mut o, false);
    assert!(result.written.is_empty(), "{:?}", result.written);
    let Error::UnsupportedEncoding { encoding } = &result.errors[0].error else {
        panic!("{:?}", result.errors[0].error);
    };
    assert_eq!(encoding, "cp1252");
    assert_eq!(fs::read(&file).unwrap(), disk);
}

#[test]
fn an_at_file_written_with_an_encoding_alias_reads_back() {
    // The header is `-encoding=utf8,.`; the comma is not part of the name.
    let dir = tempfile::tempdir().unwrap();
    let leo = dir.path().join("test.leo").to_string_lossy().to_string();

    let mut o = leolib::new_outline(&leo);
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@file sample.py");
    o.set_body(&root, "@encoding utf8\nx = 1\n");
    assert_eq!(
        external::write_external_files(&mut o, false).written.len(),
        1
    );
    leolib::save(&mut o, "").unwrap();

    let o2 = leolib::open_outline(&leo, true).unwrap();
    let root = o2.root_position().unwrap();
    assert_eq!(root.b(&o2), "@encoding utf8\nx = 1\n");
}

#[test]
fn a_crlf_file_is_left_alone_when_its_text_has_not_changed() {
    // Leo's `compareIgnoringLineEndings`. A byte compare called every CRLF
    // file changed, so a write-all rewrote all of them as LF.
    let dir = tempfile::tempdir().unwrap();
    let leo = dir.path().join("test.leo").to_string_lossy().to_string();
    let py = dir.path().join("sample.py");

    let mut o = leolib::new_outline(&leo);
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@file sample.py");
    o.set_body(&root, "x = 1\n");
    assert_eq!(
        external::write_external_files(&mut o, false).written.len(),
        1
    );
    leolib::save(&mut o, "").unwrap();

    let crlf = fs::read_to_string(&py).unwrap().replace('\n', "\r\n");
    fs::write(&py, &crlf).unwrap();

    let mut o = leolib::open_outline(&leo, true).unwrap();
    let result = external::write_external_files(&mut o, false);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.written, Vec::<String>::new());
    assert_eq!(result.unchanged, 1);
    assert_eq!(fs::read_to_string(&py).unwrap(), crlf);
}

#[test]
fn an_explicit_line_ending_still_corrects_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let leo = dir.path().join("test.leo").to_string_lossy().to_string();
    let py = dir.path().join("sample.py");

    let mut o = leolib::new_outline(&leo);
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@file sample.py");
    o.set_body(&root, "@lineending crlf\nx = 1\n");
    assert_eq!(
        external::write_external_files(&mut o, false).written.len(),
        1
    );
    assert!(fs::read_to_string(&py).unwrap().contains("\r\n"));

    fs::write(&py, fs::read_to_string(&py).unwrap().replace("\r\n", "\n")).unwrap();
    o.record_file_stamp(py.to_string_lossy().as_ref(), None);
    let result = external::write_external_files(&mut o, false);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
    assert_eq!(result.written.len(), 1);
    assert!(fs::read_to_string(&py).unwrap().contains("\r\n"));
}

#[test]
fn an_at_auto_tree_with_nothing_in_it_does_not_truncate_its_file() {
    let dir = tempfile::tempdir().unwrap();
    let py = dir.path().join("sample.py");
    fs::write(&py, "x = 1\ny = 2\n").unwrap();

    let mut o = Outline::new_empty();
    o.file_name = dir.path().join("test.leo").to_string_lossy().to_string();
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@auto sample.py");
    external::read_file_at_position(&mut o, &root).unwrap();

    // The tree the import produced, emptied: what an importer that returns
    // nothing would leave behind.
    let root = o.root_position().unwrap();
    o.delete_all_children(root.v);
    o.set_body(&root, "");

    let result = external::write_external_files(&mut o, false);
    assert!(result.written.is_empty(), "{:?}", result.written);
    assert!(
        matches!(result.errors[0].error, Error::Write { .. }),
        "{:?}",
        result.errors[0].error
    );
    assert_eq!(fs::read_to_string(&py).unwrap(), "x = 1\ny = 2\n");
}

#[test]
fn an_empty_file_leaves_an_at_edit_node_alone() {
    // Leo's #391. The read used to replace the body with the @language line,
    // which for @edit is the only copy of the text once the file is empty.
    let dir = tempfile::tempdir().unwrap();
    let txt = dir.path().join("notes.txt");
    fs::write(&txt, "").unwrap();

    let mut o = Outline::new_empty();
    o.file_name = dir.path().join("test.leo").to_string_lossy().to_string();
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@edit notes.txt");
    o.set_body(&root, "@nocolor\nwork not yet written\n");

    assert!(!external::read_file_at_position(&mut o, &root).unwrap());
    assert_eq!(root.b(&o), "@nocolor\nwork not yet written\n");
}

#[test]
fn an_at_edit_node_with_children_is_not_written() {
    // Only the node's own body reaches the file, so the children's text
    // would be dropped. Leo refuses the write for the same reason.
    let dir = tempfile::tempdir().unwrap();
    let txt = dir.path().join("notes.txt");
    fs::write(&txt, "first\n").unwrap();

    let mut o = Outline::new_empty();
    o.file_name = dir.path().join("test.leo").to_string_lossy().to_string();
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@edit notes.txt");
    external::read_file_at_position(&mut o, &root).unwrap();
    let root = o.root_position().unwrap();
    let child = o.insert_as_last_child(&root);
    o.set_body(&child, "second\n");

    let result = external::write_external_files(&mut o, false);
    assert!(result.written.is_empty(), "{:?}", result.written);
    assert!(
        matches!(result.errors[0].error, Error::Write { .. }),
        "{:?}",
        result.errors[0].error
    );
    assert_eq!(fs::read_to_string(&txt).unwrap(), "first\n");
}

#[test]
fn reading_an_at_auto_file_keeps_the_descendent_ua_blob() {
    // The importer builds the subtree the blob already describes, so the
    // read itself must not count as a restructuring.
    let dir = tempfile::tempdir().unwrap();
    let leo = dir.path().join("test.leo");
    fs::write(dir.path().join("x.py"), "a = 1\n\n\ndef f():\n    pass\n").unwrap();
    fs::write(
        &leo,
        "<?xml version=\"1.0\"?>\n<leo_file>\n<leo_header file_format=\"2\"/>\n<vnodes>\n\
         <v t=\"a.1\" descendentVnodeUnknownAttributes=\"80049501\"><vh>@auto x.py</vh></v>\n\
         </vnodes>\n<tnodes>\n</tnodes>\n</leo_file>\n",
    )
    .unwrap();

    let mut o = leolib::open_outline(&leo.to_string_lossy(), true).unwrap();
    assert!(!o.root_position().unwrap().children(&o).is_empty());
    assert!(leolib::to_xml(&mut o).contains("descendentVnodeUnknownAttributes="));
}

/// A file with another name for it is written in place. The rename that makes
/// a write atomic replaces the inode, which would leave the other name on the
/// old contents.
#[cfg(unix)]
#[test]
fn writing_a_hard_linked_file_keeps_the_link() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("one.txt");
    let other = dir.path().join("two.txt");
    fs::write(&path, "old\n").unwrap();
    fs::hard_link(&path, &other).unwrap();

    let p = path.to_string_lossy().to_string();
    assert!(external::replace_file(&p, "new\n", false).unwrap());
    assert_eq!(fs::read_to_string(&other).unwrap(), "new\n");
    assert_eq!(fs::read_to_string(&path).unwrap(), "new\n");
}

#[test]
fn a_line_the_reader_does_not_understand_is_reported() {
    // The sentinel scanner keeps such a line, so the text is not lost, but
    // its warning was collected and dropped.
    let dir = tempfile::tempdir().unwrap();
    let leo = dir.path().join("test.leo").to_string_lossy().to_string();
    let py = dir.path().join("sample.py");

    let mut o = leolib::new_outline(&leo);
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@file sample.py");
    o.set_body(&root, "x = 1\n");
    assert_eq!(
        external::write_external_files(&mut o, false).written.len(),
        1
    );
    leolib::save(&mut o, "").unwrap();

    let text = fs::read_to_string(&py).unwrap();
    fs::write(&py, text.replace("x = 1\n", "# @+nonsense\nx = 1\n")).unwrap();

    let (o, report) = leolib::open_outline_with_report(&leo, true).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
    assert!(
        report.warnings[0].message.contains("unexpected line"),
        "{:?}",
        report.warnings[0]
    );
    assert!(o.root_position().unwrap().b(&o).contains("# @+nonsense"));
}

#[test]
fn a_save_names_the_tree_whose_descendent_uas_it_dropped() {
    // The blob is keyed by archived position, and only Leo can rebuild it, so
    // a restructured subtree loses it. The save is the one place that can say
    // so: nothing in the outline records it afterwards.
    let dir = tempfile::tempdir().unwrap();
    let leo = dir.path().join("test.leo");
    fs::write(dir.path().join("x.py"), "a = 1\n\n\ndef f():\n    pass\n").unwrap();
    fs::write(
        &leo,
        "<?xml version=\"1.0\"?>\n<leo_file>\n<leo_header file_format=\"2\"/>\n<vnodes>\n\
         <v t=\"a.1\" descendentVnodeUnknownAttributes=\"80049501\"><vh>@auto x.py</vh></v>\n\
         </vnodes>\n<tnodes>\n</tnodes>\n</leo_file>\n",
    )
    .unwrap();

    let mut doc = Document::open(&leo.to_string_lossy(), true).unwrap();
    let root = doc.outline().root_position().unwrap();
    let kept = doc.save_all("");
    assert!(kept.leo.is_ok(), "{:?}", kept.leo);
    assert_eq!(kept.dropped_descendent_uas, Vec::<String>::new());
    assert!(fs::read_to_string(&leo)
        .unwrap()
        .contains("descendentVnodeUnknownAttributes="));

    let child = root.children(doc.outline())[0].clone();
    doc.insert_node_before(&child);
    let dropped = doc.save_all("");
    assert!(dropped.leo.is_ok(), "{:?}", dropped.leo);
    assert_eq!(dropped.dropped_descendent_uas, vec!["@auto x.py"]);
    assert!(!fs::read_to_string(&leo)
        .unwrap()
        .contains("descendentVnodeUnknownAttributes="));

    // Said once: the blob is already gone.
    assert_eq!(
        doc.save_all("").dropped_descendent_uas,
        Vec::<String>::new()
    );
}

/// `{'0.0': {'__bookmarks': {'is_dupe': False}}, '0.1': {'str_note': 'keep me'}}`,
/// pickled and hexlified as Leo writes it.
const BLOB: &str = "7d7100285803000000302e3071017d7102580b0000005f5f626f6f6b6d61726b737103\
7d7104580700000069735f6475706571054930300a73735803000000302e3171067d710758080000007374725f6e6f\
7465710858070000006b656570206d65710973752e";

/// An `@auto x.py` outline whose blob holds the uAs of two imported nodes.
fn outline_with_imported_uas(dir: &std::path::Path, blob: &str) -> String {
    let leo = dir.join("test.leo").to_string_lossy().to_string();
    fs::write(
        dir.join("x.py"),
        "a = 1\n\n\ndef f():\n    pass\n\n\ndef g():\n    pass\n",
    )
    .unwrap();
    fs::write(
        &leo,
        format!(
            "<?xml version=\"1.0\"?>\n<leo_file>\n<leo_header file_format=\"2\"/>\n<vnodes>\n\
             <v t=\"a.1\" descendentVnodeUnknownAttributes=\"{blob}\"><vh>@auto x.py</vh></v>\n\
             </vnodes>\n<tnodes>\n</tnodes>\n</leo_file>\n"
        ),
    )
    .unwrap();
    leo
}

#[test]
fn a_restructured_at_auto_tree_keeps_its_descendants_unknown_attributes() {
    // The blob names each node by its position under the `@auto` node, so it
    // is rebuilt from the tree on every save. It used to be written back as
    // it was read, which named other nodes after a move, and was then dropped
    // rather than let Leo restore the uAs onto them.
    let dir = tempfile::tempdir().unwrap();
    let leo = outline_with_imported_uas(dir.path(), BLOB);

    let mut doc = Document::open(&leo, true).unwrap();
    let root = doc.outline().root_position().unwrap();
    let kids = root.children(doc.outline());
    assert_eq!(kids.len(), 2, "{:?}", kids.len());
    // The read gave the uAs to the nodes the importer built.
    assert!(doc
        .outline()
        .node(kids[0].v)
        .uas
        .contains_key("__bookmarks"));
    assert_eq!(
        doc.outline().node(kids[1].v).uas["str_note"].as_file_text(),
        "keep me"
    );

    // Move the second node to the front: every position in the blob shifts,
    // and the write puts the tree's new order in the file.
    doc.move_up(&kids[1]);
    let result = doc.save_all("");
    assert!(result.leo.is_ok(), "{:?}", result.leo);
    assert_eq!(result.files.written.len(), 1, "{:?}", result.files.errors);
    assert_eq!(result.dropped_descendent_uas, Vec::<String>::new());

    let doc = Document::open(&leo, true).unwrap();
    let kids = doc
        .outline()
        .root_position()
        .unwrap()
        .children(doc.outline());
    assert_eq!(kids[0].h(doc.outline()), "function: g");
    assert_eq!(
        doc.outline().node(kids[0].v).uas["str_note"].as_file_text(),
        "keep me"
    );
    assert!(doc
        .outline()
        .node(kids[1].v)
        .uas
        .contains_key("__bookmarks"));
}

#[test]
fn a_blob_this_port_cannot_read_is_still_written_back_unchanged() {
    // Protocol 4, which `pickle.rs` does not read. It stays where it is, and
    // a structural change drops it rather than let it name other nodes.
    let dir = tempfile::tempdir().unwrap();
    let leo = outline_with_imported_uas(dir.path(), "80049501");

    let mut doc = Document::open(&leo, true).unwrap();
    assert!(doc.save_all("").leo.is_ok());
    assert!(fs::read_to_string(&leo).unwrap().contains("\"80049501\""));

    let kids = doc
        .outline()
        .root_position()
        .unwrap()
        .children(doc.outline());
    doc.move_up(&kids[1]);
    let result = doc.save_all("");
    assert_eq!(result.dropped_descendent_uas, vec!["@auto x.py"]);
    assert!(!fs::read_to_string(&leo).unwrap().contains("\"80049501\""));
}

#[test]
fn a_move_inside_an_at_file_tree_reaches_the_file() {
    // `move_to` set the dirty bit on the node it moved, where the write asks
    // the `@<file>` node above it. Nothing was written, and since an `@file`
    // tree lives in its file and not in the `.leo` file, the move was gone on
    // the next read.
    let dir = tempfile::tempdir().unwrap();
    let leo = dir.path().join("test.leo").to_string_lossy().to_string();

    let mut doc = leolib::Document::new_empty(&leo);
    let root = doc.outline().root_position().unwrap();
    doc.set_headline(&root, "@file sample.py");
    doc.set_body(&root, "@others\n");
    for (headline, body) in [("f", "def f():\n    pass\n"), ("g", "def g():\n    pass\n")] {
        let child = doc.insert_child(&root);
        doc.set_headline(&child, headline);
        doc.set_body(&child, body);
    }
    assert_eq!(doc.save_all("").files.written.len(), 1);

    let kids = doc
        .outline()
        .root_position()
        .unwrap()
        .children(doc.outline());
    let before: Vec<String> = kids
        .iter()
        .map(|p| p.h(doc.outline()).to_string())
        .collect();
    doc.move_up(&kids[1]);
    let result = doc.save_all("");
    assert_eq!(result.files.written.len(), 1, "{:?}", result.files.errors);

    let doc = Document::open(&leo, true).unwrap();
    let heads: Vec<String> = doc
        .outline()
        .root_position()
        .unwrap()
        .children(doc.outline())
        .iter()
        .map(|p| p.h(doc.outline()).to_string())
        .collect();
    assert_eq!(heads, vec![before[1].clone(), before[0].clone()]);
}

/// An outline whose example node is cloned into `@clean README.md` and
/// `@file tests/test_readme.py`, with both files written. Returns the
/// `.leo` path.
fn example_in_two_files(dir: &std::path::Path) -> String {
    fs::create_dir_all(dir.join("tests")).unwrap();
    let leo_path = dir.join("doc.leo").to_string_lossy().to_string();
    let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<leo_file xmlns:leo="http://leoeditor.com/namespaces/leo-python-editor/1.1" >
<leo_header file_format="2"/>
<vnodes>
<v t="a.1"><vh>@clean README.md</vh>
<v t="a.2"><vh>&lt;&lt; example &gt;&gt;</vh></v>
</v>
<v t="a.3"><vh>@file tests/test_readme.py</vh>
<v t="a.2"></v>
</v>
</vnodes>
<tnodes>
<t tx="a.1">@language md
Adding:

```python
&lt;&lt; example &gt;&gt;
```
</t>
<t tx="a.2">assert 2 + 3 == 5
</t>
<t tx="a.3">@language python
def test_example():
    &lt;&lt; example &gt;&gt;
</t>
</tnodes>
</leo_file>
"#;
    fs::write(&leo_path, xml).unwrap();
    let mut o = leolib::open_outline(&leo_path, false).unwrap();
    let result = external::write_external_files(&mut o, false);
    assert_eq!(result.written.len(), 2, "{:?}", result.errors);
    leo_path
}

#[test]
fn a_clone_two_files_disagree_on_keeps_both_texts_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let leo_path = example_in_two_files(dir.path());
    // The README's example is edited outside the outline; the test file still
    // holds the old text, and is read after it.
    let readme = dir.path().join("README.md");
    let text = fs::read_to_string(&readme).unwrap();
    let edited = text.replace(
        "assert 2 + 3 == 5\n",
        "assert 2 + 3 == 5\nassert 1 + 1 == 2\n",
    );
    assert_ne!(text, edited);
    fs::write(&readme, &edited).unwrap();

    let (o, report) = leolib::open_outline_with_report(&leo_path, true).unwrap();
    assert_eq!(report.conflicts.len(), 1, "{:?}", report.conflicts);
    let c = &report.conflicts[0];
    assert_eq!(c.gnx, "a.2");
    assert!(c.old.path.ends_with("README.md"), "{}", c.old.path);
    assert_eq!(c.old.body, "assert 2 + 3 == 5\nassert 1 + 1 == 2\n");
    assert!(c.new.path.ends_with("test_readme.py"), "{}", c.new.path);
    assert_eq!(c.new.body, "assert 2 + 3 == 5\n");

    // The node keeps the later file's text, as Leo does; the edit survives
    // under Recovered Nodes.
    let heads: Vec<(String, String)> = o
        .all_positions()
        .iter()
        .map(|p| (p.h(&o).to_string(), p.b(&o).to_string()))
        .collect();
    let recovered = heads.iter().position(|(h, _)| h == "Recovered Nodes");
    let recovered = recovered.expect("a Recovered Nodes node");
    let rest = &heads[recovered + 1..];
    assert_eq!(
        rest[0].0,
        "Recovered node \"<< example >>\" from test_readme.py"
    );
    assert!(
        rest[0].1.contains("+ assert 2 + 3 == 5") || rest[0].1.contains("  assert 2 + 3 == 5"),
        "{}",
        rest[0].1
    );
    assert!(rest[0].1.contains("- assert 1 + 1 == 2"), "{}", rest[0].1);
    assert_eq!(
        rest[1],
        ("old:<< example >>".to_string(), c.old.body.clone())
    );
    assert_eq!(
        rest[2],
        ("new:<< example >>".to_string(), c.new.body.clone())
    );
    assert!(o.changed, "the recovered nodes need saving");
}

#[test]
fn a_clone_both_files_agree_on_is_no_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let leo_path = example_in_two_files(dir.path());
    let (o, report) = leolib::open_outline_with_report(&leo_path, true).unwrap();
    assert!(report.conflicts.is_empty(), "{:?}", report.conflicts);
    assert!(o
        .all_positions()
        .iter()
        .all(|p| p.h(&o) != "Recovered Nodes"));
}
