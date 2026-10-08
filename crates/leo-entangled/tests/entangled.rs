//! `@entangled`: markdown whose named code fences are nodes.
//!
//! `data/entangled/` holds entangled-rs's example documents
//! (github.com/shakfu/entangled-rs, `examples/`, MIT), copied unchanged.

use std::fs;
use std::path::Path;

use leolib::{external, Error, Outline, Position};

/// The kinds these tests open outlines with.
fn kinds() -> leolib::ext::Kinds {
    leolib::ext::Kinds::empty()
        .with(leo_entangled::Entangled)
        .unwrap()
}

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
    let (o, report) = leolib::open_outline_with_kinds(&leo, true, kinds()).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    o
}

fn find(o: &Outline, headline: &str) -> Position {
    o.all_positions()
        .into_iter()
        .find(|p| p.h(o) == headline)
        .unwrap_or_else(|| panic!("no node {headline:?}"))
}

/// Every headline below the root, indented two spaces a level.
fn shape(o: &Outline) -> Vec<String> {
    let r = root(o);
    r.subtree(o)
        .into_iter()
        .filter(|p| p.v != r.v)
        .map(|p| format!("{}{}", "  ".repeat(p.level() - r.level() - 1), p.h(o)))
        .collect()
}

fn root(o: &Outline) -> Position {
    o.root_position().unwrap()
}

