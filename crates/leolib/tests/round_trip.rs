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
    let root = doc.outline.root_position().unwrap();

    let (p, needs_write) = doc.import_at_file(&root, &path).unwrap();
    assert!(needs_write);
    assert_eq!(p.h(&doc.outline), "@file x.py");
    assert!(
        p.b(&doc.outline)
            .starts_with("@first #!/usr/bin/env python3\n"),
        "{}",
        p.b(&doc.outline)
    );
    assert_eq!(p.children(&doc.outline).len(), 2);

    // The sentinels wait for the caller's approval.
    let result = doc.write_external_files(true);
    assert_eq!(result.refused, vec![p.clone()]);
    assert_eq!(fs::read_to_string(&path).unwrap(), text);

    doc.outline.remember_read_path(&p, &path);
    assert_eq!(doc.write_files(vec![p.clone()]).written, vec![path.clone()]);
    let written = fs::read_to_string(&path).unwrap();
    assert!(
        written.starts_with("#!/usr/bin/env python3\n# @+leo-ver=5-thin\n"),
        "{written}"
    );

    // Reading the written file gives back the imported tree.
    let before = bodies(&doc.outline, &p);
    external::read_file_at_position(&mut doc.outline, &p).unwrap();
    assert_eq!(bodies(&doc.outline, &p), before);
}

#[test]
fn import_at_file_reads_a_file_that_has_sentinels() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("y.py").to_string_lossy().to_string();
    let mut src = outline_in(dir.path());
    let root = src.outline.root_position().unwrap();
    src.outline.set_headline(&root, "@file y.py");
    src.outline.set_body(&root, "@others\n");
    let child = src.outline.insert_as_last_child(&root);
    src.outline.set_headline(&child, "f");
    src.outline.set_body(&child, "def f():\n    return 1\n");
    assert_eq!(src.write_external_files(false).written, vec![path.clone()]);

    let mut doc = outline_in(dir.path());
    let at = doc.outline.root_position().unwrap();
    let (p, needs_write) = doc.import_at_file(&at, &path).unwrap();
    assert!(!needs_write);
    let kids = p.children(&doc.outline);
    assert_eq!(kids.len(), 1);
    assert_eq!(kids[0].h(&doc.outline), "f");
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
        let root = doc.outline.root_position().unwrap();
        let (p, _) = doc.import_at_file(&root, &path).unwrap();
        assert_eq!(p.b(&doc.outline), text, "{name}");
        assert!(p.children(&doc.outline).is_empty(), "{name}");
    }
}

#[test]
fn import_at_file_ends_the_body_with_the_newline_the_write_adds() {
    // Without it, the tree read back from the written file has one more byte.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("n.zzz").to_string_lossy().to_string();
    fs::write(&path, "no final newline").unwrap();
    let mut doc = outline_in(dir.path());
    let root = doc.outline.root_position().unwrap();
    let (p, _) = doc.import_at_file(&root, &path).unwrap();
    assert_eq!(p.b(&doc.outline), "no final newline\n");
}

#[test]
fn import_at_file_refusals_leave_no_trace_and_an_import_undoes() {
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("b.py").to_string_lossy().to_string();
    fs::write(&binary, [0xff, 0xfe, 0x00]).unwrap();
    let plain = dir.path().join("p.py").to_string_lossy().to_string();
    fs::write(&plain, "x = 1\n").unwrap();
    let mut doc = outline_in(dir.path());
    let root = doc.outline.root_position().unwrap();
    let count = doc.outline.all_positions().len();

    let err = doc.import_at_file(&root, &binary).unwrap_err();
    assert!(matches!(err, Error::NotUtf8 { .. }), "{err}");
    assert_eq!(doc.outline.all_positions().len(), count);
    assert!(!doc.outline.changed);
    assert!(!doc.undoer.can_undo());

    let (p, _) = doc.import_at_file(&root, &plain).unwrap();
    let err = doc.import_at_file(&root, &plain).unwrap_err();
    assert!(matches!(err, Error::Import { .. }), "{err}");
    assert!(err.to_string().contains("already in the outline"), "{err}");

    // Importing from inside an @file tree puts the new node beside it.
    let child = doc.outline.insert_as_last_child(&p);
    let other = dir.path().join("q.py").to_string_lossy().to_string();
    fs::write(&other, "y = 2\n").unwrap();
    let (q, _) = doc.import_at_file(&child, &other).unwrap();
    assert_eq!(q.parent(&doc.outline), p.parent(&doc.outline));

    doc.undo();
    assert!(!doc.outline.position_exists(&q));
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
    assert!(external::replace_file(&p, "echo 1\n", "utf-8").unwrap());
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
    // @nosent is never read and is exempt from `may_overwrite`, so the write
    // needs its own guard. @clean takes the same path.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("nosent.py");
    fs::write(&file, "old = 1\n").unwrap();

    let mut o = Outline::new_empty();
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@nosent nosent.py");
    o.set_body(&root, "@encoding latin-1\nname = 'caf\u{e9}'\n");
    o.file_name = dir.path().join("test.leo").to_string_lossy().to_string();

    let result = external::write_external_files(&mut o, false);
    assert!(result.written.is_empty(), "{:?}", result.written);
    assert_eq!(result.errors.len(), 1);
    let Error::UnsupportedEncoding { encoding } = &result.errors[0].error else {
        panic!("{:?}", result.errors[0].error);
    };
    assert_eq!(encoding, "latin-1");
    assert_eq!(fs::read_to_string(&file).unwrap(), "old = 1\n");
}
