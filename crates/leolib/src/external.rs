//! Reading and writing the external files an outline refers to.
//!
//! Selecting which nodes to read or write is its own job, separate from
//! parsing: `@ignore` is honoured, clones of one path are read once, and an
//! `@<file>` node's own kind decides how it is read. Using one reader for all
//! of them reports every non-sentinel file as invalid.

use std::collections::HashSet;

use crate::atclean;
use crate::atfile_read;
use crate::atfile_write;
use crate::langdata;
use crate::outline::Outline;
use crate::position::Position;
use crate::util;

/// What happened to one external file.
#[derive(Debug, Clone)]
pub struct FileReport {
    pub headline: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Default)]
pub struct ReadResult {
    pub read: usize,
    pub errors: Vec<FileReport>,
    pub ignored: Vec<String>,
    /// Files the reader normalized. Writing the node back changes the file on
    /// disk even if nobody edits it, so a front end should say so.
    pub warnings: Vec<FileReport>,
}

#[derive(Debug, Default)]
pub struct WriteResult {
    pub written: Vec<String>,
    pub unchanged: usize,
    pub errors: Vec<FileReport>,
    pub ignored: Vec<String>,
    /// Nodes refused by [`Outline::may_overwrite`], also listed in `errors`.
    pub refused: Vec<Position>,
}

/// The `@<file>` nodes to read, in outline order.
///
/// A clone referring to the same path is read once. `@asis` and `@nosent`
/// files cannot be updated from disk -- they carry no record of the tree, so
/// re-reading one would delete every child node -- so they are skipped.
pub fn find_files_to_read(o: &Outline, root: &Position, all: bool) -> (Vec<Position>, Vec<String>) {
    let mut scanned: HashSet<(String, String)> = HashSet::new();
    let mut files = Vec::new();
    let mut ignored = Vec::new();
    let after = if all { None } else { root.node_after_tree(o) };
    let mut p = Some(root.clone());
    while let Some(cur) = p {
        if Some(&cur) == after.as_ref() {
            break;
        }
        let key = (cur.gnx(o).to_string(), o.full_path(&cur));
        if scanned.contains(&key) {
            p = cur.node_after_tree(o);
            continue;
        }
        scanned.insert(key);
        if !cur.h(o).starts_with('@') {
            p = cur.thread_next(o);
        } else if cur.is_at_ignore_node(o) {
            if cur.is_any_at_file_node(o) {
                ignored.push(cur.h(o).to_string());
            }
            p = cur.node_after_tree(o);
        } else if cur.is_at_thin_file_node(o)
            || cur.is_at_auto_node(o)
            || cur.is_at_edit_node(o)
            || cur.is_at_shadow_file_node(o)
            || cur.is_at_file_node(o)
            || cur.is_at_clean_node(o)
            || cur.is_at_jupytext_node(o)
        {
            files.push(cur.clone());
            p = cur.node_after_tree(o);
        } else if cur.is_at_asis_node(o) || cur.is_at_nosent_node(o) {
            p = cur.node_after_tree(o);
        } else {
            p = cur.thread_next(o);
        }
    }
    (files, ignored)
}

/// Read every `@file`, `@clean` and `@edit` tree in the outline.
///
/// An error on one file must not abandon the rest: a `.leo` file routinely
/// refers to files that have moved or that this machine does not have. A
/// failure leaves the node as the `.leo` file described it, which is what Leo
/// does.
pub fn read_external_files(o: &mut Outline) -> ReadResult {
    let mut result = ReadResult::default();
    let Some(root) = o.root_position() else {
        return result;
    };
    let (files, ignored) = find_files_to_read(o, &root, true);
    result.ignored = ignored;
    for p in files {
        match read_file_at_position(o, &p) {
            Ok(true) => result.read += 1,
            Ok(false) => {}
            Err(message) => result.errors.push(FileReport {
                headline: p.h(o).to_string(),
                path: o.full_path(&p),
                message,
            }),
        }
        let gnx = p.gnx(o).to_string();
        if let Some(message) = o.import_warnings.remove(&gnx) {
            result.warnings.push(FileReport {
                headline: p.h(o).to_string(),
                path: o.full_path(&p),
                message,
            });
        }
    }
    for p in o.all_positions() {
        o.node_mut(p.v).clear_bit(crate::node::status::DIRTY);
    }
    result
}

