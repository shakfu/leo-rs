//! `@qmd` and `@rmd`: Quarto and R Markdown with cells as nodes.

use std::fs;
use std::path::Path;

use leolib::{external, Outline, Position};

/// The kinds these tests open outlines with.
fn kinds() -> leolib::ext::Kinds {
    leolib::ext::Kinds::empty()
        .with(leo_markdown::QMD)
        .unwrap()
        .with(leo_markdown::RMD)
        .unwrap()
}

/// A `.leo` file holding one node headlined `kind name`, with `contents`
/// written beside it as `name`. Returns the `.leo` path.
fn outline_over(dir: &Path, kind: &str, name: &str, contents: &str) -> String {
    fs::write(dir.join(name), contents).unwrap();
    let leo = dir.join("doc.leo").to_string_lossy().to_string();
    let xml = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<leo_file xmlns:leo="http://leoeditor.com/namespaces/leo-python-editor/1.1" >
<leo_header file_format="2"/>
<vnodes>
<v t="a.1"><vh>{kind} {name}</vh></v>
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

/// `@rmd` for an R Markdown file's name, else `@qmd`.
fn kind_for(name: &str) -> &'static str {
    match name.to_lowercase().ends_with(".rmd") {
        true => "@rmd",
        false => "@qmd",
    }
}

/// Open `contents` as a `@qmd` or `@rmd` file, by its name; fails the test
/// on a read error.
fn open(dir: &Path, name: &str, contents: &str) -> Outline {
    let leo = outline_over(dir, kind_for(name), name, contents);
    let (o, report) = leolib::open_outline_with_kinds(&leo, true, kinds()).unwrap();
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    o
}

