//! Reading `@clean` files: the Mulder/Ripley update algorithm.
//!
//! An `@clean` file carries no sentinels, so nothing in it says which node a
//! line belongs to. The outline is the only record of that. Reading one works
//! by writing the tree out *with* sentinels, diffing the file against the
//! sentinel-free version of that text, and threading the old sentinels back
//! through the diff. The result is a file that looks like an `@file`, which
//! the ordinary sentinel reader then parses.
//!
//! The algorithm never deletes or rearranges sentinels; only `@verbatim`
//! sentinels are inserted or dropped as the text requires.

use once_cell::sync::Lazy;
use regex::Regex;

use crate::atfile_read;
use crate::atfile_write;
use crate::error::{Error, Result};
use crate::outline::Outline;
use crate::position::Position;
use crate::seqmatch::{SequenceMatcher, Tag};
use crate::util;

/// The comment delimiters a file's sentinels are written with.
pub struct Marker {
    /// The single-line delimiter, or "" when the file uses block comments.
    pub delim1: String,
    pub delim2: String,
    pub delim3: String,
}

impl Marker {
    /// The delimiters from a file's `@+leo` line.
    pub fn from_file_lines(lines: &[String]) -> Marker {
        let leo_line = lines
            .iter()
            .find(|s| s.contains("@+leo"))
            .cloned()
            .unwrap_or_default();
        let (start, end) = parse_leo_sentinel(&leo_line);
        if end.is_empty() {
            Marker {
                delim1: start,
                delim2: String::new(),
                delim3: String::new(),
            }
        } else {
            Marker {
                delim1: String::new(),
                delim2: start,
                delim3: end,
            }
        }
    }

    /// The pair of delimiters a sentinel line is written with.
    pub fn delims(&self) -> (String, String) {
        if self.delim1.is_empty() {
            (self.delim2.clone(), self.delim3.clone())
        } else {
            (self.delim1.clone(), String::new())
        }
    }

    pub fn is_sentinel(&self, s: &str) -> bool {
        self.is_sentinel_with(s, "")
    }

    fn is_sentinel_with(&self, s: &str, suffix: &str) -> bool {
        let s = s.trim();
        if !self.delim1.is_empty() && s.starts_with(&self.delim1) {
            return s.starts_with(&format!("{}@{suffix}", self.delim1));
        }
        if !self.delim2.is_empty() {
            return s.starts_with(&format!("{}@{suffix}", self.delim2))
                && s.ends_with(&self.delim3);
        }
        false
    }

    pub fn is_verbatim_sentinel(&self, s: &str) -> bool {
        self.is_sentinel_with(s, "verbatim")
    }
}

/// The opening and closing delimiters of an `@+leo` line.
///
/// The same pattern Leo's `at.parseLeoSentinel` uses, so a file written by
/// either implementation reads the same way.
fn parse_leo_sentinel(s: &str) -> (String, String) {
    static PATTERN: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"(.+)@\+leo(-ver=([0-9]+))?(-thin)?(-encoding=(.*)(\.))?(.*)").unwrap()
    });
    match PATTERN.captures(s) {
        Some(m) => (
            m.get(1).map(|x| x.as_str()).unwrap_or("").to_string(),
            m.get(8).map(|x| x.as_str()).unwrap_or("").to_string(),
        ),
        None => (String::new(), String::new()),
    }
}

