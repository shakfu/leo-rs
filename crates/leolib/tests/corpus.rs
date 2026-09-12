//! Check the port against the conformance corpus in `demo/`.
//!
//! Every `.leo` file under `demo/` is a case, beside the external files it
//! names, and each has a `<name>.expected.json` written by Python Leo:
//! `scripts/make_corpus.py`. For every case:
//!
//! - reading it gives the positions the expected file lists;
//! - the `.leo` writer reproduces the file it read;
//! - every external file that was read tangles back to the bytes on disk.
//!
//! leo-editor keeps a copy of the same corpus and checks Python Leo against
//! it, so the two implementations answer to the same files. Nothing outside
//! this repository is read.

use std::path::{Path, PathBuf};

use leolib::external;
use serde_json::Value;

/// Cases this port reads differently from Python Leo, and why. A case listed
/// here that no longer differs fails the test, so the list cannot go stale.
const KNOWN: &[(&str, &str)] = &[
    (
        "cases/empty_auto/empty_auto.leo",
        "Leo's at.readFileAtPosition raises AttributeError on an @auto file with \
         nothing in it and reports it unread; this port imports the empty tree",
    ),
    (
        "cases/encoding/encoding.leo",
        "a file that is not UTF-8 is left unread; Leo decodes it with its own encoding",
    ),
];

/// External files this port does not tangle to the bytes on disk, and why.
/// As with KNOWN, an entry that no longer differs fails the test.
const KNOWN_TANGLE: &[(&str, &str)] = &[];

fn demo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo")
}

/// Every `.leo` file under `demo/`, in a stable order.
fn cases() -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect(&demo(), &mut files);
    let mut leo: Vec<PathBuf> = files
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "leo"))
        .collect();
    leo.sort();
    assert!(leo.len() >= 3, "no corpus under {}", demo().display());
    leo
}

fn name(case: &Path) -> String {
    case.strip_prefix(demo())
        .unwrap_or(case)
        .to_string_lossy()
        .to_string()
}

fn expected(case: &Path) -> Value {
    let stem = case.file_stem().unwrap().to_string_lossy();
    let path = case.with_file_name(format!("{stem}.expected.json"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}; run scripts/make_corpus.py", path.display()));
    serde_json::from_str(&text).unwrap()
}

/// One line per position: level, gnx (null under an `@auto` node, whose
/// nodes get fresh gnxs on every read), headline and body.
fn positions(o: &leolib::Outline) -> Vec<Value> {
    o.all_positions()
        .iter()
        .map(|p| {
            let imported = p
                .self_and_parents(o)
                .iter()
                .skip(1)
                .any(|q| q.is_at_auto_node(o));
            serde_json::json!({
                "level": p.level(),
                "gnx": if imported { Value::Null } else { Value::from(p.gnx(o)) },
                "h": p.h(o),
                "b": p.b(o),
            })
        })
        .collect()
}

/// Where two position lists first part, for a readable failure.
fn first_difference(got: &[Value], want: &[Value]) -> String {
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        if g != w {
            return format!("position {i}: got {g}, expected {w}");
        }
    }
    format!("{} positions, expected {}", got.len(), want.len())
}

