//! `leolib::ext`: kinds beyond Leo's are read only when registered.

use std::fs;
use std::path::Path;

use leolib::ext::{FileKind, Kinds};
use leolib::{external, Outline, Position};

/// A `.leo` file with an `@entangled` and a `@qmd` node, each with a
/// child and a body as Leo would store an ordinary node, and their files
/// beside it.
fn outline(dir: &Path) -> String {
    fs::write(dir.join("doc.md"), "# From the file\n").unwrap();
    fs::write(dir.join("r.qmd"), "# From the file\n").unwrap();
    let leo = dir.join("x.leo").to_string_lossy().to_string();
    fs::write(
        &leo,
        r#"<?xml version="1.0" encoding="utf-8"?>
<leo_file>
<leo_header file_format="2"/>
<vnodes>
<v t="a.1"><vh>@entangled doc.md</vh>
<v t="a.2"><vh>stored child</vh></v>
</v>
<v t="b.1"><vh>@qmd r.qmd</vh>
<v t="b.2"><vh>stored child</vh></v>
</v>
</vnodes>
<tnodes>
<t tx="a.1">stored body</t>
<t tx="a.2">child body</t>
<t tx="b.1">stored body</t>
<t tx="b.2">child body</t>
</tnodes>
</leo_file>
"#,
    )
    .unwrap();
    leo
}

fn shape(o: &Outline) -> Vec<String> {
    o.all_positions()
        .iter()
        .map(|p: &Position| format!("{}{}", "  ".repeat(p.level()), p.h(o)))
        .collect()
}

#[test]
fn with_no_kinds_registered_they_are_ordinary_nodes_as_in_leo() {
    let dir = tempfile::tempdir().unwrap();
    let leo = outline(dir.path());
    let (mut o, report) = leolib::open_outline_with_kinds(&leo, true, Kinds::empty()).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.read, 0, "no file is read");
    assert_eq!(
        shape(&o),
        [
            "@entangled doc.md",
            "  stored child",
            "@qmd r.qmd",
            "  stored child"
        ]
    );
    let root = o.root_position().unwrap();
    assert_eq!(root.b(&o), "stored body");
    assert!(!root.is_any_at_file_node(&o));

    // Saved as Leo saves any node: children and bodies in the .leo file.
    leolib::save(&mut o, &leo).unwrap();
    let xml = fs::read_to_string(&leo).unwrap();
    assert!(xml.contains("<vh>stored child</vh>"), "{xml}");
    assert!(xml.contains(">stored body</t>"), "{xml}");
    assert!(!xml.contains("leo-rs-"), "{xml}");
    // And the files are never written.
    let written = external::write_external_files(&mut o, true);
    assert!(written.written.is_empty(), "{:?}", written.written);
    assert_eq!(
        fs::read_to_string(dir.path().join("doc.md")).unwrap(),
        "# From the file\n"
    );
}

/// A kind that reads its file into the node's body and writes the body back,
/// and saves nothing in the `.leo` file.
struct Whole(&'static str);

impl FileKind for Whole {
    fn directive(&self) -> &'static str {
        self.0
    }
    fn read(&self, o: &mut Outline, p: &Position) -> leolib::Result<bool> {
        let path = o.full_path(p);
        let text = external::read_file_to_string(&path)?;
        o.detach_subtree_keeping_clones(p.v);
        o.node_mut(p.v).b = text;
        o.remember_read_path(p, &path);
        Ok(true)
    }
    fn write(&self, o: &Outline, p: &Position) -> leolib::Result<String> {
        Ok(p.b(o).to_string())
    }
    fn stores_body(&self) -> bool {
        false
    }
}

#[test]
fn a_registered_kind_is_read_written_and_saved_through_the_registry() {
    let dir = tempfile::tempdir().unwrap();
    let leo = outline(dir.path());
    let kinds = Kinds::empty().with(Whole("@entangled")).unwrap();
    let (mut o, report) = leolib::open_outline_with_kinds(&leo, true, kinds).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(report.read, 1, "only the registered kind is read");
    assert_eq!(
        shape(&o),
        ["@entangled doc.md", "@qmd r.qmd", "  stored child"]
    );
    let root = o.root_position().unwrap();
    assert_eq!(root.b(&o), "# From the file\n");
    assert!(root.is_any_at_file_node(&o));

    // Its file is written from the tree; the .leo file keeps no body for it.
    o.set_body(&root, "# Edited\n");
    let written = external::write_external_files(&mut o, true);
    assert_eq!(written.written.len(), 1, "{:?}", written.errors);
    assert_eq!(
        fs::read_to_string(dir.path().join("doc.md")).unwrap(),
        "# Edited\n"
    );
    leolib::save(&mut o, &leo).unwrap();
    let xml = fs::read_to_string(&leo).unwrap();
    assert!(!xml.contains("Edited"), "{xml}");
    assert!(xml.contains("<vh>@entangled doc.md</vh></v>"), "{xml}");
}

struct Kind(&'static str);

impl FileKind for Kind {
    fn directive(&self) -> &'static str {
        self.0
    }
    fn read(&self, _: &mut Outline, _: &Position) -> leolib::Result<bool> {
        Ok(false)
    }
    fn write(&self, _: &Outline, _: &Position) -> leolib::Result<String> {
        Ok(String::new())
    }
}

#[test]
fn a_kind_can_add_to_leo_but_not_replace_it() {
    for leo in ["@clean", "@auto", "@auto-md", "@file", "@edit", "@leo"] {
        let why = Kinds::empty().with(Kind(leo)).unwrap_err();
        assert!(why.contains("Leo's"), "{leo}: {why}");
    }
    assert!(Kinds::empty().with(Kind("qmd")).is_err(), "not a directive");
    let twice = Kinds::empty()
        .with(Kind("@qmd"))
        .unwrap()
        .with(Kind("@qmd"));
    assert!(twice.unwrap_err().contains("twice"));
    let kinds = Kinds::empty().with(Kind("@qmd")).unwrap();
    let (kind, name) = kinds.find("@qmd report.qmd").unwrap();
    assert_eq!((kind.directive(), name.as_str()), ("@qmd", "report.qmd"));
    assert!(kinds.find("@qmdx report.qmd").is_none());
}
