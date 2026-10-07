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

// --- Renaming a block ------------------------------------------------------

/// A `.leo` file over two `@entangled` documents, both written beside it.
fn two_documents(dir: &Path, readme: &str, tests: &str) -> leolib::Document {
    fs::write(dir.join("README.md"), readme).unwrap();
    fs::write(dir.join("tests.md"), tests).unwrap();
    let leo = dir.join("doc.leo");
    fs::write(
        &leo,
        r#"<?xml version="1.0" encoding="utf-8"?>
<leo_file xmlns:leo="http://leoeditor.com/namespaces/leo-python-editor/1.1" >
<leo_header file_format="2"/>
<vnodes>
<v t="a.1"><vh>@entangled README.md</vh></v>
<v t="a.2"><vh>@entangled tests.md</vh></v>
</vnodes>
<tnodes>
<t tx="a.1"></t>
<t tx="a.2"></t>
</tnodes>
</leo_file>
"#,
    )
    .unwrap();
    let doc = leolib::Document::open(&leo.to_string_lossy(), true).unwrap();
    assert!(
        doc.read_report.errors.is_empty(),
        "{:?}",
        doc.read_report.errors
    );
    doc
}

fn written_files(doc: &mut leolib::Document) -> (String, String) {
    let o = doc.outline();
    let roots: Vec<Position> = o
        .all_positions()
        .into_iter()
        .filter(|p| p.is_at_entangled_node(o))
        .collect();
    (
        leolib::entangled::write_string(o, &roots[0]).unwrap(),
        leolib::entangled::write_string(o, &roots[1]).unwrap(),
    )
}

const README: &str =
    "# Lib\n\n```python #count\nn = 1\n```\n\nMore of it:\n\n```python #count\nn += 1\n```\n\n\
                      ```python #report file=report.py\n<<count>>\nprint(n)\n```\n";
const TESTS: &str =
    "# Tests\n\n```python file=test_readme.py\ndef test_count():\n    <<README.md#count>>\n```\n";

#[test]
fn renaming_a_fence_node_renames_the_block_everywhere_as_one_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = two_documents(dir.path(), README, TESTS);
    let count = find(doc.outline(), "<< count >>");
    let r = doc
        .rename_entangled_block(&count, "<< tally >>")
        .unwrap()
        .unwrap();
    assert_eq!(
        (r.old.as_str(), r.new.as_str(), r.fences, r.references),
        ("count", "tally", 2, 2)
    );
    let (readme, tests) = written_files(&mut doc);
    assert_eq!(
        readme,
        "# Lib\n\n```python #tally\nn = 1\n```\n\nMore of it:\n\n```python #tally\nn += 1\n```\n\n\
         ```python #report file=report.py\n<<tally>>\nprint(n)\n```\n"
    );
    assert_eq!(
        tests,
        "# Tests\n\n```python file=test_readme.py\ndef test_count():\n    <<README.md#tally>>\n```\n"
    );
    doc.undo();
    assert_eq!(
        written_files(&mut doc),
        (README.to_string(), TESTS.to_string())
    );
}

#[test]
fn a_bare_name_is_enough_and_another_documents_own_block_is_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let tests =
        "# Tests\n\n```python #count\nlocal = 1\n```\n\n```python file=t.py\n<<count>>\n```\n";
    let mut doc = two_documents(dir.path(), README, tests);
    let count = find(doc.outline(), "<< count >>");
    let r = doc
        .rename_entangled_block(&count, "tally")
        .unwrap()
        .unwrap();
    assert_eq!((r.fences, r.references), (2, 1));
    let (_, tests_after) = written_files(&mut doc);
    assert_eq!(
        tests_after, tests,
        "tests.md's <<count>> means its own block"
    );
}

#[test]
fn a_rename_is_refused_for_a_block_named_by_its_file_or_a_name_with_a_space() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = two_documents(dir.path(), README, TESTS);
    let target = find(doc.outline(), "<< test_readme.py >>");
    let why = doc
        .rename_entangled_block(&target, "<< other >>")
        .unwrap_err();
    assert!(why.contains("file="), "{why}");
    let count = find(doc.outline(), "<< count >>");
    assert!(doc
        .rename_entangled_block(&count, "<< two words >>")
        .is_err());
    assert_eq!(
        written_files(&mut doc),
        (README.to_string(), TESTS.to_string())
    );
}

#[test]
fn knitr_and_quarto_labels_are_renamed() {
    let dir = tempfile::tempdir().unwrap();
    let readme = "```{python, label=knit}\na\n```\n\n```{python}\n#| label: quart\nb\n```\n";
    let mut doc = two_documents(dir.path(), readme, "# T\n");
    let knit = find(doc.outline(), "<< knit >>");
    doc.rename_entangled_block(&knit, "<< knitted >>")
        .unwrap()
        .unwrap();
    let quart = find(doc.outline(), "<< quart >>");
    doc.rename_entangled_block(&quart, "<< quarto >>")
        .unwrap()
        .unwrap();
    let (readme_after, _) = written_files(&mut doc);
    assert_eq!(
        readme_after,
        "```{python, label=knitted}\na\n```\n\n```{python}\n#| label: quarto\nb\n```\n"
    );
}