/// What the tree writes, as leolib would write it to disk.
fn written(o: &Outline) -> String {
    leo_entangled::write_string(o, &root(o)).unwrap()
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
               ```cpp #c\nint c;\n```\n";
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
fn a_fence_is_named_as_entangled_names_it_in_its_documents_style() {
    // `.md`: `#name` or `file=`, with or without Pandoc's braces.
    let dir = tempfile::tempdir().unwrap();
    let md = "```python file=out.py\na\n```\n\n```{.python #pan}\nb\n```\n\n\
              ```python\nunnamed\n```\n\n```{python, label=knit}\nnot this style\n```\n";
    let o = open(dir.path(), "doc.md", md);
    assert_eq!(fences(&o), ["<< out.py >>", "<< pan >>"]);
    assert!(root(&o).b(&o).contains("```python\nunnamed\n```\n"));
    assert_eq!(written(&o), md);

    // `.Rmd`: knitr's `label=` and `file=`; a `#name` is not knitr's.
    let dir = tempfile::tempdir().unwrap();
    let rmd = "```{python, label=knit, file=k.py}\nb\n```\n\n```{python, file=k2.py}\nc\n```\n\n\
               ```python #md file=md.py\nnot this style\n```\n";
    let o = open(dir.path(), "doc.Rmd", rmd);
    assert_eq!(fences(&o), ["<< knit >>", "<< k2.py >>"]);
    assert_eq!(written(&o), rmd);

    // `.qmd`: `#|` options opening the code; they stay in it.
    let dir = tempfile::tempdir().unwrap();
    let qmd = "```{python}\n#| label: quarto\nc\n```\n\n```{python}\n#| file: q.py\nd\n```\n\n\
               ```python #md\nnot this style\n```\n";
    let o = open(dir.path(), "doc.qmd", qmd);
    assert_eq!(fences(&o), ["<< quarto >>", "<< q.py >>"]);
    assert_eq!(find(&o, "<< quarto >>").b(&o), "#| label: quarto\nc\n");
    assert_eq!(written(&o), qmd);
}

/// The fence nodes' headlines, in outline order.
fn fences(o: &Outline) -> Vec<String> {
    o.all_positions()
        .iter()
        .filter(|p| leo_markdown::is_fence_node(o, p))
        .map(|p| p.h(o).to_string())
        .collect()
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
    let (mut o, _) = leolib::open_outline_with_kinds(&leo, true, kinds()).unwrap();
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
    let (mut o, _) = leolib::open_outline_with_kinds(&leo, true, kinds()).unwrap();
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
    let (o, report) = leolib::open_outline_with_kinds(&leo, true, kinds()).unwrap();
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
    let (mut o, _) = leolib::open_outline_with_kinds(&leo, true, kinds()).unwrap();
    leolib::save(&mut o, &leo).unwrap();
    let xml = fs::read_to_string(&leo).unwrap();
    assert!(xml.contains("@entangled doc.md"), "{xml}");
    // No fence node and no code: the tree is read from the markdown. The
    // one leo-rs attribute is the map that gives `add` its gnx back.
    assert!(
        !xml.contains("&lt;&lt; add") && !xml.contains("x = 1"),
        "{xml}"
    );
    assert_eq!(xml.matches("leo-rs-").count(), 1, "{xml}");
    assert!(xml.contains(" str_leo-rs-cell-ids=\"add "), "{xml}");
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
    let doc = leolib::Document::open_with(&leo.to_string_lossy(), true, kinds()).unwrap();
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
        .filter(|p| leo_entangled::is_entangled(p.h(o)))
        .collect();
    (
        leo_entangled::write_string(o, &roots[0]).unwrap(),
        leo_entangled::write_string(o, &roots[1]).unwrap(),
    )
}

const README: &str =
    "# Lib\n\n```python #count\nn = 1\n```\n\nMore of it:\n\n```python #more\nn += 1\n```\n\n\
                      ```python #report file=report.py\n<<count>>\n<<more>>\nprint(n)\n```\n";
const TESTS: &str =
    "# Tests\n\n```python file=test_readme.py\ndef test_count():\n    <<README.md#count>>\n```\n";

#[test]
fn renaming_a_fence_node_renames_the_block_everywhere_as_one_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = two_documents(dir.path(), README, TESTS);
    let count = find(doc.outline(), "<< count >>");
    let r = doc.rename_block(&count, "<< tally >>").unwrap().unwrap();
    assert_eq!(
        (r.old.as_str(), r.new.as_str(), r.fences, r.references),
        ("count", "tally", 1, 2)
    );
    let (readme, tests) = written_files(&mut doc);
    assert_eq!(
        readme,
        "# Lib\n\n```python #tally\nn = 1\n```\n\nMore of it:\n\n```python #more\nn += 1\n```\n\n\
         ```python #report file=report.py\n<<tally>>\n<<more>>\nprint(n)\n```\n"
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
    let r = doc.rename_block(&count, "tally").unwrap().unwrap();
    assert_eq!((r.fences, r.references), (1, 1));
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
    let why = doc.rename_block(&target, "<< other >>").unwrap_err();
    assert!(why.contains("file="), "{why}");
    let count = find(doc.outline(), "<< count >>");
    assert!(doc.rename_block(&count, "<< two words >>").is_err());
    assert_eq!(
        written_files(&mut doc),
        (README.to_string(), TESTS.to_string())
    );
}

#[test]
fn knitr_and_quarto_labels_are_renamed() {
    for (name, before, after) in [
        (
            "doc.Rmd",
            "```{python, label=knit}\na\n```\n",
            "```{python, label=knitted}\na\n```\n",
        ),
        (
            "doc.qmd",
            "```{python}\n#| label: knit\nb\n```\n",
            "```{python}\n#| label: knitted\nb\n```\n",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let leo = outline_over(dir.path(), name, before);
        let mut doc = leolib::Document::open_with(&leo, true, kinds()).unwrap();
        let knit = find(doc.outline(), "<< knit >>");
        doc.rename_block(&knit, "<< knitted >>").unwrap().unwrap();
        let o = doc.outline();
        assert_eq!(
            leo_entangled::write_string(o, &root(o)).unwrap(),
            after,
            "{name}"
        );
    }
}

// --- Against entangled's own reader -----------------------------------------

/// The blocks entangled's reader finds in `text`, named as the fence nodes
/// are: the name without its document prefix, or a file target's path.
fn entangled_blocks(name: &str, text: &str) -> Vec<String> {
    let mut config = entangled::config::Config::default();
    // As its project's entangled.toml says.
    if name == "pandoc-style.md" {
        config.style = entangled::Style::Pandoc;
    }
    let doc = entangled::readers::parse_markdown(text, Some(Path::new(name)), &config)
        .unwrap_or_else(|e| panic!("entangled cannot read {name}: {e}"));
    doc.refs
        .iter()
        .map(|(id, _)| match id.name.file_path() {
            Some(path) => path.to_string(),
            None => id.name.as_str().rsplit('#').next().unwrap().to_string(),
        })
        .collect()
}

#[test]
fn fence_nodes_are_the_blocks_entangleds_reader_finds() {
    const MIXED: &str = "```python #a file=a.py\na\n```\n\n```{.python #b}\nb\n```\n\n\
                         ```{python, label=c, file=c.py}\nc\n```\n\n\
                         ```{python}\n#| label: d\n#| file: d.py\nd\n```\n\n\
                         ```python file=e.py\ne\n```\n\n```python\nanonymous\n```\n";
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/entangled");
    let mut docs: Vec<(String, String)> = fs::read_dir(&data)
        .unwrap()
        .map(|e| e.unwrap().path())
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            (name, fs::read_to_string(&p).unwrap())
        })
        .collect();
    docs.extend([
        (
            "knitr.Rmd".to_string(),
            "```{python, label=c, file=c.py}\nc\n```\n\n```{python, file=e.py}\ne\n```\n"
                .to_string(),
        ),
        (
            "quarto.qmd".to_string(),
            "```{python}\n#| label: d\nd\n```\n\n```{python}\n#| file: e.py\ne\n```\n".to_string(),
        ),
        ("mixed.Rmd".to_string(), MIXED.to_string()),
        ("mixed.qmd".to_string(), MIXED.to_string()),
    ]);
    assert_eq!(docs.len(), 9);
    for (name, text) in docs {
        let dir = tempfile::tempdir().unwrap();
        let o = open(dir.path(), &name, &text);
        let ours: Vec<String> = fences(&o)
            .iter()
            .map(|h| {
                h.trim_start_matches("<< ")
                    .trim_end_matches(" >>")
                    .to_string()
            })
            .collect();
        assert_eq!(ours, entangled_blocks(&name, &text), "{name}");
    }
}

