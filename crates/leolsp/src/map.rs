//! Positions in a server's document and positions in a node's body.
//!
//! A document is an external file, mapped by `leolib::goto::LineMap`, or a
//! node in no file, whose body is the whole document. A body position is a
//! row and a character column; a document position is a line and a column
//! in the encoding the server chose.

use leolib::goto::LineMap;
use leolib::seqmatch::{SequenceMatcher, Tag};

/// How a server counts columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf16,
}

impl Encoding {
    /// Columns `s` takes.
    pub fn len(self, s: &str) -> usize {
        match self {
            Encoding::Utf8 => s.len(),
            Encoding::Utf16 => s.encode_utf16().count(),
        }
    }

    /// The byte offset of column `col` of `line`, clamped to the line.
    pub fn byte(self, line: &str, col: usize) -> usize {
        let mut seen = 0;
        for (i, ch) in line.char_indices() {
            if seen >= col {
                return i;
            }
            seen += match self {
                Encoding::Utf8 => ch.len_utf8(),
                Encoding::Utf16 => ch.len_utf16(),
            };
        }
        line.len()
    }
}

/// What a document's lines are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mapping {
    File(LineMap),
    /// The body of the node with this gnx, line for row.
    Node(String),
}

/// A body position: the node, its row, and a character column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BodyPos {
    pub gnx: String,
    pub row: usize,
    pub col: usize,
}

/// A document's text and how its lines map.
#[derive(Clone, Debug)]
pub struct Doc {
    pub text: String,
    pub mapping: Mapping,
}

impl Doc {
    fn line(&self, n: usize) -> Option<&str> {
        self.text
            .split_inclusive('\n')
            .nth(n)
            .map(|l| l.trim_end_matches(['\n', '\r']))
    }

    /// The document line, and the bytes before the body text on it, of
    /// body row `row` of `gnx`.
    fn line_of(&self, gnx: &str, row: usize) -> Option<(usize, usize)> {
        match &self.mapping {
            Mapping::File(map) => {
                let n = map.line_of(gnx, row)?;
                Some((n, map.lines[n].indent?))
            }
            Mapping::Node(g) => (g == gnx && self.line(row).is_some()).then_some((row, 0)),
        }
    }

    /// The document position of a body position. None if the row writes no
    /// line of its own.
    pub fn to_doc(&self, at: &BodyPos, enc: Encoding) -> Option<(u32, u32)> {
        let (n, indent) = self.line_of(&at.gnx, at.row)?;
        let line = self.line(n)?;
        let body = line.get(indent..)?;
        let byte = indent
            + body
                .char_indices()
                .nth(at.col)
                .map_or(body.len(), |(i, _)| i);
        Some((n as u32, enc.len(&line[..byte]) as u32))
    }

    /// The body position of a document position. None on a line no body row
    /// writes as it is, such as a sentinel; a column inside the `@others`
    /// indent is column 0.
    pub fn to_body(&self, line: u32, col: u32, enc: Encoding) -> Option<BodyPos> {
        let n = line as usize;
        let (gnx, row, indent) = match &self.mapping {
            Mapping::File(map) => {
                let l = map.lines.get(n)?;
                (l.gnx.clone(), l.row, l.indent?)
            }
            Mapping::Node(gnx) => (gnx.clone(), n, 0),
        };
        let text = self.line(n).unwrap_or("");
        let byte = enc.byte(text, col as usize).max(indent.min(text.len()));
        let col = text
            .get(indent.min(byte)..byte)
            .map_or(0, |s| s.chars().count());
        Some(BodyPos { gnx, row, col })
    }
}

impl Doc {
    /// A document range as one node's body range. None unless every line it
    /// spans is that node's own text: a range across a sentinel or into a
    /// child under `@others` is no range of one body.
    pub fn range_to_body(
        &self,
        start: (u32, u32),
        end: (u32, u32),
        enc: Encoding,
    ) -> Option<(BodyPos, BodyPos)> {
        let a = self.to_body(start.0, start.1, enc)?;
        let b = self.to_body(end.0, end.1, enc)?;
        if a.gnx != b.gnx {
            return None;
        }
        if let Mapping::File(map) = &self.mapping {
            let lines = map.lines.get(start.0 as usize..=end.0 as usize)?;
            let mut rows = lines
                .iter()
                .map(|l| (l.indent.is_some() && l.gnx == a.gnx).then_some(l.row));
            let first = rows.next()??;
            let mut want = first;
            for row in rows {
                want += 1;
                if row? != want {
                    return None;
                }
            }
        }
        Some((a, b))
    }
}