fn doc(dir: &Path, name: &str, contents: &str) -> leolib::Document {
    let leo = outline_over(dir, kind_for(name), name, contents);
    leolib::Document::open_with(&leo, true, kinds()).unwrap()
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

fn written(o: &Outline) -> String {
    leo_markdown::write_string(o, &root(o)).unwrap()
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

const QMD: &str = "---\ntitle: \"Report\"\nformat: html\n---\n\n\
# Load\n\nSome prose.\n\n```{python}\n#| label: load\nimport pandas as pd\n```\n\n\
```{python}\ndf = pd.read_csv(\"x.csv\")\n```\n\n\
A display fence stays in the prose:\n\n```python\nprint(1)\n```\n\n\
```{.python}\nshown()\n```\n\n```{=html}\n<b>raw</b>\n```\n\n\
## Plot\n\n```{r}\n#| echo: false\nplot(1)\n```\n";

const RMD: &str = "---\ntitle: Paper\n---\n\n# Setup\n\n\
```{r setup, include=FALSE}\nlibrary(x)\n```\n\n\
```{r, echo=FALSE}\nsummary(x)\n```\n\n```{python label=\"py\"}\n1\n```\n";

const MD: &str = "# Notes\n\n```python #helper\ndef f(): pass\n```\n\n\
```python file=out.py\nnot a node\n```\n\n```{python}\nrun()\n```\n";

#[test]
fn every_sample_document_is_written_back_byte_for_byte() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files: Vec<std::path::PathBuf> =
        fs::read_dir(base.join("../leo-entangled/tests/data/entangled"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
    for doc in ["README.md", "tests.md"] {
        files.push(base.join("../../demo/entangled").join(doc));
    }
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let contents = fs::read_to_string(&path).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut o = open(dir.path(), &name, &contents);
        assert_eq!(written(&o), contents, "{name}");
        let result = external::write_external_files(&mut o, false);
        assert!(result.errors.is_empty(), "{name}: {:?}", result.errors);
        assert_eq!(result.unchanged, 1, "{name}: {:?}", result.written);
    }
    for (name, text) in [("r.qmd", QMD), ("p.Rmd", RMD)] {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(written(&open(dir.path(), name, text)), text, "{name}");
    }
}

#[test]
fn quarto_cells_are_nodes_and_display_fences_stay_prose() {
    let dir = tempfile::tempdir().unwrap();
    let o = open(dir.path(), "r.qmd", QMD);
    assert_eq!(
        shape(&o),
        [
            "Load",
            "  << load >>",
            "  << python cell 2 >>",
            "  Plot",
            "    << r cell 3 >>"
        ]
    );
    assert!(
        root(&o).b(&o).starts_with("---\ntitle:"),
        "front matter in the root"
    );
    assert_eq!(
        find(&o, "<< load >>").b(&o),
        "#| label: load\nimport pandas as pd\n"
    );
    assert_eq!(
        o.language_at(&find(&o, "<< r cell 3 >>")).as_deref(),
        Some("r")
    );
    let load = find(&o, "Load").b(&o).to_string();
    assert!(load.contains("```python\nprint(1)\n```"), "{load}");
    assert!(load.contains("```{.python}\nshown()\n```"), "{load}");
}

#[test]
fn knitr_chunks_take_their_positional_label() {
    let dir = tempfile::tempdir().unwrap();
    let o = open(dir.path(), "p.Rmd", RMD);
    assert_eq!(
        shape(&o),
        ["Setup", "  << setup >>", "  << r cell 2 >>", "  << py >>"]
    );
}

#[test]
fn the_kind_fixes_the_rules_whatever_the_extension() {
    let dir = tempfile::tempdir().unwrap();
    // knitr's chunk label names a cell under @rmd, even in a `.md` file...
    let text = "# A\n\n```{r setup}\nx <- 1\n```\n";
    let leo = outline_over(dir.path(), "@rmd", "notes.md", text);
    let (o, _) = leolib::open_outline_with_kinds(&leo, true, kinds()).unwrap();
    assert_eq!(shape(&o), ["A", "  << setup >>"]);
    // ...and not under @qmd, where the label is `#| label:`.
    let leo = outline_over(dir.path(), "@qmd", "notes.md", text);
    let (o, _) = leolib::open_outline_with_kinds(&leo, true, kinds()).unwrap();
    assert_eq!(shape(&o), ["A", "  << r cell 1 >>"]);
}

#[test]
fn an_edited_cell_changes_only_its_code() {
    let dir = tempfile::tempdir().unwrap();
    let mut o = open(dir.path(), "r.qmd", QMD);
    let cell = find(&o, "<< python cell 2 >>");
    o.set_body(&cell, "df = None\n");
    assert_eq!(
        written(&o),
        QMD.replace("df = pd.read_csv(\"x.csv\")\n", "df = None\n")
    );
}

#[test]
fn renaming_an_unnamed_cell_gives_it_a_label_in_the_files_own_syntax() {
    for (name, text, cell, expect) in [
        (
            "r.qmd",
            QMD,
            "<< python cell 2 >>",
            QMD.replace("```{python}\ndf =", "```{python}\n#| label: read\ndf ="),
        ),
        (
            "p.Rmd",
            RMD,
            "<< r cell 2 >>",
            RMD.replace("{r, echo=FALSE}", "{r read, echo=FALSE}"),
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut d = doc(dir.path(), name, text);
        let p = find(d.outline(), cell);
        let r = d.rename_block(&p, "read").unwrap().unwrap();
        assert!(r.note.is_none());
        assert_eq!(written(d.outline()), expect, "{name}");
        // And it reads back under its new name.
        fs::write(dir.path().join(name), &expect).unwrap();
        let o = open(dir.path(), name, &expect);
        find(&o, "<< read >>");
    }
}

#[test]
fn renaming_a_labelled_cell_renames_its_label() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = doc(dir.path(), "p.Rmd", RMD);
    let p = find(d.outline(), "<< setup >>");
    d.rename_block(&p, "<< init >>").unwrap().unwrap();
    assert_eq!(written(d.outline()), RMD.replace("{r setup,", "{r init,"));
    let mut d = doc(dir.path(), "r.qmd", QMD);
    let p = find(d.outline(), "<< load >>");
    d.rename_block(&p, "data").unwrap().unwrap();
    assert_eq!(
        written(d.outline()),
        QMD.replace("#| label: load", "#| label: data")
    );
    // A label in use, or one with a space, is refused.
    let p = find(d.outline(), "<< python cell 2 >>");
    assert!(d.rename_block(&p, "data").is_err());
    assert!(d.rename_block(&p, "two words").is_err());
}

#[test]
fn two_cells_with_one_label_are_refused_and_the_file_kept() {
    let dir = tempfile::tempdir().unwrap();
    let text = "```{r a}\n1\n```\n\n```{r a}\n2\n```\n";
    let leo = outline_over(dir.path(), "@rmd", "x.Rmd", text);
    let (mut o, report) = leolib::open_outline_with_kinds(&leo, true, kinds()).unwrap();
    let why = report.errors[0].error.to_string();
    assert!(why.contains("`a` is defined by 2 fences"), "{why}");
    assert_eq!(root(&o).b(&o), text);
    external::write_external_files(&mut o, false);
    assert_eq!(fs::read_to_string(dir.path().join("x.Rmd")).unwrap(), text);
}

#[test]
fn the_dot_leo_file_stores_only_the_qmd_node() {
    let dir = tempfile::tempdir().unwrap();
    let leo = outline_over(dir.path(), "@qmd", "r.qmd", QMD);
    let mut d = leolib::Document::open_with(&leo, true, kinds()).unwrap();
    d.save(&leo).unwrap();
    let xml = fs::read_to_string(&leo).unwrap();
    assert!(xml.contains("<vh>@qmd r.qmd</vh>"), "{xml}");
    assert!(!xml.contains("cell 2"), "{xml}");
    assert!(!xml.contains("pandas"), "{xml}");
}

#[test]
fn a_headings_markdown_has_its_cells_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let o = open(dir.path(), "r.qmd", QMD);
    let plot = leo_markdown::node_markdown(&o, &find(&o, "Plot")).unwrap();
    assert_eq!(plot, "## Plot\n\n```{r}\n#| echo: false\nplot(1)\n```\n");
}

#[test]
fn plain_auto_on_markdown_is_still_leos_importer() {
    let dir = tempfile::tempdir().unwrap();
    let leo = outline_over(dir.path(), "@auto", "n.md", MD);
    let (o, _) = leolib::open_outline_with_kinds(&leo, true, kinds()).unwrap();
    // Leo's importer: headings only; fences stay in the body.
    assert_eq!(shape(&o), ["Notes"]);
}

// --- Fenced divs --------------------------------------------------------------

const DIVS: &str = "# Results\n\n\
::: {.callout-note}\n## A note\nInside the callout.\n:::\n\n\
::::: {.panel-tabset}\n\n## Python\n\n```{python}\nprint(1)\n```\n\n\
## R\n\n::: {.column}\nNested\n=====\n:::\n\n```{r}\nprint(2)\n```\n\n:::::\n\n\
:::\nNot a div: no open div to close.\n\n## After\n\n::: aside\n### Side\n:::\n";

#[test]
fn a_heading_inside_a_fenced_div_stays_in_the_body() {
    let dir = tempfile::tempdir().unwrap();
    let o = open(dir.path(), "r.qmd", DIVS);
    // The callout's title, the tabs, the nested underlined heading and the
    // aside's heading stay text; cells inside a div are still nodes.
    assert_eq!(
        shape(&o),
        [
            "Results",
            "  << python cell 1 >>",
            "  << r cell 2 >>",
            "  After",
        ]
    );
    let results = find(&o, "Results").b(&o).to_string();
    assert!(
        results.contains("::: {.callout-note}\n## A note\n"),
        "{results}"
    );
    assert!(
        results.contains("## R\n\n::: {.column}\nNested\n=====\n:::\n"),
        "{results}"
    );
    assert!(find(&o, "After")
        .b(&o)
        .contains("::: aside\n### Side\n:::\n"));
    assert_eq!(written(&o), DIVS);
}

// --- Cells cloned into @clean: code-first literate programming ---------------

/// `r.qmd` open under `@qmd`, with `<< load >>` cloned into a new
/// `@clean analysis.py` whose body is `clean_body`, saved. Returns the
/// document and the `.leo` path.
fn literate(dir: &Path, clean_body: &str) -> (leolib::Document, String) {
    let leo = outline_over(dir, "@qmd", "r.qmd", QMD);
    let mut d = leolib::Document::open_with(&leo, true, kinds()).unwrap();
    let r = root(d.outline());
    let clean = d.outline_mut_untracked().insert_after(&r);
    d.set_headline(&clean, "@clean analysis.py");
    let load = find(d.outline(), "<< load >>");
    let copy = d.clone_node(&load);
    d.outline_mut_untracked()
        .move_to_nth_child_of(&copy, &clean, 0);
    let clean = find(d.outline(), "@clean analysis.py");
    d.set_body(&clean, clean_body);
    let saved = d.save_all(&leo);
    assert!(
        saved.leo.is_ok() && saved.files.errors.is_empty(),
        "{:?}",
        saved.files.errors
    );
    (d, leo)
}

fn positions(o: &Outline, headline: &str) -> Vec<Position> {
    o.all_positions()
        .into_iter()
        .filter(|p| p.h(o) == headline)
        .collect()
}

#[test]
fn a_cell_cloned_into_clean_writes_the_code_file_and_stays_a_clone() {
    let dir = tempfile::tempdir().unwrap();
    let (_, leo) = literate(
        dir.path(),
        "@language python\n<< load >>\nprint(\"done\")\n",
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("analysis.py")).unwrap(),
        "#| label: load\nimport pandas as pd\nprint(\"done\")\n"
    );
    // The markdown is unchanged, and the .leo file holds the clone's gnx
    // but no in-memory attribute.
    assert_eq!(fs::read_to_string(dir.path().join("r.qmd")).unwrap(), QMD);
    let xml = fs::read_to_string(&leo).unwrap();
    assert!(xml.contains("str_leo-rs-cell-ids="), "{xml}");
    assert!(!xml.contains(" leo-rs-"), "{xml}");

    // Reopened, the cell under @qmd is the node under @clean.
    let (o, report) = leolib::open_outline_with_kinds(&leo, true, kinds()).unwrap();
    assert!(
        report.errors.is_empty() && report.conflicts.is_empty(),
        "{report:?}"
    );
    let both = positions(&o, "<< load >>");
    assert_eq!(both.len(), 2);
    assert_eq!(both[0].v, both[1].v);
    assert_eq!(o.language_at(&both[1]).as_deref(), Some("python"));
}

