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
}

#[derive(Debug, Default)]
pub struct WriteResult {
    pub written: Vec<String>,
    pub unchanged: usize,
    pub errors: Vec<FileReport>,
    pub ignored: Vec<String>,
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
    }
    for p in o.all_positions() {
        o.node_mut(p.v).clear_bit(crate::node::status::DIRTY);
    }
    result
}

/// Read the `@<file>` node at p, dispatching on its kind.
pub fn read_file_at_position(o: &mut Outline, p: &Position) -> Result<bool, String> {
    if p.is_at_auto_node(o) {
        // An @auto file has no sentinels, so its structure comes from one of
        // Leo's 34 language importers. None is ported, and guessing a
        // structure would silently rewrite the user's tree, so the node is
        // left exactly as the .leo file describes it.
        return Err("@auto is not supported: no language importers".to_string());
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
    o.remember_read_path(p, &path);
    if !atfile_read::read_into_root(o, &contents, &path, p) {
        return Err(format!("not a valid external file: {path}"));
    }
    o.clear_dirty_in_tree(p);
    Ok(true)
}

/// Read an `@edit` file: one node, no structure, prefixed by a language directive.
fn read_one_at_edit_node(o: &mut Outline, p: &Position) -> Result<bool, String> {
    let path = o.full_path(p);
    let contents = read_file_to_string(&path)?;
    o.remember_read_path(p, &path);
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
    o.clear_dirty_in_tree(p);
    Ok(true)
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

/// Read a file as text. Bytes that are not UTF-8 are replaced, never rejected.
pub fn read_file_to_string(path: &str) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
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
    let mut result = WriteResult::default();
    let (files, ignored) = find_files_to_write(o, dirty_only);
    result.ignored = ignored;
    for p in files {
        let path = o.full_path(&p);
        if !o.may_overwrite(&p) {
            result.errors.push(FileReport {
                headline: p.h(o).to_string(),
                path,
                message: "refusing to overwrite a file this outline has not read".to_string(),
            });
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
    if p.is_at_auto_node(o) {
        return Err("@auto is not supported: no language writers".to_string());
    }
    if p.is_at_shadow_file_node(o) {
        return Err("@shadow is deprecated and not supported".to_string());
    }
    let at = atfile_write::AtWrite::new(o, p);
    let newline = at.output_newline.clone();
    let encoding = at.encoding().to_string();
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
pub fn replace_file(path: &str, contents: &str, _encoding: &str) -> Result<bool, String> {
    if let Ok(old) = std::fs::read(path) {
        if old == contents.as_bytes() {
            return Ok(false);
        }
    }
    let dir = util::os_path_dirname(path);
    if !dir.is_empty() && !std::path::Path::new(&dir).exists() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{dir}: {e}"))?;
    }
    let tmp = format!("{path}.leo-rs-tmp");
    std::fs::write(&tmp, contents.as_bytes()).map_err(|e| format!("{tmp}: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{path}: {e}"))?;
    Ok(true)
}
