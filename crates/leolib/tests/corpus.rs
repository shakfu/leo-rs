//! Check the port against a real Leo outline and its external files.
//!
//! Set `LEO_CORPUS` to a `.leo` file to run these. They are the strongest
//! evidence the port is faithful: every external file the outline names must
//! tangle back to exactly the bytes on disk, and the `.leo` writer must
//! reproduce the file it read.
//!
//!     LEO_CORPUS=~/leo-editor/leo/core/LeoPyRef.leo cargo test -p leolib

use leolib::external;

fn corpus() -> Option<String> {
    let path = std::env::var("LEO_CORPUS").ok()?;
    let path = leolib::util::finalize(&path);
    if std::path::Path::new(&path).exists() {
        Some(path)
    } else {
        panic!("LEO_CORPUS does not exist: {path}");
    }
}

#[test]
fn every_external_file_tangles_to_the_bytes_on_disk() {
    let Some(path) = corpus() else {
        eprintln!("skipped: set LEO_CORPUS to a .leo file");
        return;
    };
    let o = leolib::open_outline(&path, true).expect("open failed");
    let (files, _ignored) = external::find_files_to_write(&o, false);
    assert!(files.len() > 1, "the corpus has no external files");
    let mut differ = Vec::new();
    for p in &files {
        let disk_path = o.full_path(p);
        let Ok(disk) = std::fs::read_to_string(&disk_path) else {
            continue; // A file this machine does not have.
        };
        match external::file_contents(&o, p) {
            Ok((text, _, _)) if text == disk => {}
            Ok(_) => differ.push(p.h(&o).to_string()),
            Err(e) => differ.push(format!("{}: {e}", p.h(&o))),
        }
    }
    assert!(
        differ.is_empty(),
        "{} files differ: {differ:?}",
        differ.len()
    );
}

#[test]
fn the_leo_writer_reproduces_the_file_it_read() {
    let Some(path) = corpus() else {
        eprintln!("skipped: set LEO_CORPUS to a .leo file");
        return;
    };
    // Without the external files: reading them fills in bodies that the .leo
    // file does not store, and writing them back would not match.
    let mut o = leolib::open_outline(&path, false).expect("open failed");
    let written = leolib::to_xml(&mut o);
    let original = std::fs::read_to_string(&path).unwrap();
    assert_eq!(written, original);
}

/// Every file under `LEO_CORPUS_DIR` that has an importer must import and
/// write back unchanged. This is the only guard an `@auto` node has: its file
/// is regenerated from the tree alone.
#[test]
fn every_importable_file_survives_an_at_auto_round_trip() {
    let Ok(dir) = std::env::var("LEO_CORPUS_DIR") else {
        eprintln!("skipped: set LEO_CORPUS_DIR to a directory of source files");
        return;
    };
    let mut files = Vec::new();
    collect(std::path::Path::new(&dir), &mut files);
    let mut checked = 0usize;
    let mut failures = Vec::new();
    for path in &files {
        let path = path.to_string_lossy().to_string();
        if leolib::importers::spec_for("", &path).is_none() {
            continue;
        }
        let mut o = leolib::Outline::new_empty();
        o.file_name = format!("{}/x.leo", leolib::util::os_path_dirname(&path));
        let root = o.root_position().unwrap();
        o.set_headline(&root, &format!("@auto {path}"));
        match external::read_file_at_position(&mut o, &root) {
            Ok(_) => checked += 1,
            Err(e) => failures.push(format!("{path}: {e}")),
        }
    }
    assert!(checked > 0, "no importable files under {dir}");
    // Two files in leo-editor cannot round-trip, and fail in Leo too.
    // slide-008.html quotes `@others` twice, which the writer reads as
    // directives. jquery.color.js loses the trailing blanks of a
    // whitespace-only line when `move_blank_lines` runs. Anything else is a
    // regression.
    let known = ["jquery.color.js", "slide-008.html"];
    let unexpected: Vec<&String> = failures
        .iter()
        .filter(|f| !known.iter().any(|k| f.contains(k)))
        .collect();
    assert!(
        unexpected.is_empty(),
        "{} of {checked} files failed unexpectedly: {:?}",
        unexpected.len(),
        &unexpected[..unexpected.len().min(5)]
    );
}

/// Every file under `LEO_CORPUS_DIR` that has an importer must import as
/// `@file`, and the sentinel file it would write must read back into the same
/// tree. The import already checks the text; this checks the tree. Nothing is
/// written to disk.
#[test]
fn every_importable_file_survives_an_at_file_import() {
    let Ok(dir) = std::env::var("LEO_CORPUS_DIR") else {
        eprintln!("skipped: set LEO_CORPUS_DIR to a directory of source files");
        return;
    };
    let mut files = Vec::new();
    collect(std::path::Path::new(&dir), &mut files);
    let (mut split, mut whole, mut read) = (0usize, 0usize, 0usize);
    let mut refused = Vec::new();
    let mut differ = Vec::new();
    for path in &files {
        let path = path.to_string_lossy().to_string();
        if leolib::importers::spec_for("", &path).is_none() {
            continue;
        }
        let leo = format!("{}/x.leo", leolib::util::os_path_dirname(&path));
        let mut doc = leolib::Document::new_empty(&leo);
        let root = doc.outline.root_position().unwrap();
        let p = match doc.import_at_file(&root, &path) {
            Err(e) => {
                refused.push(format!("{path}: {e}"));
                continue;
            }
            Ok((_, false)) => {
                read += 1;
                continue;
            }
            Ok((p, true)) => p,
        };
        let o = &mut doc.outline;
        if p.children(o).is_empty() {
            whole += 1;
        } else {
            split += 1;
        }
        let text = external::file_contents(o, &p).map(|(t, _, _)| t);
        let q = o.insert_after(&p);
        let same = text.is_ok_and(|t| leolib::atfile_read::read_into_root(o, &t, &path, &q))
            && tree(o, &p) == tree(o, &q);
        if !same {
            differ.push(path);
        }
    }
    eprintln!(
        "@file import: {split} split, {whole} kept whole, {read} read by sentinels, {} refused",
        refused.len()
    );
    for r in &refused {
        eprintln!("  refused {r}");
    }
    assert!(split > 0, "no file under {dir} imported as a split tree");
    // A Leo tutorial page quoting `@others` twice in `<pre>` blocks. In an
    // @file body both are directives, and a node may have only one.
    let known = ["slide-008.html"];
    let unexpected: Vec<&String> = refused
        .iter()
        .filter(|r| !known.iter().any(|k| r.contains(k)))
        .collect();
    assert!(
        unexpected.is_empty(),
        "refused unexpectedly: {unexpected:?}"
    );
    assert!(
        differ.is_empty(),
        "{} of {} files read back differently: {:?}",
        differ.len(),
        split + whole,
        &differ[..differ.len().min(5)]
    );
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

fn collect(path: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    if path.is_file() {
        out.push(path.to_path_buf());
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        let name = p.file_name().map(|s| s.to_string_lossy().to_string());
        let skip = name.as_deref() == Some("__pycache__")
            || name.as_deref().map(|s| s.starts_with('.')) == Some(true);
        if !skip {
            collect(&p, out);
        }
    }
}
