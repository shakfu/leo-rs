//! Positions in a server's document and positions in a node's body.
//!
//! A document is an external file, mapped by `leolib::goto::LineMap`, or a
//! node in no file, whose body is the whole document. A body position is a
//! row and a character column; a document position is a line and a column
//! in the encoding the server chose.

use leolib::goto::LineMap;

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