#[test]
fn a_headings_markdown_has_its_code_in_place_and_not_its_subheadings() {
    let dir = tempfile::tempdir().unwrap();
    let doc = "# A\n\nIntro.\n\n```python #add\nx = 1\n```\n\n## B\n\nMore.\n";
    let o = open(dir.path(), "doc.md", doc);
    let a = find(&o, "A");
    assert_eq!(
        leo_entangled::node_markdown(&o, &a).as_deref(),
        Some("# A\n\nIntro.\n\n```python #add\nx = 1\n```\n\n")
    );
    assert_eq!(
        leo_entangled::node_markdown(&o, &find(&o, "B")).as_deref(),
        Some("## B\n\nMore.\n")
    );
    assert_eq!(
        leo_entangled::node_markdown(&o, &find(&o, "<< add >>")),
        None
    );
}

// --- include= ---------------------------------------------------------------

const LIB: &str = "import re\n\n# ANCHOR: count\ndef count(text):\n    # ANCHOR: inner\n    return len(text.split())\n    # ANCHOR_END: inner\n# ANCHOR_END: count\n";

#[test]
fn an_include_fence_is_filled_from_its_file_and_left_unsaved() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("lib.py"), LIB).unwrap();
    let doc = "# A\n\n```python #lib include=lib.py\nstale\n```\n";
    let leo = outline_over(dir.path(), "doc.md", doc);
    let (mut o, report) = leolib::open_outline_with_kinds(&leo, true, kinds()).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    let lib = find(&o, "<< lib >>");
    assert_eq!(lib.b(&o), LIB);
    assert_eq!(report.refilled.len(), 1);
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.message.contains("include=")),
        "{:?}",
        report.warnings
    );
    assert!(root(&o).is_dirty(&o), "the next save writes the markdown");
    let result = external::write_external_files(&mut o, true);
    assert_eq!(result.written.len(), 1, "{:?}", result.errors);
    assert_eq!(
        fs::read_to_string(dir.path().join("doc.md")).unwrap(),
        format!("# A\n\n```python #lib include=lib.py\n{LIB}```\n")
    );
}

#[test]
fn an_anchor_takes_the_lines_between_its_markers() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("lib.py"), LIB).unwrap();
    assert_eq!(
        leo_entangled::include_text(dir.path(), "lib.py#count").unwrap(),
        "def count(text):\n    return len(text.split())\n"
    );
    assert_eq!(
        leo_entangled::include_text(dir.path(), "lib.py#inner").unwrap(),
        "    return len(text.split())\n"
    );
    let why = leo_entangled::include_text(dir.path(), "lib.py#nope").unwrap_err();
    assert!(why.contains("no `ANCHOR: nope`"), "{why}");
    assert!(leo_entangled::include_text(dir.path(), "gone.py").is_err());
}

#[test]
fn an_up_to_date_include_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("lib.py"), "x = 1\n").unwrap();
    let doc = "```python include=lib.py\nx = 1\n```\n";
    let leo = outline_over(dir.path(), "doc.md", doc);
    let (mut o, report) = leolib::open_outline_with_kinds(&leo, true, kinds()).unwrap();
    assert!(
        report.refilled.is_empty() && report.warnings.is_empty(),
        "{report:?}"
    );
    // An unnamed include= fence is a node, named by its target.
    assert_eq!(fences(&o), ["<< lib.py >>"]);
    assert!(!root(&o).is_dirty(&o));
    let result = external::write_external_files(&mut o, false);
    assert_eq!((result.written.len(), result.unchanged), (0, 1));
}