/// Update the `@clean` node at `root` from its file on disk.
pub fn read_one_at_clean_node(o: &mut Outline, root: &Position) -> Result<bool> {
    let path = o.full_path(root);
    if !std::path::Path::new(&path).exists() {
        return Err(Error::NotFound { path });
    }
    // #4385: do nothing if the file has not changed since we last saw it.
    let new_mod_time = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    let gnx = root.gnx(o).to_string();
    if let (Some(old), Some(new)) = (o.mod_time_cache.get(&gnx).copied(), new_mod_time) {
        if old >= new {
            return Ok(false);
        }
    }
    if let Some(t) = new_mod_time {
        o.mod_time_cache.insert(gnx, t);
    }

    let contents = crate::external::read_file_to_string(&path)?.replace("\r\n", "\n");
    let new_public_lines = util::split_lines(&contents);
    let private = atfile_write::at_file_to_string(o, root, true)?;
    let old_private_lines = util::split_lines(&private);
    let marker = Marker::from_file_lines(&old_private_lines);
    let (old_public_lines, _) = separate_sentinels(&old_private_lines, &marker);
    if old_public_lines.is_empty() {
        // Nothing to thread sentinels through: the file becomes one node.
        o.set_body(root, &new_public_lines.concat());
        return Ok(true);
    }
    let new_private_lines = propagate_changed_lines(&new_public_lines, &old_private_lines, &marker);
    if new_private_lines == old_private_lines {
        return Ok(false);
    }
    let text = new_private_lines.concat();
    atfile_read::read_into_root(o, &text, &path, root)?;
    Ok(true)
}

/// Split lines into (regular, sentinel).
///
/// A line preceded by an `@verbatim` sentinel is regular, and the sentinel
/// itself is dropped: keeping it would make the sentinel test disagree with
/// the text once the user adds or removes a line that needs one.
pub fn separate_sentinels(lines: &[String], marker: &Marker) -> (Vec<String>, Vec<String>) {
    let mut regular = Vec::new();
    let mut sentinels = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        let line = &lines[i];
        if marker.is_verbatim_sentinel(line) {
            i += 1;
            if i < lines.len() {
                regular.push(lines[i].clone());
            }
        } else if marker.is_sentinel(line) {
            sentinels.push(line.clone());
        } else {
            regular.push(line.clone());
        }
        i += 1;
    }
    (regular, sentinels)
}

/// The sentinels preceding each non-sentinel line, plus the trailing ones.
fn init_data(lines: &[String], marker: &Marker) -> (Vec<Vec<String>>, Vec<String>, Vec<String>) {
    let mut sentinels: Vec<String> = Vec::new();
    let mut per_line: Vec<Vec<String>> = Vec::new();
    let mut new_lines: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        let line = &lines[i];
        i += 1;
        if marker.is_verbatim_sentinel(line) {
            // The @verbatim sentinel itself is not carried through.
            if i < lines.len() {
                let line = lines[i].clone();
                i += 1;
                per_line.push(std::mem::take(&mut sentinels));
                new_lines.push(line);
            }
        } else if marker.is_sentinel(line) {
            sentinels.push(line.clone());
        } else {
            per_line.push(std::mem::take(&mut sentinels));
            new_lines.push(line.clone());
        }
    }
    (per_line, sentinels, new_lines)
}

fn preprocess(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .map(|l| {
            if l.ends_with('\n') {
                l.clone()
            } else {
                format!("{l}\n")
            }
        })
        .collect()
}

