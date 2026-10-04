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
    let lines = util::split_lines(&text);
    let marker = Marker::from_file_lines(&lines);
    let delim = marker.delims().0.trim_end().to_string();
    let sentinels_count = root.is_at_file_node(o);
    let (mut gnx, mut offset) = (root.gnx(o).to_string(), 0usize);
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
                map.push((gnx.clone(), offset));
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
            map.push((gnx.clone(), offset));
        }
    }
    Some(map)
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
    fn a_node_in_no_mappable_file_has_no_lines() {
        let (o, root, _) = outline("@nosent");
        assert_eq!(find_file_line(&o, &root, 1), None);
        let o = Outline::new_empty();
        let p = o.root_position().unwrap();
        assert_eq!(file_line(&o, &p, 0), None);
    }
}
