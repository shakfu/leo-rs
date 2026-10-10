//! The plugin examples in `demo/`, which the conformance corpus skips.
#![cfg(all(feature = "markdown", feature = "wiki"))]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use leolib::{external, Outline};

/// `demo/NAME`, copied to a temporary directory so a write cannot touch it.
/// None outside the workspace: the published crate has no `demo/`.
fn example(name: &str) -> Option<(tempfile::TempDir, String)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../demo")
        .join(name);
    if !src.is_dir() {
        return None;
    }
    let dir = tempfile::tempdir().unwrap();
    for entry in fs::read_dir(&src).unwrap() {
        let p: PathBuf = entry.unwrap().path();
        if !p.is_file() {
            continue;
        }
        fs::copy(&p, dir.path().join(p.file_name().unwrap())).unwrap();
    }
    let leo = dir.path().join("demo.leo").to_string_lossy().to_string();
    Some((dir, leo))
}

fn open(leo: &str) -> Outline {
    let (o, report) = leolib::open_outline_with_kinds(leo, true, leo_plugins::kinds()).unwrap();
    assert!(
        report.errors.is_empty() && report.conflicts.is_empty(),
        "{report:?}"
    );
    o
}

/// Every headline, indented two spaces a level.
fn shape(o: &Outline) -> Vec<String> {
    o.all_positions()
        .iter()
        .map(|p| format!("{}{}", "  ".repeat(p.level()), p.h(o)))
        .collect()
}

/// Opening and saving leaves every external file as it was, and each cell
/// in `cloned` is one node in both trees.
fn literate(name: &str, expected: &[&str], cloned: &[&str]) {
    let Some((_dir, leo)) = example(name) else {
        return;
    };
    let mut o = open(&leo);
    assert_eq!(shape(&o), expected);
    for cell in cloned {
        let both: Vec<_> = o
            .all_positions()
            .into_iter()
            .filter(|p| p.h(&o) == *cell)
            .collect();
        assert!(both.len() == 2 && both[0].v == both[1].v, "{cell}");
    }
    let written = external::write_external_files(&mut o, false);
    assert!(written.errors.is_empty(), "{:?}", written.errors);
    assert_eq!(written.unchanged, 2, "{:?}", written.written);
}

#[test]
fn the_qmd_example_opens_with_its_cells_cloned_into_clean() {
    literate(
        "qmd",
        &[
            "About this demo",
            "@qmd rainfall.qmd",
            "  Data",
            "    << data >>",
            "  Summary",
            "    << stats >>",
            "    << python cell 3 >>",
            "@clean rainfall.py",
            "  << data >>",
            "  << stats >>",
        ],
        &["<< data >>", "<< stats >>"],
    );
}

#[test]
fn the_rmd_example_opens_with_its_chunks_cloned_into_clean() {
    literate(
        "rmd",
        &[
            "About this demo",
            "@rmd growth.Rmd",
            "  Setup",
            "    << setup >>",
            "  Model",
            "    << fit >>",
            "    << r cell 3 >>",
            "    Group means",
            "      << means >>",
            "@clean growth.R",
            "  << setup >>",
            "  << fit >>",
            "  << means >>",
        ],
        &["<< setup >>", "<< fit >>", "<< means >>"],
    );
}

#[test]
fn the_wiki_example_keeps_the_rules_and_exports() {
    let Some((_dir, leo)) = example("wiki") else {
        return;
    };
    let o = open(&leo);
    assert_eq!(leo_wiki::check(&o), Vec::<String>::new());
    let roots = leo_wiki::roots(&o);
    assert_eq!(roots.len(), 2);
    let garden = leo_wiki::export(&o, &roots[0]).unwrap();
    for link in [
        "[the herb bed](#herbs)",
        "[Notes](#notes-1)",
        "[Sow/transplant](#sowtransplant)",
        "[Mulch](glossary.md#mulch)",
        "`[[North]]`",
    ] {
        assert!(garden.contains(link), "{link}\n{garden}");
    }
    let glossary = leo_wiki::export(&o, &roots[1]).unwrap();
    assert!(glossary.contains("[North](garden.md#north)"), "{glossary}");
}

// --- Running the examples, where the tools are installed ----------------------

/// Whether `program args` runs and exits 0.
fn runs(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .output()
        .is_ok_and(|out| out.status.success())
}

/// Run `program args` in `dir`; fails the test unless it exits 0. Returns
/// its stdout.
fn run(dir: &Path, program: &str, args: &[&str], env: &[(&str, &Path)]) -> String {
    let out = Command::new(program)
        .args(args)
        .current_dir(dir)
        .envs(env.iter().copied())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{program} {args:?}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn skip(why: &str) {
    eprintln!("skipped: {why}");
}

/// The uv environment's Python, with Jupyter: `uv sync` in the repository.
fn jupyter_python() -> Option<PathBuf> {
    let python = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.venv/bin/python");
    let imports = "import jupyter_client, nbformat, ipykernel, yaml";
    runs(&python.to_string_lossy(), &["-c", imports]).then_some(python)
}

const RMARKDOWN: &str =
    "quit(status = !(requireNamespace('rmarkdown', quietly = TRUE) && rmarkdown::pandoc_available()))";

#[test]
fn the_qmd_examples_cells_run_and_render() {
    let Some((dir, _)) = example("qmd") else {
        return;
    };
    let printed = "mean 60.7 mm, wettest month 11";
    if runs("python3", &["--version"]) {
        assert!(run(dir.path(), "python3", &["rainfall.py"], &[]).contains(printed));
    } else {
        skip("python3 is not installed");
    }
    match jupyter_python() {
        Some(python) if runs("quarto", &["--version"]) => {
            let env = [("QUARTO_PYTHON", python.as_path())];
            run(dir.path(), "quarto", &["render", "rainfall.qmd"], &env);
            let html = fs::read_to_string(dir.path().join("rainfall.html")).unwrap();
            assert!(html.contains(printed));
        }
        _ => skip("quarto, or Jupyter in the uv environment, is not installed"),
    }
}

#[test]
fn the_rmd_examples_chunks_run_and_render() {
    let Some((dir, _)) = example("rmd") else {
        return;
    };
    if !runs("Rscript", &["--version"]) {
        return skip("R is not installed");
    }
    assert!(run(dir.path(), "Rscript", &["growth.R"], &[]).contains("5.03"));
    if !runs("Rscript", &["-e", RMARKDOWN]) {
        return skip("rmarkdown, or the pandoc it needs, is not installed");
    }
    let render = "rmarkdown::render('growth.Rmd', quiet = TRUE)";
    run(dir.path(), "Rscript", &["-e", render], &[]);
    let html = fs::read_to_string(dir.path().join("growth.html")).unwrap();
    assert!(html.contains("5.03"));
}