/// Read the `@<file>` node at p, dispatching on its kind.
pub fn read_file_at_position(o: &mut Outline, p: &Position) -> Result<bool, String> {
    // An `@encoding` directive the writer cannot honour: see `read_file_to_string`.
    let encoding = o.get_encoding(p);
    if !encoding_is_supported(&encoding) {
        return Err(format!(
            "@encoding {encoding} is not supported; this port reads and writes UTF-8 only"
        ));
    }
    if p.is_at_auto_node(o) {
        return read_one_at_auto_node(o, p);
    }
    if p.is_at_clean_node(o) {
        return atclean::read_one_at_clean_node(o, p);
    }
    if p.is_at_edit_node(o) {
        return read_one_at_edit_node(o, p);
    }
    if p.is_at_file_node(o) || p.is_at_thin_file_node(o) || p.is_at_jupytext_node(o) {
        return read_at_file_node(o, p);
    }
    if p.is_at_shadow_file_node(o) {
        return Err("@shadow is deprecated and not supported".to_string());
    }
    Ok(false)
}

fn read_at_file_node(o: &mut Outline, p: &Position) -> Result<bool, String> {
    let path = o.full_path(p);
    let contents = read_file_to_string(&path)?;
    atfile_read::read_into_root(o, &contents, &path, p)?;
    o.remember_read_path(p, &path);
    o.clear_dirty_in_tree(p);
    Ok(true)
}

/// Read an `@auto` file, building a tree from the language's own structure.
///
/// The tree is checked before it is kept: an `@auto` file has no sentinels, so
/// the next write of the node reproduces the file from the tree alone. An
/// importer that dropped or reordered a line would therefore overwrite the
/// user's source. On failure the whole file goes into the node's body, which
/// is what Leo does when an importer raises.
fn read_one_at_auto_node(o: &mut Outline, p: &Position) -> Result<bool, String> {
    let path = o.full_path(p);
    let contents = read_file_to_string(&path)?;
    let report = crate::importers::import_string(o, p, &contents, &path)?;
    // An importer may normalize what it read. Say so: the file changes on the
    // next write even if nobody edits the outline.
    let mut notes: Vec<&str> = Vec::new();
    if report.regularized_whitespace {
        notes.push("leading whitespace was converted to match @tabwidth");
    }
    if report.text != contents.replace('\r', "") {
        notes.push("the text was reformatted by the importer");
    }
    if !notes.is_empty() {
        let gnx = p.gnx(o).to_string();
        o.import_warnings.insert(gnx, notes.join("; "));
    }
    if report.round_trips {
        let written = crate::importers::write_string(o, p, &path);
        let ok = match &written {
            Ok(text) => *text == report.text || *text == format!("{}\n", report.text),
            Err(_) => false,
        };
        if !ok {
            o.detach_subtree(p.v);
            o.node_mut(p.v).b = contents;
            // The body is the whole file, so writing it reproduces the file.
            o.remember_read_path(p, &path);
            let detail = written.err().unwrap_or_else(|| "text differs".to_string());
            return Err(format!(
                "the {} importer did not reproduce {}: {detail}. \
                 The whole file is in the node's body.",
                report.language,
                util::short_file_name(&path)
            ));
        }
    }
    o.remember_read_path(p, &path);
    o.clear_dirty_in_tree(p);
    Ok(true)
}

/// Read an `@edit` file: one node, no structure, prefixed by a language directive.
fn read_one_at_edit_node(o: &mut Outline, p: &Position) -> Result<bool, String> {
    let path = o.full_path(p);
    let contents = read_file_to_string(&path)?;
    let kids = p.children(o);
    for child in kids.iter().rev() {
        o.delete_position(child);
    }
    let (_, ext) = util::os_path_splitext(&path);
    let ext = ext.to_lowercase();
    let head = match ext.as_str() {
        ".html" | ".htm" => "@language html\n".to_string(),
        ".txt" | ".text" => "@nocolor\n".to_string(),
        _ => {
            let language = language_for_extension(&ext);
            if language != "unknown_language" {
                format!("@language {language}\n")
            } else {
                "@nocolor\n".to_string()
            }
        }
    };
    o.node_mut(p.v).b = format!("{head}{contents}");
    o.remember_read_path(p, &path);
    o.clear_dirty_in_tree(p);
    Ok(true)
}

