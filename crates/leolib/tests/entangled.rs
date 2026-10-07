//! `@entangled`: markdown whose named code fences are nodes.
//!
//! `data/entangled/` holds entangled-rs's example documents
//! (github.com/shakfu/entangled-rs, `examples/`, MIT), copied unchanged.

use std::fs;
use std::path::Path;

use leolib::{external, Error, Outline, Position};

/// A `.leo` file holding one `@entangled` node over `name`, written with
/// `contents` beside it. Returns the `.leo` path.
fn outline_over(dir: &Path, name: &str, contents: &str) -> String {
    fs::write(dir.join(name), contents).unwrap();
    let leo = dir.join("doc.leo").to_string_lossy().to_string();
    let xml = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<leo_file xmlns:leo="http://leoeditor.com/namespaces/leo-python-editor/1.1" >
<leo_header file_format="2"/>
<vnodes>
<v t="a.1"><vh>@entangled {name}</vh></v>
</vnodes>
<tnodes>
<t tx="a.1"></t>
</tnodes>
</leo_file>
"#
    );
    fs::write(&leo, xml).unwrap();
    leo
}

/// Open `contents` as an `@entangled` file; fails the test on a read error.
fn open(dir: &Path, name: &str, contents: &str) -> Outline {
    let leo = outline_over(dir, name, contents);
    let (o, report) = leolib::open_outline_with_report(&leo, true).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    o
}

fn find(o: &Outline, headline: &str) -> Position {
    o.all_positions()
        .into_iter()
        .find(|p| p.h(o) == headline)
        .unwrap_or_else(|| panic!("no node {headline:?}"))
}

fn root(o: &Outline) -> Position {
    o.root_position().unwrap()
}

/// What the tree writes, as leolib would write it to disk.
fn written(o: &Outline) -> String {
    leolib::entangled::write_string(o, &root(o)).unwrap()
}

#[test]
fn every_example_document_is_written_back_byte_for_byte() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/entangled");
    let mut seen = 0;
    for entry in fs::read_dir(&data).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let contents = fs::read_to_string(&path).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut o = open(dir.path(), &name, &contents);
        assert_eq!(written(&o), contents, "{name}");
        let result = external::write_external_files(&mut o, false);
        assert!(result.errors.is_empty(), "{name}: {:?}", result.errors);
        assert_eq!(result.unchanged, 1, "{name}: {:?}", result.written);
        seen += 1;
    }
    assert_eq!(seen, 5);
}

#[test]
fn headings_and_named_fences_are_nodes_and_the_rest_is_text() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/entangled/hello.md");
    let dir = tempfile::tempdir().unwrap();
    let o = open(dir.path(), "hello.md", &fs::read_to_string(data).unwrap());
    let heads: Vec<String> = o
        .all_positions()
        .iter()
        .map(|p| format!("{}{}", "  ".repeat(p.level()), p.h(&o)))
        .collect();
    assert_eq!(
        heads,
        [
            "@entangled hello.md",
            "  Hello World",
            "    The program",
            "      << main >>",
            "    Running",
        ]
    );
    let program = find(&o, "The program");
    assert_eq!(
        program.b(&o),
        "\nThis program prints a greeting to the console.\n\n\
         ```python #main file=hello.py\n<< main >>\n```\n\n"
    );
    assert_eq!(
        find(&o, "<< main >>").b(&o),
        "print(\"Hello from entangled!\")\n"
    );
    // The unnamed `sh` fence stays in its heading's body.
    assert!(find(&o, "Running")
        .b(&o)
        .contains("```sh\nentangled tangle\n"));
}

#[test]
fn a_fence_node_is_in_its_fences_language() {
    let dir = tempfile::tempdir().unwrap();
    let doc = "# A\n\n```py #a\nx = 1\n```\n\n```text {.rust #b}\nfn b() {}\n```\n\n\
               ```{cpp, label=c}\nint c;\n```\n";
    let o = open(dir.path(), "doc.md", doc);
    assert_eq!(
        o.language_at(&find(&o, "<< a >>")).as_deref(),
        Some("python")
    );
    assert_eq!(o.language_at(&find(&o, "<< b >>")).as_deref(), Some("rust"));
    assert_eq!(
        o.language_at(&find(&o, "<< c >>")).as_deref(),
        Some("cplusplus")
    );
    assert_eq!(o.get_language(&find(&o, "A")), "md");
}

#[test]
fn a_fence_is_named_as_entangled_names_it_in_each_style() {
    let dir = tempfile::tempdir().unwrap();
    let doc = "```python file=out.py\na\n```\n\n\
               ```{python, label=knit, file=k.py}\nb\n```\n\n\
               ```{python}\n#| label: quarto\nc\n```\n\n\
               ```{python}\n#| file: q.py\nd\n```\n\n\
               ```python\nunnamed\n```\n";
    let o = open(dir.path(), "doc.md", doc);
    let fences: Vec<String> = root(&o)
        .children(&o)
        .iter()
        .map(|p| p.h(&o).to_string())
        .collect();
    assert_eq!(
        fences,
        ["<< out.py >>", "<< knit >>", "<< quarto >>", "<< q.py >>"]
    );
    // Quarto's options stay in the code, as the file has them.
    assert_eq!(find(&o, "<< quarto >>").b(&o), "#| label: quarto\nc\n");
    assert!(root(&o).b(&o).contains("```python\nunnamed\n```\n"));
    assert_eq!(written(&o), doc);
}

