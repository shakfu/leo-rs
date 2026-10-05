//! Leo's `goto-global-line` and `show-file-line`: `gotoCommands.py`.
//!
//! Both read one map: each line of an external file, and the node and body
//! row it comes from. The file is written with sentinels to make the map, so
//! an `@clean` file maps as well as an `@file` one.

use crate::atclean::Marker;
use crate::outline::Outline;
use crate::position::Position;
use crate::util;

/// The `@<file>` node p's text is written by, as Leo's `find_root`: the
/// nearest above p that is not `@all`, else one above another of p's clones.
pub fn find_root(o: &Outline, p: &Position) -> Option<Position> {
    let above = |q: &Position| {
        q.self_and_parents(o)
            .into_iter()
            .find(|a| !a.is_at_all_node(o) && a.is_any_at_file_node(o))
    };
    above(p).or_else(|| {
        o.all_positions()
            .iter()
            .filter(|q| q.v == p.v && *q != p)
            .find_map(above)
    })
}

/// Whether line numbers mean anything for root's file: Leo's step 0.
fn is_mappable(o: &Outline, root: &Position) -> bool {
    root.is_at_clean_node(o)
        || root.is_at_edit_node(o)
        || root.is_at_file_node(o)
        || (root.is_at_asis_node(o) && !root.has_children(o))
}

/// For each line of root's file, the gnx of its node and its 1-based offset
/// in that node, as Leo's `scan_sentinel_lines` and `scan_nonsentinel_lines`
/// count it. Only an `@file` file holds its sentinels, so only there do they
/// take a line.
fn line_map(o: &Outline, root: &Position) -> Option<Vec<(String, usize)>> {
    let text = crate::atfile_write::at_file_to_string(o, root, true).ok()?;
    let rows = row_map(&text, root.gnx(o), root.is_at_file_node(o));
    Some(
        rows.into_iter()
            .map(|(gnx, offset, _)| (gnx, offset))
            .collect(),
    )
}

/// `line_map`'s walk over `text`, a file written with sentinels: each line's
/// gnx, offset, and whether it is a sentinel. `sentinels_count` keeps the
/// sentinel lines, for a file that holds them.
fn row_map(text: &str, root_gnx: &str, sentinels_count: bool) -> Vec<(String, usize, bool)> {
    let lines = util::split_lines(text);
    let marker = Marker::from_file_lines(&lines);
    let delim = marker.delims().0.trim_end().to_string();
    let (mut gnx, mut offset) = (root_gnx.to_string(), 0usize);
    let mut stack: Vec<(String, usize)> = Vec::new();
    let mut map = Vec::new();
    let mut verbatim = false;
    for line in &lines {
        let sentinel = !verbatim && marker.is_sentinel(line);
        // Leo counts an `@verbatim` sentinel as a body line, which puts every
        // later row of the node off by one. It is no line of the body.
        verbatim = sentinel && marker.is_verbatim_sentinel(line);
        if verbatim {
            if sentinels_count {
                map.push((gnx.clone(), offset, true));
            }
            continue;
        }
        if sentinel {
            let s = line.trim().strip_prefix(delim.as_str()).unwrap_or("");
            let s = s.strip_prefix(' ').unwrap_or(s);
            if s.starts_with("@+node") {
                offset = 0;
                gnx = node_gnx(line);
            } else if s.starts_with("@+others") || s.starts_with("@+<<") {
                stack.push((gnx.clone(), offset));
                offset += 1;
            } else if s.starts_with("@-others") || s.starts_with("@-<<") {
                (gnx, offset) = stack.pop().unwrap_or_default();
                offset += 1;
            } else {
                offset += 1;
            }
        } else {
            offset += 1;
        }
        if sentinels_count || !sentinel {
            map.push((gnx.clone(), offset, sentinel));
        }
    }
    map
}

/// One line of an external file, and the body row that writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileLine {
    /// The node whose body writes the line.
    pub gnx: String,
    /// 0-based row in that node's body.
    pub row: usize,
    /// Bytes the writer put before the row's text, an `@others` indent. None
    /// when the line is not the row's text: a sentinel, a directive, a doc
    /// part written as a comment.
    pub indent: Option<usize>,
}

/// An external file as written, with the body row behind each line. A
/// language server reads `text`; a front end maps positions through `lines`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineMap {
    /// The file's text, with `\n` line ends.
    pub text: String,
    /// One per line of `text`.
    pub lines: Vec<FileLine>,
}

impl LineMap {
    /// The 0-based line body row `row` of node `gnx` is written to, the first
    /// if a clone writes it twice. None if the row writes no line of its own.
    pub fn line_of(&self, gnx: &str, row: usize) -> Option<usize> {
        self.lines
            .iter()
            .position(|l| l.gnx == gnx && l.row == row && l.indent.is_some())
    }
}