/// Fill the new `@file` node at p from a file that may have no sentinels.
///
/// A file with sentinels is read as it is, as Leo's `importDerivedFiles` does.
/// Any other file is split by its `@auto` importer, or kept whole in p's body
/// when the tree would not write the file back unchanged. Returns true when
/// the node still has to be written to give the file its sentinels.
pub fn import_at_file(o: &mut Outline, p: &Position) -> Result<bool, String> {
    let path = o.full_path(p);
    let name = util::short_file_name(&path);
    let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
    // Sentinels written into a binary file would corrupt it.
    let Ok(contents) = std::str::from_utf8(&bytes) else {
        return Err(format!("{name} is not UTF-8 text"));
    };
    if contents.contains("@+leo-ver=") {
        read_at_file_node(o, p)?;
        return Ok(false);
    }
    let mut text = contents.trim_start_matches('\u{feff}').replace('\r', "");
    // The writer ends the file with a newline, so the node holds one too.
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    let split = crate::importers::import_string(o, p, &text, &path).is_ok()
        && mark_first_lines(o, p, &text)
        && reproduces(o, p, &text);
    if !split {
        o.detach_subtree(p.v);
        o.node_mut(p.v).b = text.clone();
        if !(mark_first_lines(o, p, &text) && reproduces(o, p, &text)) {
            return Err(format!(
                "{name} would not be written back unchanged as @file"
            ));
        }
    }
    o.set_dirty(p);
    Ok(true)
}

/// Put `@first` on a leading `#!` line and PEP 263 coding line, which must
/// stay on lines 1 and 2 rather than follow the sentinel header.
///
/// False if p's body does not start with them, as `@first` then cannot work.
fn mark_first_lines(o: &mut Outline, p: &Position, text: &str) -> bool {
    static CODING: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| regex::Regex::new(r"^[ \t\f]*#.*?coding[:=]").unwrap());
    let lines = util::split_lines(text);
    let mut n = usize::from(lines.first().is_some_and(|l| l.starts_with("#!")));
    if lines.get(n).is_some_and(|l| CODING.is_match(l)) {
        n += 1;
    }
    let body = o.node(p.v).b.clone();
    let Some(rest) = body.strip_prefix(lines[..n].concat().as_str()) else {
        return false;
    };
    let marked: String = lines[..n].iter().map(|l| format!("@first {l}")).collect();
    o.node_mut(p.v).b = format!("{marked}{rest}");
    true
}

/// True if p's tree, written without sentinels, is `text`.
fn reproduces(o: &Outline, p: &Position, text: &str) -> bool {
    match atfile_write::at_file_to_string(o, p, false) {
        Ok(w) => w == text,
        Err(_) => false,
    }
}

/// The language for a file extension, as `ic.languageForExtension`.
pub fn language_for_extension(ext: &str) -> String {
    let ext = ext.strip_prefix('.').unwrap_or(ext);
    if ext.is_empty() {
        return "unknown_language".to_string();
    }
    let language = match langdata::extra_extension_dict().get(ext) {
        Some(z) if *z != "none" && *z != "None" => Some(*z),
        _ => langdata::extension_dict().get(ext).copied(),
    };
    match language {
        Some(l) if l != "none" && l != "None" && !l.is_empty() => l.to_string(),
        _ => "unknown_language".to_string(),
    }
}

/// Read a file as text, refusing anything that is not UTF-8.
///
/// A leading byte-order mark is removed, as `g.stripBOM` does: it is an
/// encoding marker, not text, and leaving it in would put it in a node's body.
///
/// Leo decodes with the file's own encoding and encodes with it again on the
/// way out. This port writes UTF-8 and nothing else, so decoding anything else
/// would put text in the outline that the writer cannot put back: the write
/// would replace the file's own bytes with UTF-8 and lose every character the
/// two encodings spell differently. Refusing leaves the file alone instead --
/// the node keeps what the `.leo` file said, and [`Outline::may_overwrite`]
/// refuses the write, since nothing recorded the file as read.
pub fn read_file_to_string(path: &str) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let bytes = bytes
        .strip_prefix(&[0xEF, 0xBB, 0xBF][..])
        .unwrap_or(&bytes);
    String::from_utf8(bytes.to_vec()).map_err(|e| {
        format!(
            "{path}: not UTF-8 (byte {}); this port reads and writes UTF-8 only",
            e.utf8_error().valid_up_to()
        )
    })
}

/// True if `encoding` names UTF-8, or a subset of it.
///
/// The spellings are Python's aliases for utf-8, which is what an `@encoding`
/// directive and the `-encoding=` field of an `@+leo` header are written in.
/// ASCII is accepted because an ASCII file is the same bytes either way. An
/// empty name means nothing declared one, which leaves the default, utf-8.
pub fn encoding_is_supported(encoding: &str) -> bool {
    let e = encoding.trim().to_lowercase().replace('_', "-");
    matches!(
        e.as_str(),
        "" | "utf-8" | "utf8" | "utf" | "u8" | "cp65001" | "ascii" | "us-ascii" | "646"
    )
}