/// Thread the old file's sentinels through the diff of the public lines.
pub fn propagate_changed_lines(
    new_public_lines: &[String],
    old_private_lines: &[String],
    marker: &Marker,
) -> Vec<String> {
    let (sentinels, trailing, old_public_lines) = init_data(old_private_lines, marker);
    let a = preprocess(&old_public_lines);
    let b = preprocess(new_public_lines);
    let (d1, d2) = marker.delims();
    let verbatim_line = format!("{d1}@verbatim{d2}\n");
    let mut results: Vec<String> = Vec::new();

    // The leading sentinels come first, before any diff opcode, so that an
    // insertion at the top of the file lands after them rather than before.
    let mut sentinels = sentinels;
    if !sentinels.is_empty() {
        results.extend(std::mem::take(&mut sentinels[0]));
    }
    let put_sentinels = |results: &mut Vec<String>, i: usize| {
        if let Some(s) = sentinels.get(i) {
            results.extend(s.iter().cloned());
        }
    };
    let put_plain_line = |results: &mut Vec<String>, line: &str| {
        // A plain line that looks like a sentinel needs one of its own.
        if marker.is_sentinel(line) {
            results.push(verbatim_line.clone());
        }
        results.push(line.to_string());
    };

    let sm = SequenceMatcher::new(&a, &b);
    for op in sm.opcodes() {
        match op.tag {
            Tag::Delete => {
                for i in op.ai..op.aj {
                    put_sentinels(&mut results, i);
                }
            }
            Tag::Equal => {
                // The index addresses both the old lines and their sentinels.
                #[allow(clippy::needless_range_loop)]
                for i in op.ai..op.aj {
                    put_sentinels(&mut results, i);
                    put_plain_line(&mut results, &a[i]);
                }
            }
            Tag::Insert => {
                // Sentinels go *after* inserted lines, which is why the
                // leading block was emitted before the loop.
                for line in &b[op.bi..op.bj] {
                    put_plain_line(&mut results, line);
                }
            }
            Tag::Replace => {
                let mut b_lines: Vec<&String> = b[op.bi..op.bj].iter().collect();
                let mut k = 0usize;
                for i in op.ai..op.aj {
                    put_sentinels(&mut results, i);
                    if k < b_lines.len() {
                        put_plain_line(&mut results, b_lines[k]);
                        k += 1;
                    }
                }
                while k < b_lines.len() {
                    put_plain_line(&mut results, b_lines[k]);
                    k += 1;
                }
                b_lines.clear();
            }
        }
    }
    results.extend(trailing);
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Outline;

    fn at_clean_outline() -> Outline {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "@clean test.py");
        o.set_body(&root, "@others\n");
        let a = o.insert_as_last_child(&root);
        o.set_headline(&a, "one");
        o.set_body(&a, "a = 1\n");
        let b = o.insert_as_last_child(&root);
        o.set_headline(&b, "two");
        o.set_body(&b, "b = 2\n");
        o
    }

    #[test]
    fn an_unchanged_file_produces_the_same_private_lines() {
        let o = at_clean_outline();
        let root = o.root_position().unwrap();
        let private = atfile_write::at_file_to_string(&o, &root, true).unwrap();
        let old = util::split_lines(&private);
        let marker = Marker::from_file_lines(&old);
        let (public, _) = separate_sentinels(&old, &marker);
        let result = propagate_changed_lines(&public, &old, &marker);
        assert_eq!(result, old);
    }

    #[test]
    fn an_edited_line_lands_in_the_right_node() {
        let mut o = at_clean_outline();
        let root = o.root_position().unwrap();
        let public = vec!["a = 111\n".to_string(), "b = 2\n".to_string()];
        let private = atfile_write::at_file_to_string(&o, &root, true).unwrap();
        let old = util::split_lines(&private);
        let marker = Marker::from_file_lines(&old);
        let new_private = propagate_changed_lines(&public, &old, &marker);
        assert!(
            atfile_read::read_into_root(&mut o, &new_private.concat(), "test.py", &root).is_ok()
        );
        let kids = root.children(&o);
        assert_eq!(kids[0].b(&o), "a = 111\n");
        assert_eq!(kids[1].b(&o), "b = 2\n");
    }

    #[test]
    fn a_line_added_to_a_node_stays_in_that_node() {
        let mut o = at_clean_outline();
        let root = o.root_position().unwrap();
        let public = vec![
            "a = 1\n".to_string(),
            "a2 = 1\n".to_string(),
            "b = 2\n".to_string(),
        ];
        let private = atfile_write::at_file_to_string(&o, &root, true).unwrap();
        let old = util::split_lines(&private);
        let marker = Marker::from_file_lines(&old);
        let new_private = propagate_changed_lines(&public, &old, &marker);
        assert!(
            atfile_read::read_into_root(&mut o, &new_private.concat(), "test.py", &root).is_ok()
        );
        let kids = root.children(&o);
        assert_eq!(kids[0].b(&o), "a = 1\na2 = 1\n");
        assert_eq!(kids[1].b(&o), "b = 2\n");
    }

    #[test]
    fn the_leo_sentinel_gives_up_its_delimiters() {
        let (start, end) = parse_leo_sentinel("# @+leo-ver=5-thin\n");
        assert_eq!((start.as_str(), end.as_str()), ("# ", ""));
        let (start, end) = parse_leo_sentinel("/*@+leo-ver=5-thin*/\n");
        assert_eq!((start.as_str(), end.as_str()), ("/*", "*/"));
    }
}
