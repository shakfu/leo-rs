//! Entry points for the fuzz targets in `fuzz/`, behind the `fuzzing` feature.
//!
//! The parsers they reach are crate-private; this module is not an API.

use crate::outline::Outline;

/// Read `contents` as an `@file` external file into a fresh outline.
pub fn read_into_root(contents: &str) {
    let mut o = Outline::new_empty();
    let root = o.root_position().expect("a new outline has a node");
    o.set_headline(&root, "@file fuzz.py");
    let _ = crate::atfile_read::read_into_root(&mut o, contents, "fuzz.py", &root);
}

/// Unpickle `bytes` as a uA blob. A value that reads must write back to bytes
/// that read as the same value.
pub fn pickle(bytes: &[u8]) {
    if let Ok(value) = crate::pickle::loads(bytes) {
        let again = crate::pickle::loads(&crate::pickle::dumps(&value));
        assert_eq!(again.ok(), Some(value));
    }
}