// --- Writing ------------------------------------------------------------

/// The `@<file>` nodes to write. With `dirty_only`, only those needing it.
pub fn find_files_to_write(o: &Outline, dirty_only: bool) -> (Vec<Position>, Vec<String>) {
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut files = Vec::new();
    let mut ignored = Vec::new();
    let mut p = o.root_position();
    while let Some(cur) = p {
        if !crate::node::find_at_file_name(cur.h(o), &["@leo"]).is_empty() {
            p = cur.node_after_tree(o);
        } else if cur.is_at_ignore_node(o) && !cur.is_at_asis_node(o) {
            // @ignore in body text is honoured, but not in an @asis node.
            if cur.is_any_at_file_node(o) {
                ignored.push(cur.h(o).to_string());
            }
            p = cur.node_after_tree(o);
        } else if cur.is_any_at_file_node(o) {
            let key = (cur.gnx(o).to_string(), o.full_path(&cur));
            if !seen.contains(&key) {
                seen.insert(key);
                files.push(cur.clone());
            }
            p = cur.node_after_tree(o);
        } else {
            p = cur.thread_next(o);
        }
    }
    if dirty_only {
        files.retain(|p| p.is_dirty(o));
    }
    (files, ignored)
}

/// Write every `@file`, `@clean`, `@edit`, `@asis` and `@nosent` tree to disk.
///
/// A file whose regenerated contents are unchanged is not touched at all, so
/// writing an outline nobody edited is a no-op down to the mtimes. The write
/// itself goes to a temporary file and is renamed over the target, so an
/// interrupted write cannot leave a half-written source file.
pub fn write_external_files(o: &mut Outline, dirty_only: bool) -> WriteResult {
    let (files, ignored) = find_files_to_write(o, dirty_only);
    let mut result = write_files(o, files);
    result.ignored = ignored;
    result
}

/// Write the given `@<file>` nodes, refusing any `may_overwrite` rejects.
pub fn write_files(o: &mut Outline, files: Vec<Position>) -> WriteResult {
    let mut result = WriteResult::default();
    for p in files {
        let path = o.full_path(&p);
        if !o.may_overwrite(&p) {
            // A file that is not UTF-8 is not offered for approval: approving
            // it would write UTF-8 over bytes this port could not read.
            let decodable = file_on_disk_is_utf8(&path);
            result.errors.push(FileReport {
                headline: p.h(o).to_string(),
                path,
                message: if decodable {
                    "refusing to overwrite a file this outline has not read".to_string()
                } else {
                    "the file on disk is not UTF-8; this port reads and writes UTF-8 only"
                        .to_string()
                },
            });
            if decodable {
                result.refused.push(p);
            }
            continue;
        }
        match file_contents(o, &p) {
            Err(message) => result.errors.push(FileReport {
                headline: p.h(o).to_string(),
                path,
                message,
            }),
            Ok((contents, newline, encoding)) => {
                let contents = if newline != "\n" {
                    contents.replace('\r', "").replace('\n', &newline)
                } else {
                    contents
                };
                match replace_file(&path, &contents, &encoding) {
                    Ok(true) => {
                        result.written.push(path.clone());
                        o.remember_read_path(&p, &path);
                        o.clear_dirty_in_tree(&p);
                        if p.is_at_clean_node(o) {
                            // The mod time is what stops the next read from
                            // treating our own write as an external edit.
                            if let Some(t) = file_mtime(&path) {
                                let gnx = p.gnx(o).to_string();
                                o.mod_time_cache.insert(gnx, t);
                            }
                        }
                    }
                    Ok(false) => {
                        result.unchanged += 1;
                        o.remember_read_path(&p, &path);
                        o.clear_dirty_in_tree(&p);
                    }
                    Err(message) => result.errors.push(FileReport {
                        headline: p.h(o).to_string(),
                        path,
                        message,
                    }),
                }
            }
        }
    }
    result
}

