//! Load and save times for Leo's own outline.
//!
//!     cargo bench -p leolib
//!     LEO_EDITOR=~/projects/leo-editor cargo bench -p leolib
//!
//! `demo/LeoPyRef.leo` is a copy of leo-editor's outline without its sources,
//! so it times the `.leo` reader and writer alone. With `LEO_EDITOR` set, the
//! checkout's own `leo/core/LeoPyRef.leo` is read with its external files too,
//! which is the load `docs/dev/comparison.md` reports.

use std::path::{Path, PathBuf};

use criterion::{criterion_group, criterion_main, Criterion};

fn demo() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo/LeoPyRef.leo");
    path.to_string_lossy().to_string()
}

fn leo_editor() -> Option<String> {
    let dir = std::env::var_os("LEO_EDITOR")?;
    let path = PathBuf::from(dir).join("leo/core/LeoPyRef.leo");
    path.exists().then(|| path.to_string_lossy().to_string())
}

fn load(c: &mut Criterion) {
    let path = demo();
    c.bench_function("open demo/LeoPyRef.leo", |b| {
        b.iter(|| leolib::open_outline(&path, false).unwrap())
    });

    let dir = tempfile::tempdir().unwrap();
    let copy = dir
        .path()
        .join("LeoPyRef.leo")
        .to_string_lossy()
        .to_string();
    let mut o = leolib::open_outline(&path, false).unwrap();
    c.bench_function("save demo/LeoPyRef.leo", |b| {
        b.iter(|| leolib::save(&mut o, &copy).unwrap())
    });

    if let Some(path) = leo_editor() {
        let mut group = c.benchmark_group("leo-editor");
        // Each read parses every external file of Leo's core.
        group.sample_size(20);
        group.bench_function("open LeoPyRef.leo with external files", |b| {
            b.iter(|| leolib::open_outline(&path, true).unwrap())
        });
        group.finish();
    }
}

criterion_group!(benches, load);
criterion_main!(benches);