#[test]
fn every_case_reads_as_python_leo_reads_it() {
    let mut differ: Vec<(String, String)> = Vec::new();
    for case in cases() {
        let want = expected(&case);
        let read_external = want["read_external"].as_bool().unwrap();
        let path = case.to_string_lossy().to_string();
        let (o, report) = leolib::open_outline_with_report(&path, read_external).unwrap();
        let mut unread: Vec<String> = report.errors.iter().map(|e| e.headline.clone()).collect();
        unread.sort();
        let want_unread: Vec<String> = want["unread"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        let got = positions(&o);
        let want_positions = want["positions"].as_array().unwrap();
        if unread != want_unread {
            differ.push((
                name(&case),
                format!("unread {unread:?}, expected {want_unread:?}"),
            ));
        } else if got != *want_positions {
            differ.push((name(&case), first_difference(&got, want_positions)));
        }
    }
    let unexpected: Vec<&(String, String)> = differ
        .iter()
        .filter(|(case, _)| !KNOWN.iter().any(|(k, _)| k == case))
        .collect();
    let stale: Vec<&str> = KNOWN
        .iter()
        .map(|(k, _)| *k)
        .filter(|k| !differ.iter().any(|(case, _)| case == k))
        .collect();
    assert!(
        unexpected.is_empty(),
        "read differently from Python Leo: {unexpected:#?}"
    );
    assert!(
        stale.is_empty(),
        "listed in KNOWN but no longer differ: {stale:?}"
    );
}

#[test]
fn the_leo_writer_reproduces_every_outline() {
    for case in cases() {
        // Without the external files: reading them fills in bodies that the
        // .leo file does not store, and writing those back would not match.
        let mut o = leolib::open_outline(&case.to_string_lossy(), false).unwrap();
        let original = std::fs::read_to_string(&case).unwrap();
        assert!(
            leolib::to_xml(&mut o) == original,
            "{} is not rewritten unchanged",
            name(&case)
        );
    }
}

#[test]
fn every_external_file_tangles_to_the_bytes_on_disk() {
    let mut checked = 0;
    let mut differ = Vec::new();
    for case in cases() {
        if !expected(&case)["read_external"].as_bool().unwrap() {
            continue;
        }
        let (o, report) = leolib::open_outline_with_report(&case.to_string_lossy(), true).unwrap();
        // A file the read could not take in has nothing in the outline to reproduce.
        let unread: Vec<&str> = report.errors.iter().map(|e| e.headline.as_str()).collect();
        let (files, _ignored) = external::find_files_to_write(&o, false);
        for p in &files {
            if unread.iter().any(|h| *h == p.h(&o)) {
                continue;
            }
            let disk = std::fs::read(o.full_path(p)).unwrap();
            checked += 1;
            match external::file_contents(&o, p) {
                Ok((text, newline, encoding)) => {
                    if encode(&text.replace('\n', &newline), &encoding) != disk {
                        differ.push(format!("{}: {}", name(&case), p.h(&o)));
                    }
                }
                Err(e) => differ.push(format!("{}: {}: {e}", name(&case), p.h(&o))),
            }
        }
    }
    assert!(checked > 10, "only {checked} external files in the corpus");
    let unexpected: Vec<&String> = differ
        .iter()
        .filter(|d| !KNOWN_TANGLE.iter().any(|(k, _)| d.starts_with(k)))
        .collect();
    let stale: Vec<&str> = KNOWN_TANGLE
        .iter()
        .map(|(k, _)| *k)
        .filter(|k| !differ.iter().any(|d| d.starts_with(k)))
        .collect();
    assert!(
        unexpected.is_empty(),
        "{} files differ: {unexpected:#?}",
        unexpected.len()
    );
    assert!(
        stale.is_empty(),
        "listed in KNOWN_TANGLE but no longer differ: {stale:?}"
    );
}

/// The bytes Leo writes for text in an encoding.
fn encode(text: &str, encoding: &str) -> Vec<u8> {
    match encoding.to_lowercase().as_str() {
        "latin-1" | "latin1" | "iso-8859-1" => text.chars().map(|c| c as u32 as u8).collect(),
        _ => text.as_bytes().to_vec(),
    }
}

/// Every source file in the corpus that has an importer must import as
/// `@auto` and write back unchanged. This is the only guard an `@auto` node
/// has: its file is regenerated from the tree alone.
#[test]
fn every_importable_file_survives_an_at_auto_round_trip() {
    let mut checked = 0usize;
    let mut failures = Vec::new();
    for path in sources() {
        let mut o = leolib::Outline::new_empty();
        o.file_name = format!("{}/x.leo", leolib::util::os_path_dirname(&path));
        let root = o.root_position().unwrap();
        o.set_headline(&root, &format!("@auto {path}"));
        match external::read_file_at_position(&mut o, &root) {
            Ok(_) => checked += 1,
            Err(e) => failures.push(format!("{path}: {e}")),
        }
    }
    assert!(checked > 3, "only {checked} importable files in the corpus");
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Every source file in the corpus that has an importer must import as
/// `@file`, and the sentinel file it would write must read back into the same
/// tree. Nothing is written to disk.
#[test]
fn every_importable_file_survives_an_at_file_import() {
    let mut differ = Vec::new();
    let mut split = 0usize;
    for path in sources() {
        let leo = format!("{}/x.leo", leolib::util::os_path_dirname(&path));
        let mut doc = leolib::Document::new_empty(&leo);
        let root = doc.outline.root_position().unwrap();
        let p = match doc.import_at_file(&root, &path) {
            Err(e) => {
                differ.push(format!("{path}: refused: {e}"));
                continue;
            }
            Ok((_, false)) => continue, // Read by its sentinels.
            Ok((p, true)) => p,
        };
        let o = &mut doc.outline;
        if !p.children(o).is_empty() {
            split += 1;
        }
        let text = external::file_contents(o, &p).map(|(t, _, _)| t);
        let q = o.insert_after(&p);
        let same = text
            .is_ok_and(|t| leolib::atfile_read::read_into_root(o, &t, &path, &q).is_ok())
            && tree(o, &p) == tree(o, &q);
        if !same {
            differ.push(format!("{path}: read back differently"));
        }
    }
    assert!(split > 0, "no corpus file imported as a split tree");
    assert!(differ.is_empty(), "{differ:#?}");
}

/// The corpus's plain source files that have an `@auto` importer.
///
/// A file with sentinels is an `@file` file, and importing it means nothing.
/// Nor does a file that is not UTF-8: `import_at_file` refuses those, on
/// purpose, and the `@encoding` case is one.
fn sources() -> Vec<String> {
    let mut files = Vec::new();
    collect(&demo(), &mut files);
    let mut out: Vec<String> = files
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .filter(|p| !p.ends_with(".leo") && !p.ends_with(".json"))
        .filter(|p| leolib::importers::spec_for("", p).is_some())
        .filter(|p| std::fs::read_to_string(p).is_ok_and(|s| !s.contains("@+leo-ver=")))
        .collect();
    out.sort();
    out
}

/// The headline and body of each node under p, in outline order, with p's
/// own headline left out: the reader sets it from the sentinels.
fn tree(o: &leolib::Outline, p: &leolib::Position) -> Vec<(String, String)> {
    let mut out = vec![(String::new(), p.b(o).to_string())];
    for child in p.children(o) {
        let mut sub = tree(o, &child);
        sub[0].0 = child.h(o).to_string();
        out.extend(sub);
    }
    out
}

fn collect(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        out.push(path.to_path_buf());
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let hidden = p
            .file_name()
            .is_some_and(|s| s.to_string_lossy().starts_with('.'));
        if !hidden {
            collect(&p, out);
        }
    }
}