#[test]
fn an_edit_in_either_tree_reaches_both_files() {
    let dir = tempfile::tempdir().unwrap();
    let (_, leo) = literate(dir.path(), "<< load >>\n");
    let mut d = leolib::Document::open_with(&leo, true, kinds()).unwrap();
    let in_clean = positions(d.outline(), "<< load >>")
        .into_iter()
        .find(|p| p.parent(d.outline()).unwrap().h(d.outline()) == "@clean analysis.py")
        .unwrap();
    d.set_body(&in_clean, "#| label: load\nimport polars as pl\n");
    let saved = d.save_all(&leo);
    assert!(saved.files.errors.is_empty(), "{:?}", saved.files.errors);
    assert_eq!(
        fs::read_to_string(dir.path().join("analysis.py")).unwrap(),
        "#| label: load\nimport polars as pl\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("r.qmd")).unwrap(),
        QMD.replace("import pandas as pd", "import polars as pl")
    );
}

#[test]
fn both_files_changed_on_disk_is_a_clone_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let (_, leo) = literate(dir.path(), "<< load >>\n");
    let qmd = QMD.replace("import pandas as pd", "import duckdb");
    fs::write(dir.path().join("r.qmd"), &qmd).unwrap();
    fs::write(
        dir.path().join("analysis.py"),
        "#| label: load\nimport arrow\n",
    )
    .unwrap();
    let (o, report) = leolib::open_outline_with_kinds(&leo, true, kinds()).unwrap();
    assert_eq!(report.conflicts.len(), 1, "{report:?}");
    let c = &report.conflicts[0];
    assert!(
        c.old.body.contains("duckdb") && c.new.body.contains("arrow"),
        "{c:?}"
    );
    find(&o, "Recovered Nodes");
}