#[test]
fn awkward_markdown_round_trips() {
    let doc = "---\ntitle: x\nlist:\n---\n\n\
               Title\n=====\n\n\
               Subtitle\n--------\n\n\
               #hashtag is not a heading, nor is ####### seven.\n\n\
               A paragraph mentions\n<< add >>\non its own line.\n\n\
               ### Skips a level ###\n\n\
               ~~~python #tilde\nprint(1)\n~~~\n\n\
               ````markdown #nested\n```python\ninner\n```\n````\n\n\
               - a list item:\n\n  ```python #indented\n  def f():\n      return 1\n\n  ```\n\n\
               - item\n---\n\n\
               ```python #open\nno close, no final newline";
    let dir = tempfile::tempdir().unwrap();
    let o = open(dir.path(), "doc.md", doc);
    assert_eq!(written(&o), doc);
    let heads: Vec<&str> = ["Title", "Subtitle", "Skips a level"]
        .into_iter()
        .filter(|h| o.all_positions().iter().any(|p| p.h(&o) == *h))
        .collect();
    assert_eq!(heads, ["Title", "Subtitle", "Skips a level"]);
    // The front matter's closing `---` does not underline `list:`.
    assert!(o.all_positions().iter().all(|p| p.h(&o) != "list:"));
    assert_eq!(find(&o, "<< nested >>").b(&o), "```python\ninner\n```\n");
    // An indented fence's code loses its indent in the node.
    assert_eq!(
        find(&o, "<< indented >>").b(&o),
        "def f():\n    return 1\n\n"
    );
    assert_eq!(find(&o, "<< open >>").b(&o), "no close, no final newline");
}

#[test]
fn an_edited_fence_node_changes_only_its_code() {
    let dir = tempfile::tempdir().unwrap();
    let doc = "# A\n\n```python #add\nx = 1\n```\n\nText.\n";
    let leo = outline_over(dir.path(), "doc.md", doc);
    let (mut o, _) = leolib::open_outline_with_report(&leo, true).unwrap();
    let add = find(&o, "<< add >>");
    o.set_body(&add, "x = 1\ny = 2\n");
    let result = external::write_external_files(&mut o, true);
    assert_eq!(result.written.len(), 1, "{:?}", result.errors);
    assert_eq!(
        fs::read_to_string(dir.path().join("doc.md")).unwrap(),
        "# A\n\n```python #add\nx = 1\ny = 2\n```\n\nText.\n"
    );
}

#[test]
fn a_renamed_heading_is_written_in_its_own_style() {
    let dir = tempfile::tempdir().unwrap();
    let doc = "Top\n===\n\n## Two ##\n\nText.\n";
    let mut o = open(dir.path(), "doc.md", doc);
    let top = find(&o, "Top");
    o.set_headline(&top, "Summit");
    let two = find(&o, "Two");
    o.set_headline(&two, "Second");
    assert_eq!(written(&o), "Summit\n======\n\n## Second\n\nText.\n");
}

#[test]
fn a_new_node_is_a_heading_one_level_below_its_parent() {
    let dir = tempfile::tempdir().unwrap();
    let mut o = open(dir.path(), "doc.md", "# A\n\nText.\n");
    let a = find(&o, "A");
    let b = o.insert_as_last_child(&a);
    o.set_headline(&b, "B");
    o.set_body(&b, "More.\n");
    assert_eq!(written(&o), "# A\n\nText.\n## B\nMore.\n");
}

#[test]
fn a_deleted_fence_node_refuses_the_write_and_keeps_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let doc = "# A\n\n```python #add\nx = 1\n```\n";
    let leo = outline_over(dir.path(), "doc.md", doc);
    let (mut o, _) = leolib::open_outline_with_report(&leo, true).unwrap();
    let add = find(&o, "<< add >>");
    o.delete_position(&add);
    let a = find(&o, "A");
    o.set_dirty(&a);
    let result = external::write_external_files(&mut o, true);
    assert!(result.written.is_empty());
    assert!(
        matches!(&result.errors[0].error, Error::Write { detail } if detail.contains("<< add >>")),
        "{:?}",
        result.errors
    );
    assert_eq!(fs::read_to_string(dir.path().join("doc.md")).unwrap(), doc);
}

#[test]
fn crlf_line_endings_are_read_and_reported() {
    let dir = tempfile::tempdir().unwrap();
    let leo = outline_over(
        dir.path(),
        "doc.md",
        "# A\r\n\r\n```python #add\r\nx = 1\r\n```\r\n",
    );
    let (o, report) = leolib::open_outline_with_report(&leo, true).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(find(&o, "<< add >>").b(&o), "x = 1\n");
    assert!(
        report.warnings.iter().any(|w| w.message.contains("CRLF")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn the_dot_leo_file_stores_only_the_entangled_node() {
    let dir = tempfile::tempdir().unwrap();
    let leo = outline_over(dir.path(), "doc.md", "# A\n\n```python #add\nx = 1\n```\n");
    let (mut o, _) = leolib::open_outline_with_report(&leo, true).unwrap();
    leolib::save(&mut o, &leo).unwrap();
    let xml = fs::read_to_string(&leo).unwrap();
    assert!(xml.contains("@entangled doc.md"), "{xml}");
    assert!(!xml.contains("add"), "{xml}");
    assert!(!xml.contains("leo-rs-"), "{xml}");
}