/// root's file as it would be written, mapped line by line to body rows.
///
/// Every file the sentinel writer writes maps: `@file`, `@clean`, `@nosent`
/// and an `@auto` file of code. The map is read from the file written with
/// sentinels, so a file written any other way -- `@edit`, `@asis`, `@auto` of
/// markdown or org -- is None rather than mapped by guesswork: the check is
/// that the sentinel writer, with sentinels as the file has them, gives the
/// same text.
pub fn line_map_of(o: &Outline, root: &Position) -> Option<LineMap> {
    let (text, _, _) = crate::external::file_contents(o, root).ok()?;
    let auto = root.is_at_auto_node(o);
    let sentinels = !(root.is_at_clean_node(o) || root.is_at_nosent_node(o) || auto);
    let write = |sentinels| crate::atfile_write::write_to_string(o, root, sentinels, auto).ok();
    if write(sentinels)? != text {
        return None;
    }
    let rows = row_map(&write(true)?, root.gnx(o), sentinels);
    let file_lines = util::split_lines(&text);
    if rows.len() != file_lines.len() {
        return None;
    }
    let mut bodies: std::collections::HashMap<String, Vec<String>> = Default::default();
    let positions = root.self_and_subtree(o);
    let lines = rows
        .into_iter()
        .zip(&file_lines)
        .map(|((gnx, offset, sentinel), file_line)| {
            let body = bodies.entry(gnx.clone()).or_insert_with(|| {
                positions
                    .iter()
                    .find(|p| p.gnx(o) == gnx)
                    .map_or_else(Vec::new, |p| util::split_lines(p.b(o)))
            });
            let row = offset.saturating_sub(1);
            let indent = match sentinel {
                true => None,
                false => body.get(row).and_then(|b| indent_of(file_line, b)),
            };
            FileLine { gnx, row, indent }
        })
        .collect();
    Some(LineMap { text, lines })
}

/// The body of p, a node in no file, as a language server reads it: each
/// Leo directive line blanked, so its line numbers stay the body's rows.
pub fn body_as_code(o: &Outline, p: &Position) -> String {
    let at = crate::atfile_write::AtWrite::new(o, p);
    util::split_lines(p.b(o))
        .iter()
        .map(|line| match at.is_directive_line(line) {
            true => "\n",
            false => line.as_str(),
        })
        .collect()
}

/// The blanks before `body` in `file`, if `file` is `body` behind blanks.
fn indent_of(file: &str, body: &str) -> Option<usize> {
    let (file, body) = (file.trim_end_matches('\n'), body.trim_end_matches('\n'));
    let prefix = file.strip_suffix(body)?;
    prefix
        .chars()
        .all(|c| c == ' ' || c == '\t')
        .then_some(prefix.len())
}

/// The gnx of an `@+node` sentinel: between its first two colons.
fn node_gnx(line: &str) -> String {
    let mut parts = line.splitn(3, ':');
    parts.next();
    parts.next().unwrap_or("").to_string()
}

/// Leo's `goto-global-line`: the node and body row of line `n` (1-based) of
/// the file p belongs to. `None` if p is in no file whose lines map, or the
/// file has no line `n`.
pub fn find_file_line(o: &Outline, p: &Position, n: usize) -> Option<(Position, usize)> {
    let root = find_root(o, p)?;
    if !is_mappable(o, &root) {
        return None;
    }
    let (gnx, offset) = line_map(o, &root)?.get(n.checked_sub(1)?)?.clone();
    // p itself if it is the node, as Leo's `find_gnx2` prefers `c.p`; else
    // the last position of that node.
    let found = match p.gnx(o) == gnx {
        true => p.clone(),
        false => o
            .all_positions()
            .into_iter()
            .rev()
            .find(|q| q.gnx(o) == gnx)?,
    };
    Some((found, offset.saturating_sub(1)))
}