#[test]
fn a_missing_file_or_anchor_is_reported_and_the_fence_kept() {
    let dir = tempfile::tempdir().unwrap();
    let doc = "```python include=gone.py\nkept\n```\n";
    let leo = outline_over(dir.path(), "doc.md", doc);
    let (o, report) = leolib::open_outline_with_kinds(&leo, true, kinds()).unwrap();
    assert!(report.errors.is_empty());
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.message.contains("include=gone.py")),
        "{:?}",
        report.warnings
    );
    assert_eq!(find(&o, "<< gone.py >>").b(&o), "kept\n");
    assert!(!root(&o).is_dirty(&o));
}

#[test]
fn saving_fills_an_include_fence_from_the_file_as_it_is_then() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("lib.py"), "x = 1\n").unwrap();
    let doc = "# A\n\n```python #lib include=lib.py\nx = 1\n```\n";
    let leo = outline_over(dir.path(), "doc.md", doc);
    let (mut o, _) = leolib::open_outline_with_kinds(&leo, true, kinds()).unwrap();
    // The file is changed and tested after the outline was read.
    fs::write(dir.path().join("lib.py"), "x = 2\n").unwrap();
    let a = find(&o, "A");
    o.set_headline(&a, "Alpha");
    external::write_external_files(&mut o, true);
    assert_eq!(
        fs::read_to_string(dir.path().join("doc.md")).unwrap(),
        "# Alpha\n\n```python #lib include=lib.py\nx = 2\n```\n"
    );
}

#[test]
fn a_block_named_by_its_include_target_cannot_be_renamed() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("lib.py"), "x = 1\n").unwrap();
    let leo = outline_over(
        dir.path(),
        "doc.md",
        "```python include=lib.py\nx = 1\n```\n",
    );
    let mut doc = leolib::Document::open_with(&leo, true, kinds()).unwrap();
    let lib = find(doc.outline(), "<< lib.py >>");
    let why = doc.rename_block(&lib, "<< other >>").unwrap_err();
    assert!(why.contains("include="), "{why}");
}

#[test]
fn include_is_knitrs_own_option_in_an_rmd_file() {
    let dir = tempfile::tempdir().unwrap();
    let doc = "```{python, include=FALSE}\nhidden\n```\n";
    let o = open(dir.path(), "doc.Rmd", doc);
    assert!(fences(&o).is_empty());
    assert_eq!(written(&o), doc);
}

// --- Heading levels follow the tree ------------------------------------------

fn doc_over(dir: &Path, text: &str) -> leolib::Document {
    let leo = outline_over(dir, "doc.md", text);
    leolib::Document::open_with(&leo, true, kinds()).unwrap()
}

fn doc_written(doc: &leolib::Document) -> String {
    let o = doc.outline();
    leo_entangled::write_string(o, &root(o)).unwrap()
}

#[test]
fn a_demoted_heading_goes_one_level_down_and_back_on_promote() {
    let dir = tempfile::tempdir().unwrap();
    let text = "# Doc\n\n## Install\n\nA.\n\n## Usage\n\nB.\n\n### Flags\n\nC.\n";
    let mut doc = doc_over(dir.path(), text);
    let usage = find(doc.outline(), "Usage");
    let usage = doc.move_right(&usage).expect("Usage goes under Install");
    // Usage and its subsection Flags each go one level down.
    assert_eq!(
        doc_written(&doc),
        "# Doc\n\n## Install\n\nA.\n\n### Usage\n\nB.\n\n#### Flags\n\nC.\n"
    );
    doc.move_left(&usage).expect("and back");
    assert_eq!(
        doc_written(&doc),
        text,
        "back where it was, as the file had it"
    );
}

#[test]
fn an_underlined_heading_moved_deeper_becomes_atx_and_levels_stop_at_six() {
    let dir = tempfile::tempdir().unwrap();
    let text = "Top\n===\n\nOne\n---\n\nTwo\n---\n";
    let mut doc = doc_over(dir.path(), text);
    let two = find(doc.outline(), "Two");
    doc.move_right(&two).expect("Two goes under One");
    assert_eq!(doc_written(&doc), "Top\n===\n\nOne\n---\n\n### Two\n");

    let text = "###### Six\n\n###### Under\n";
    let mut doc = doc_over(dir.path(), text);
    let under = find(doc.outline(), "Under");
    doc.move_right(&under).expect("Under goes under Six");
    assert_eq!(
        doc_written(&doc),
        "###### Six\n\n###### Under\n",
        "no seventh level"
    );
}

