//! leolib: Leo's outline model and file machinery, with no view of any kind.
//!
//! A Rust port of `leo/leolib` from the Leo editor. The boundary is the point:
//! this crate reads and writes `.leo` files and the external files they refer
//! to, and knows nothing about how any of it is displayed.
//!
//! ```no_run
//! let mut outline = leolib::open_outline("myfile.leo", true).unwrap();
//! for p in outline.all_unique_positions() {
//!     println!("{}", p.h(&outline));
//! }
//! ```

pub(crate) mod atclean;
pub mod atfile_read;
pub(crate) mod atfile_write;
pub mod document;
pub mod error;
pub mod external;
pub mod gnx;
pub mod importers;
pub mod langdata;
pub mod leofile;
pub mod node;
pub mod outline;
pub(crate) mod pickle;
pub mod position;
pub(crate) mod seqmatch;
pub mod state;
pub mod undo;
pub mod util;

pub use document::Document;
pub use error::{Error, Result};
pub use node::{Vnode, VnodeId};
pub use outline::{Config, Outline};
pub use position::Position;
pub use undo::{Bead, Undoer};

/// Read a `.leo` file and return its outline. No commander, no frame, no gui.
///
/// A `.leo` file stores only the outline's own nodes; the contents of `@file`,
/// `@clean` and friends live in the external files. Reading them is on by
/// default because otherwise this returns a shell. Pass `read_external =
/// false` when only the shape of the outline is wanted -- it is much faster.
pub fn open_outline(path: &str, read_external: bool) -> Result<Outline> {
    open_outline_with_report(path, read_external).map(|(o, _)| o)
}

/// `open_outline`, with what reading the external files reported.
///
/// A file that could not be read leaves its node as the `.leo` file described
/// it, which for an `@file` node is empty. A front end should say so, or the
/// empty node reads as the file's contents.
pub fn open_outline_with_report(
    path: &str,
    read_external: bool,
) -> Result<(Outline, external::ReadResult)> {
    let path = util::finalize(path);
    if !std::path::Path::new(&path).exists() {
        return Err(Error::NotFound { path });
    }
    let mut o = leofile::read_leo_file(&path)?;
    let report = match read_external {
        true => {
            let report = external::read_external_files(&mut o);
            // After the files, as Leo's `fc.readExternalFiles` does: the
            // nodes a blob names by position are the ones they just built.
            leofile::restore_descendent_uas(&mut o);
            report
        }
        false => external::ReadResult::default(),
    };
    o.changed = false;
    Ok((o, report))
}

/// Create an empty outline with a single node, and no view.
pub fn new_outline(file_name: &str) -> Outline {
    let mut o = Outline::new_empty();
    o.file_name = file_name.to_string();
    o
}

/// Write the outline to a `.leo` file and return the path written.
///
/// This writes the `.leo` file only. External `@file` nodes are a separate
/// concern; conflating them here would make a headless save touch the user's
/// source tree as a side effect.
pub fn save(o: &mut Outline, path: &str) -> Result<String> {
    leofile::write_leo_file(o, path)
}

/// What [`save_all`] did: the `.leo` file, then each changed external file.
#[derive(Debug)]
pub struct SaveResult {
    pub leo: Result<String>,
    /// Empty when the `.leo` write failed, as no file was attempted.
    pub files: external::WriteResult,
    /// Nodes whose descendants' unknown attributes this session dropped,
    /// by headline. An `@auto` tree's descendants are not in the `.leo`
    /// file, so their uAs live in a blob keyed by position that only Leo can
    /// rebuild; restructuring the tree leaves it naming other nodes, and this
    /// port drops it rather than let Leo restore those uAs onto them.
    pub dropped_descendent_uas: Vec<String>,
}

/// Save the `.leo` file, then write every dirty external file. Leo's `save`.
///
/// Leo writes the external files first. The `.leo` file goes first here, so
/// the outline's own edits reach disk however the files fare. A file that
/// fails does not stop the rest; it stays dirty and is retried on the next
/// save. [`save`] and [`write_external_files`] do each half.
///
/// If the `.leo` write fails, no file is written. `@nosent` and `@asis` files
/// are never read back, so a file newer than its `.leo` file would be shown
/// stale on reopen and overwritten by the next write of that tree.
pub fn save_all(o: &mut Outline, path: &str) -> SaveResult {
    let leo = leofile::write_leo_file(o, path);
    let files = match leo {
        Ok(_) => external::write_external_files(o, true),
        Err(_) => external::WriteResult::default(),
    };
    // Reported once: the blob is already gone, and holding the names would
    // repeat them on every later save.
    let dropped_descendent_uas = match leo {
        Ok(_) => std::mem::take(&mut o.dropped_descendent_uas),
        Err(_) => Vec::new(),
    };
    SaveResult {
        leo,
        files,
        dropped_descendent_uas,
    }
}

/// Write a copy of the outline to `path`, leaving its file name alone.
pub fn save_to(o: &mut Outline, path: &str) -> Result<String> {
    leofile::write_leo_copy(o, path)
}

/// Return the outline in `.leo` (XML) format.
pub fn to_xml(o: &mut Outline) -> String {
    leofile::outline_to_xml_string(o)
}

/// Read every `@file`, `@clean` and `@edit` tree in the outline.
pub fn read_external_files(o: &mut Outline) -> external::ReadResult {
    external::read_external_files(o)
}

/// Write every external file the outline owns. Returns what was written.
pub fn write_external_files(o: &mut Outline, dirty_only: bool) -> external::WriteResult {
    external::write_external_files(o, dirty_only)
}

/// The text of p's external file, without writing anything.
pub fn tangle(o: &Outline, p: &Position) -> Result<String> {
    atfile_write::tangle(o, p)
}