#[test]
fn a_reference_in_a_cell_resolves_as_a_leo_section_and_a_namespaced_one_does_not() {
    let dir = tempfile::tempdir().unwrap();
    // `<<load>>`, entangled's spelling, matches the `<< load >>` headline.
    let (_, _) = literate(dir.path(), "<<load>>\n");
    assert_eq!(
        fs::read_to_string(dir.path().join("analysis.py")).unwrap(),
        "#| label: load\nimport pandas as pd\n"
    );
    let dir = tempfile::tempdir().unwrap();
    let leo = outline_over(dir.path(), "@qmd", "r.qmd", QMD);
    let mut d = leolib::Document::open_with(&leo, true, kinds()).unwrap();
    let r = root(d.outline());
    let clean = d.outline_mut_untracked().insert_after(&r);
    d.set_headline(&clean, "@clean analysis.py");
    d.set_body(&clean, "<<r.qmd#load>>\n");
    let saved = d.save_all(&leo);
    let why = format!("{:?}", saved.files.errors);
    assert!(why.contains("undefined section"), "{why}");
}

#[test]
fn a_cell_with_children_is_not_written_into_the_markdown() {
    let dir = tempfile::tempdir().unwrap();
    let mut o = open(dir.path(), "r.qmd", QMD);
    let load = find(&o, "<< load >>");
    o.insert_as_last_child(&load);
    let err = leo_markdown::write_string(&o, &root(&o))
        .unwrap_err()
        .to_string();
    assert!(err.contains("<< load >> has children"), "{err}");
}

#[test]
fn a_labelled_cell_keeps_its_gnx_and_an_unnamed_one_is_not_saved() {
    let dir = tempfile::tempdir().unwrap();
    let leo = outline_over(dir.path(), "@qmd", "r.qmd", QMD);
    let mut d = leolib::Document::open_with(&leo, true, kinds()).unwrap();
    d.save(&leo).unwrap();
    let ids = fs::read_to_string(&leo).unwrap();
    assert!(!ids.contains("cell 2"), "only labels are saved: {ids}");
    let load = find(d.outline(), "<< load >>").gnx(d.outline()).to_string();
    let again = leolib::Document::open_with(&leo, true, kinds()).unwrap();
    assert_eq!(
        find(again.outline(), "<< load >>").gnx(again.outline()),
        load
    );
}