#[test]
fn a_skipped_level_survives_a_move_that_keeps_its_depth() {
    let dir = tempfile::tempdir().unwrap();
    let text = "# A\n\n### Deep\n\nx\n\n# B\n";
    let mut doc = doc_over(dir.path(), text);
    let deep = find(doc.outline(), "Deep");
    let b = find(doc.outline(), "B");
    // Deep moves from under A to under B: the same depth, so still `###`.
    doc.outline_mut_untracked()
        .move_to_nth_child_of(&deep, &b, 0);
    assert_eq!(doc_written(&doc), "# A\n\n# B\n### Deep\n\nx\n\n");
}

#[test]
fn a_new_heading_under_a_moved_one_is_a_level_below_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = doc_over(dir.path(), "# A\n\n## B\n\n## C\n");
    let c = find(doc.outline(), "C");
    let c = doc.move_right(&c).expect("C goes under B");
    let d = doc.insert_child(&c);
    doc.set_headline(&d, "D");
    let o = doc.outline();
    assert_eq!(
        leo_entangled::node_markdown(o, &d).as_deref(),
        Some("#### D\n")
    );
    assert_eq!(doc_written(&doc), "# A\n\n## B\n\n### C\n#### D\n");
}

// --- One fence per name ------------------------------------------------------

#[test]
fn a_block_split_across_fences_is_refused_and_the_file_kept() {
    let dir = tempfile::tempdir().unwrap();
    let text = "# A\n\n```python #imports\nimport os\n```\n\nLater:\n\n```python #imports\nimport re\n```\n";
    let leo = outline_over(dir.path(), "doc.md", text);
    let (mut o, report) = leolib::open_outline_with_kinds(&leo, true, kinds()).unwrap();
    assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
    let why = report.errors[0].error.to_string();
    assert!(
        why.contains("block `imports` is defined by 2 fences"),
        "{why}"
    );
    // The node holds the whole file and writes it back unchanged.
    assert_eq!(root(&o).b(&o), text);
    assert!(fences(&o).is_empty());
    external::write_external_files(&mut o, false);
    assert_eq!(fs::read_to_string(dir.path().join("doc.md")).unwrap(), text);
}

#[test]
fn a_rename_onto_a_name_in_use_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = two_documents(dir.path(), README, TESTS);
    let count = find(doc.outline(), "<< count >>");
    let why = doc.rename_block(&count, "<< more >>").unwrap_err();
    assert!(why.contains("`more` is already in this document"), "{why}");
    assert_eq!(
        written_files(&mut doc),
        (README.to_string(), TESTS.to_string())
    );
}

#[test]
fn the_writer_refuses_two_fences_with_one_name() {
    let dir = tempfile::tempdir().unwrap();
    let mut doc = two_documents(dir.path(), README, TESTS);
    // Bypass the rename's check, as an MCP client setting both by hand could.
    let more = find(doc.outline(), "<< more >>");
    doc.set_headline(&more, "<< count >>");
    let lib = find(doc.outline(), "Lib");
    let body = lib
        .b(doc.outline())
        .replace("#more\n<< more >>", "#count\n<< count >>");
    doc.set_body(&lib, &body);
    let o = doc.outline();
    let err = leo_entangled::write_string(o, &root(o))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("block `count` is defined by 2 fences"),
        "{err}"
    );
}

#[test]
fn an_r_fence_is_r_and_not_rebol() {
    // Leo's extension table reads `.r` as REBOL; in a fence `r` is R.
    let dir = tempfile::tempdir().unwrap();
    let o = open(dir.path(), "p.Rmd", "```{r, label=fit}\nlm(y ~ x)\n```\n");
    assert_eq!(o.language_at(&find(&o, "<< fit >>")).as_deref(), Some("r"));
}

#[test]
fn entangled_keeps_a_divs_heading_in_the_body_too() {
    let dir = tempfile::tempdir().unwrap();
    let text = "# A\n\n::: {.callout-tip}\n## Tip\n```python #tip\nx = 1\n```\n:::\n\n## B\n";
    let leo = outline_over(dir.path(), "doc.md", text);
    let (o, report) = leolib::open_outline_with_kinds(&leo, true, kinds()).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    assert_eq!(shape(&o), ["A", "  << tip >>", "  B"]);
    assert_eq!(leo_entangled::write_string(&o, &root(&o)).unwrap(), text);
}