impl Doc {
    /// Document line `n`'s node, body row and indent, if a body row writes it.
    fn row(&self, n: usize) -> Option<(&str, usize, usize)> {
        match &self.mapping {
            Mapping::File(map) => {
                let l = map.lines.get(n)?;
                Some((l.gnx.as_str(), l.row, l.indent?))
            }
            Mapping::Node(gnx) => Some((gnx.as_str(), n, 0)),
        }
    }

    /// A document edit as body edits: start, end and replacement.
    ///
    /// An edit inside one body maps as it is. A wider one, such as a
    /// formatter's or a fix-all's replacement of the whole file, is cut by a
    /// line diff into the runs of lines it changes, and each run must lie in
    /// one body. None if a run does not, or if a changed line under
    /// `@others` loses its indent.
    pub fn edit_to_body(
        &self,
        start: (u32, u32),
        end: (u32, u32),
        text: &str,
        enc: Encoding,
    ) -> Option<Vec<(BodyPos, BodyPos, String)>> {
        if let Some((a, b)) = self.range_to_body(start, end, enc) {
            return Some(vec![(a, b, text.to_string())]);
        }
        // Widen the edit to whole lines, so old and new are lists of lines.
        let lines: Vec<&str> = self.text.split_inclusive('\n').collect();
        let (l0, l1) = (start.0 as usize, end.0 as usize);
        if l0 > l1 || l0 > lines.len() {
            return None;
        }
        let bare = |n: usize| {
            lines
                .get(n)
                .map_or("", |l| l.trim_end_matches(['\n', '\r']))
        };
        let head = &bare(l0)[..enc.byte(bare(l0), start.1 as usize)];
        let tail = lines
            .get(l1)
            .map_or("", |l| &l[enc.byte(bare(l1), end.1 as usize)..]);
        let old = &lines[l0..(l1 + 1).min(lines.len())];
        let new_text = format!("{head}{text}{tail}");
        let new: Vec<&str> = new_text.split_inclusive('\n').collect();

        let mut out = Vec::new();
        for op in SequenceMatcher::new(old, &new).opcodes() {
            if op.tag == Tag::Equal {
                continue;
            }
            let (first, last) = (l0 + op.ai, l0 + op.aj);
            // Where the run lands: its own lines; for an insertion, before
            // the line it precedes, else after the line it follows. `at` is
            // the document line whose indent the new lines carry.
            let (gnx, row, end_row, indent, at) = if first < last {
                let (gnx, row, indent) = self.row(first)?;
                for (k, n) in (first..last).enumerate() {
                    let (g, r, _) = self.row(n)?;
                    if g != gnx || r != row + k {
                        return None;
                    }
                }
                (gnx, row, row + (last - first), indent, first)
            } else if let Some((gnx, row, indent)) = self.row(first).filter(|_| first < lines.len())
            {
                (gnx, row, row, indent, first)
            } else {
                let before = first.checked_sub(1)?;
                let (gnx, row, indent) = self.row(before)?;
                (gnx, row + 1, row + 1, indent, before)
            };
            let prefix = lines.get(at).and_then(|l| l.get(..indent)).unwrap_or("");
            let mut body = String::new();
            for l in &new[op.bi..op.bj] {
                // A blank line carries no indent.
                if l.trim().is_empty() {
                    if l.ends_with('\n') {
                        body.push('\n');
                    }
                } else {
                    body.push_str(l.strip_prefix(prefix)?);
                }
            }
            let pos = |row| BodyPos {
                gnx: gnx.to_string(),
                row,
                col: 0,
            };
            out.push((pos(row), pos(end_row), body));
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leolib::goto::FileLine;

    fn doc() -> Doc {
        // Line 0 is the root's row 0; line 1 a sentinel; line 2 row 1 of `f`
        // behind a four-space `@others` indent.
        let text = "a = '\u{e9}'\n#@+others\n    x = '\u{1f600}' + y\n".to_string();
        let line = |gnx: &str, row, indent| FileLine {
            gnx: gnx.to_string(),
            row,
            indent,
        };
        Doc {
            text,
            mapping: Mapping::File(LineMap {
                text: String::new(),
                lines: vec![
                    line("r", 0, Some(0)),
                    line("r", 1, None),
                    line("f", 1, Some(4)),
                ],
            }),
        }
    }

    fn at(gnx: &str, row: usize, col: usize) -> BodyPos {
        BodyPos {
            gnx: gnx.to_string(),
            row,
            col,
        }
    }

    #[test]
    fn a_body_column_is_counted_past_the_indent_in_the_servers_units() {
        let d = doc();
        // `y` is body column 10: `x = '`, the emoji, `' + `. The emoji is two
        // UTF-16 units and four UTF-8 bytes.
        let y = at("f", 1, 10);
        assert_eq!(d.to_doc(&y, Encoding::Utf16), Some((2, 15)));
        assert_eq!(d.to_doc(&y, Encoding::Utf8), Some((2, 17)));
        assert_eq!(d.to_body(2, 15, Encoding::Utf16), Some(y.clone()));
        assert_eq!(d.to_body(2, 17, Encoding::Utf8), Some(y));
    }

    #[test]
    fn the_indent_and_a_sentinel_map_as_they_must() {
        let d = doc();
        assert_eq!(d.to_body(2, 2, Encoding::Utf16), Some(at("f", 1, 0)));
        assert_eq!(d.to_body(1, 0, Encoding::Utf16), None);
        assert_eq!(d.to_doc(&at("r", 1, 0), Encoding::Utf16), None);
        // A column past the end is the end.
        assert_eq!(d.to_doc(&at("r", 0, 99), Encoding::Utf16), Some((0, 7)));
    }

    #[test]
    fn a_range_is_one_bodys_or_none() {
        let d = doc();
        let (a, b) = d.range_to_body((2, 4), (2, 5), Encoding::Utf16).unwrap();
        assert_eq!((a, b), (at("f", 1, 0), at("f", 1, 1)));
        // Across the sentinel, or from one node into another.
        assert_eq!(d.range_to_body((0, 0), (2, 5), Encoding::Utf16), None);
        assert_eq!(d.range_to_body((1, 0), (1, 1), Encoding::Utf16), None);
    }

    /// An `@auto` file: the root's imports, child `f` under a four-space
    /// `@others`, then the root's last line. No sentinels.
    fn auto() -> Doc {
        let line = |gnx: &str, row, indent| FileLine {
            gnx: gnx.to_string(),
            row,
            indent: Some(indent),
        };
        Doc {
            text: "import os\nimport sys\n    def f():\n        return os\nx = 1\n".into(),
            mapping: Mapping::File(LineMap {
                text: String::new(),
                lines: vec![
                    line("r", 0, 0),
                    line("r", 1, 0),
                    line("f", 0, 4),
                    line("f", 1, 4),
                    line("r", 3, 0),
                ],
            }),
        }
    }

    #[test]
    fn a_whole_file_replacement_is_cut_into_each_bodys_changes() {
        let d = auto();
        let fixed = "import sys\n    def f():\n        return sys\nx = 1\n";
        let edits = d
            .edit_to_body((0, 0), (5, 0), fixed, Encoding::Utf16)
            .unwrap();
        assert_eq!(
            edits,
            [
                (at("r", 0, 0), at("r", 1, 0), String::new()),
                (at("f", 1, 0), at("f", 2, 0), "    return sys\n".into()),
            ]
        );
        let added = "import re\nimport os\nimport sys\n    def f():\n        return os\nx = 1\n";
        let edits = d
            .edit_to_body((0, 0), (5, 0), added, Encoding::Utf16)
            .unwrap();
        assert_eq!(
            edits,
            [(at("r", 0, 0), at("r", 0, 0), "import re\n".into())]
        );
        // Unchanged text is no edit.
        assert_eq!(
            d.edit_to_body((0, 0), (5, 0), &d.text.clone(), Encoding::Utf16),
            Some(vec![])
        );
    }

    #[test]
    fn a_change_that_drops_the_indent_or_meets_a_sentinel_is_refused() {
        let d = auto();
        let outdented = "import os\nimport sys\n    def f():\nreturn os\nx = 1\n";
        assert_eq!(
            d.edit_to_body((0, 0), (5, 0), outdented, Encoding::Utf16),
            None
        );
        let d = doc();
        let text = "a = 1\n#@+others edited\n    x = 1 + y\n";
        assert_eq!(d.edit_to_body((0, 0), (3, 0), text, Encoding::Utf16), None);
    }

    #[test]
    fn a_node_document_is_its_body() {
        let d = Doc {
            text: "one\ntwo\n".into(),
            mapping: Mapping::Node("n".into()),
        };
        assert_eq!(d.to_doc(&at("n", 1, 2), Encoding::Utf8), Some((1, 2)));
        assert_eq!(d.to_doc(&at("m", 1, 2), Encoding::Utf8), None);
        assert_eq!(d.to_body(1, 2, Encoding::Utf8), Some(at("n", 1, 2)));
    }
}