/// The text of p's external file, its line ending and its encoding.
///
/// The dispatch on node kind is the point: `@edit` is a body with its
/// directives removed, `@asis` is the tree's text verbatim, and only the rest
/// go through the sentinel writer.
pub fn file_contents(o: &Outline, p: &Position) -> Result<(String, String, String), String> {
    if p.is_at_shadow_file_node(o) {
        return Err("@shadow is deprecated and not supported".to_string());
    }
    let at = atfile_write::AtWrite::new(o, p);
    let newline = at.output_newline.clone();
    let encoding = at.encoding().to_string();
    // The read refuses these, but `@nosent` is never read and `@clean` is
    // exempt from `may_overwrite`, so the write needs its own guard: writing
    // UTF-8 over a file the directive says is not UTF-8 changes its bytes.
    if !encoding_is_supported(&encoding) {
        return Err(format!(
            "@encoding {encoding} is not supported; this port reads and writes UTF-8 only"
        ));
    }
    if p.is_at_auto_node(o) {
        let path = o.full_path(p);
        return Ok((
            crate::importers::write_string(o, p, &path)?,
            newline,
            encoding,
        ));
    }
    if p.is_at_asis_node(o) {
        return Ok((write_asis(o, p), newline, encoding));
    }
    if p.is_at_edit_node(o) {
        return Ok((write_at_edit(o, p), newline, encoding));
    }
    let sentinels = !(p.is_at_clean_node(o) || p.is_at_nosent_node(o));
    let contents = atfile_write::at_file_to_string(o, p, sentinels)?;
    Ok((contents, newline, encoding))
}

/// An `@asis` file is the tree's text with nothing added.
fn write_asis(o: &Outline, root: &Position) -> String {
    let mut out = String::new();
    for p in root.self_and_subtree(o) {
        let h = p.h(o);
        if let Some(rest) = h.strip_prefix("@@") {
            if !rest.is_empty() {
                out.push('\n');
                out.push_str(rest);
                out.push('\n');
            }
        }
        out.push_str(p.b(o));
    }
    out
}

/// An `@edit` file is one node's body with its directive lines removed.
fn write_at_edit(o: &Outline, p: &Position) -> String {
    let at = atfile_write::AtWrite::new(o, p);
    util::split_lines(p.b(o))
        .into_iter()
        .filter(|line| !at.is_directive_line(line))
        .collect::<Vec<_>>()
        .concat()
}

/// True if the file is UTF-8, or is not there at all.
fn file_on_disk_is_utf8(path: &str) -> bool {
    match std::fs::read(path) {
        Ok(bytes) => std::str::from_utf8(&bytes).is_ok(),
        Err(_) => true,
    }
}

fn file_mtime(path: &str) -> Option<u64> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// Write `contents` to `path`. Returns whether the file on disk changed.
///
/// The comparison is what makes a save of an untouched outline free. The
/// write is to a sibling temporary file, renamed into place: a crash midway
/// leaves the original intact rather than a truncated source file.
///
/// The temporary file takes the original's permissions before the rename,
/// or rewriting an executable script would leave it without `+x`.
pub fn replace_file(path: &str, contents: &str, _encoding: &str) -> Result<bool, String> {
    if let Ok(old) = std::fs::read(path) {
        if old == contents.as_bytes() {
            return Ok(false);
        }
        // The last guard, for the paths that reach here without a read:
        // `@clean` and `@nosent` are exempt from `may_overwrite`, and a front
        // end can approve an overwrite. Replacing bytes this port cannot
        // decode with UTF-8 loses every character the two spell differently.
        if std::str::from_utf8(&old).is_err() {
            return Err(format!(
                "{path}: the file on disk is not UTF-8; this port reads and writes UTF-8 only"
            ));
        }
    }
    let dir = util::os_path_dirname(path);
    if !dir.is_empty() && !std::path::Path::new(&dir).exists() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{dir}: {e}"))?;
    }
    let tmp = format!("{path}.leo-rs-tmp");
    std::fs::write(&tmp, contents.as_bytes()).map_err(|e| format!("{tmp}: {e}"))?;
    let finish = || -> Result<(), String> {
        if let Ok(meta) = std::fs::metadata(path) {
            std::fs::set_permissions(&tmp, meta.permissions())
                .map_err(|e| format!("{tmp}: {e}"))?;
        }
        std::fs::rename(&tmp, path).map_err(|e| format!("{path}: {e}"))
    };
    if let Err(e) = finish() {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_utf8_aliases_are_supported_and_nothing_else_is() {
        for name in ["", "utf-8", "UTF-8", "utf8", "u8", "ascii", "us-ascii"] {
            assert!(encoding_is_supported(name), "{name}");
        }
        for name in ["latin-1", "iso-8859-1", "cp1252", "utf-16", "shift-jis"] {
            assert!(!encoding_is_supported(name), "{name}");
        }
    }
}
