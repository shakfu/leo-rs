//! Import files as @auto nodes and report whether each round-trips.
//!
//!     cargo run --example autocheck -- DIR_OR_FILE [DIR_OR_FILE ...]
//!
//! An @auto node's file is regenerated from its tree alone, so an importer
//! that does not reproduce the file it read would overwrite the user's source.

use std::path::{Path, PathBuf};

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
        let name = p.file_name().map(|s| s.to_string_lossy().to_string());
        if name.as_deref() == Some("__pycache__")
            || name.as_deref().map(|s| s.starts_with('.')) == Some(true)
        {
            continue;
        }
        collect(&p, out);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: autocheck DIR_OR_FILE ...");
        std::process::exit(2);
    }
    let mut files = Vec::new();
    for a in &args {
        collect(Path::new(a), &mut files);
    }
    files.sort();

    let (mut ok, mut skipped, mut failed) = (0usize, 0usize, 0usize);
    let mut one_node = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for file in &files {
        let path = file.to_string_lossy().to_string();
        if leolib::importers::spec_for("", &path).is_none() {
            skipped += 1;
            continue;
        }
        let mut o = leolib::Outline::new_empty();
        o.file_name = format!("{}/x.leo", leolib::util::os_path_dirname(&path));
        let root = o.root_position().unwrap();
        o.set_headline(&root, &format!("@auto {path}"));
        match leolib::external::read_file_at_position(&mut o, &root) {
            Ok(_) => {
                ok += 1;
                if root.self_and_subtree(&o).len() == 1 {
                    one_node += 1;
                }
            }
            Err(e) => {
                failed += 1;
                if failures.len() < 10 {
                    failures.push(format!("{}: {e}", leolib::util::short_file_name(&path)));
                }
            }
        }
    }
    for f in &failures {
        println!("FAILED {f}");
    }
    println!(
        "files: {} imported: {ok} (of those, {one_node} as a single node) failed: {failed} no importer: {skipped}",
        files.len()
    );
    if failed > 0 {
        std::process::exit(1);
    }
}
