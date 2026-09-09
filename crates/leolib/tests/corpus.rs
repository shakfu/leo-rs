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
    // Two files in leo-editor cannot round-trip, and fail in Leo too: one
    // contains the literal text `@others`, which the writer reads as a
    // directive; the other loses the trailing blanks of a whitespace-only
    // line when `move_blank_lines` runs. Anything else is a regression.
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
