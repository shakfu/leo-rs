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