/// The reverse: the line (1-based) of p's file that body row `row` of p is
/// written to. `None` if that row writes no line, as a directive does in an
/// `@clean` file.
///
/// Leo's `show-file-line` adds the row to the line of the node's first row,
/// which is wrong for every row after an `@others` or a section reference.
pub fn file_line(o: &Outline, p: &Position, row: usize) -> Option<usize> {
    let root = find_root(o, p)?;
    if !is_mappable(o, &root) {
        return None;
    }
    let gnx = p.gnx(o);
    let map = line_map(o, &root)?;
    map.iter()
        .position(|(g, offset)| g == gnx && *offset == row + 1)
        .map(|i| i + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `@file x.py` (or `kind`) with a child `f` under `@others`.
    fn outline(kind: &str) -> (Outline, Position, Position) {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, &format!("{kind} x.py"));
        o.set_body(&root, "import os\n@others\nx = 1\n");
        let f = o.insert_as_last_child(&root);
        o.set_headline(&f, "f");
        o.set_body(&f, "def f():\n    return 1\n");
        (o, root, f)
    }

    #[test]
    fn a_sentinel_file_counts_its_sentinels_as_lines() {
        let (o, root, f) = outline("@file");
        let text = crate::atfile_write::at_file_to_string(&o, &root, true).unwrap();
        let lines = util::split_lines(&text);
        let at = |needle: &str| lines.iter().position(|l| l.starts_with(needle)).unwrap() + 1;
        assert_eq!(
            find_file_line(&o, &root, at("import os")),
            Some((root.clone(), 0))
        );
        assert_eq!(
            find_file_line(&o, &root, at("    return 1")),
            Some((f.clone(), 1))
        );
        assert_eq!(
            find_file_line(&o, &root, at("x = 1")),
            Some((root.clone(), 2))
        );
        assert_eq!(file_line(&o, &f, 1), Some(at("    return 1")));
        assert_eq!(file_line(&o, &root, 2), Some(at("x = 1")));
        assert_eq!(find_file_line(&o, &root, lines.len() + 1), None);
    }

    #[test]
    fn a_clean_file_counts_only_its_own_lines() {
        let (o, root, f) = outline("@clean");
        // import os / def f(): / return 1 / x = 1
        assert_eq!(find_file_line(&o, &root, 1), Some((root.clone(), 0)));
        assert_eq!(find_file_line(&o, &root, 3), Some((f.clone(), 1)));
        assert_eq!(find_file_line(&o, &f, 4), Some((root.clone(), 2)));
        assert_eq!(file_line(&o, &f, 0), Some(2));
        assert_eq!(file_line(&o, &root, 2), Some(4));
        // `@others` writes no line of its own.
        assert_eq!(file_line(&o, &root, 1), None);
    }

    #[test]
    fn a_file_maps_each_line_to_a_row_and_its_indent() {
        let (mut o, root, f) = outline("@file");
        o.set_body(&root, "import os\n    @others\nx = 1\n");
        let map = line_map_of(&o, &root).unwrap();
        assert_eq!(
            map.text,
            crate::atfile_write::at_file_to_string(&o, &root, true).unwrap()
        );
        let at = |needle: &str| map.text.lines().position(|l| l.contains(needle)).unwrap();
        let line = |n: usize| map.lines[n].clone();
        assert_eq!(line(at("import os")).row, 0);
        assert_eq!(line(at("import os")).indent, Some(0));
        let ret = line(at("return 1"));
        assert_eq!(
            (ret.gnx.as_str(), ret.row, ret.indent),
            (f.gnx(&o), 1, Some(4))
        );
        // A sentinel is a line of the file but of no body.
        assert_eq!(line(at("@+others")).indent, None);
        assert_eq!(map.line_of(f.gnx(&o), 1), Some(at("return 1")));
        assert_eq!(map.line_of(root.gnx(&o), 1), None);
    }

    #[test]
    fn a_clean_file_and_a_code_auto_file_map_their_own_lines() {
        for kind in ["@clean", "@auto"] {
            let (o, root, f) = outline(kind);
            let map = line_map_of(&o, &root).unwrap();
            assert_eq!(
                map.text, "import os\ndef f():\n    return 1\nx = 1\n",
                "{kind}"
            );
            let rows: Vec<(bool, usize, Option<usize>)> = map
                .lines
                .iter()
                .map(|l| (l.gnx == f.gnx(&o), l.row, l.indent))
                .collect();
            assert_eq!(
                rows,
                [
                    (false, 0, Some(0)),
                    (true, 0, Some(0)),
                    (true, 1, Some(0)),
                    (false, 2, Some(0))
                ],
                "{kind}"
            );
        }
    }

    #[test]
    fn a_file_the_sentinel_writer_does_not_write_has_no_line_map() {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "@auto notes.md");
        o.set_body(&root, "# Title\n");
        let child = o.insert_as_last_child(&root);
        o.set_headline(&child, "Section");
        o.set_body(&child, "text\n");
        assert_eq!(line_map_of(&o, &root), None);
    }

    #[test]
    fn a_body_as_code_blanks_its_directives() {
        let mut o = Outline::new_empty();
        let p = o.root_position().unwrap();
        o.set_body(&p, "@language python\nx = 1\n@tabwidth -4\n");
        assert_eq!(body_as_code(&o, &p), "\nx = 1\n\n");
    }

    #[test]
    fn a_node_in_no_mappable_file_has_no_lines() {
        let (o, root, _) = outline("@nosent");
        assert_eq!(find_file_line(&o, &root, 1), None);
        let o = Outline::new_empty();
        let p = o.root_position().unwrap();
        assert_eq!(file_line(&o, &p, 0), None);
    }
}
