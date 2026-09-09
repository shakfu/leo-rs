//! Motions and text objects.
//!
//! Every motion is a pure function of the buffer, the cursor and a count, and
//! returns a [`Target`]. Operators then compose with all of them by
//! construction rather than by a matrix of special cases.

/// A position in the buffer: line, then character within it.
pub type Pos = (usize, usize);

/// Whether a range covers characters or whole lines. vim's distinction, and
/// it decides what `d` deletes and how `p` puts it back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Charwise,
    /// The range covers whole lines, newline included.
    Linewise,
}

/// Where a motion lands, and whether the range it implies includes its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub pos: Pos,
    pub kind: Kind,
    /// vim's "inclusive" motions (`e`, `f`, `$`) cover the character they land
    /// on; exclusive ones (`w`, `0`) stop before it.
    pub inclusive: bool,
}

impl Target {
    fn char_ex(pos: Pos) -> Self {
        Self {
            pos,
            kind: Kind::Charwise,
            inclusive: false,
        }
    }
    fn char_in(pos: Pos) -> Self {
        Self {
            pos,
            kind: Kind::Charwise,
            inclusive: true,
        }
    }
    fn line(pos: Pos) -> Self {
        Self {
            pos,
            kind: Kind::Linewise,
            inclusive: true,
        }
    }
}

/// Every motion this editor knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    Up,
    Down,
    WordForward {
        big: bool,
    },
    WordBackward {
        big: bool,
    },
    WordEnd {
        big: bool,
    },
    WordEndBackward {
        big: bool,
    },
    LineStart,
    FirstNonBlank,
    LineEnd,
    FileEnd,
    ParagraphForward,
    ParagraphBackward,
    /// `f` `F` `t` `T`: the char, and whether to stop before it.
    Find {
        ch: char,
        forward: bool,
        till: bool,
    },
    MatchingBracket,
    ScreenTop,
    ScreenMiddle,
    ScreenBottom,
    /// `G` with a count, and `:N`.
    GotoLine(usize),
}

fn chars(line: &str) -> Vec<char> {
    line.chars().collect()
}

fn len(lines: &[String], row: usize) -> usize {
    lines.get(row).map(|l| l.chars().count()).unwrap_or(0)
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// vim's three character classes: word, punctuation, whitespace.
fn class(c: char, big: bool) -> u8 {
    if c.is_whitespace() {
        0
    } else if big || is_word(c) {
        1
    } else {
        2
    }
}

/// Resolve a motion. Returns None when it cannot move at all.
pub fn apply(
    lines: &[String],
    cursor: Pos,
    motion: Motion,
    count: usize,
    screen: (usize, usize),
) -> Option<Target> {
    let (row, col) = cursor;
    let last_row = lines.len().saturating_sub(1);
    match motion {
        Motion::Left => {
            let n = col.saturating_sub(count);
            (n != col).then(|| Target::char_ex((row, n)))
        }
        Motion::Right => {
            let n = (col + count).min(len(lines, row));
            (n != col).then(|| Target::char_ex((row, n)))
        }
        Motion::Up => {
            let n = row.checked_sub(count)?;
            Some(Target::line((n, col)))
        }
        Motion::Down => {
            let n = row + count;
            (n <= last_row).then(|| Target::line((n, col)))
        }
        Motion::WordForward { big } => {
            let mut pos = cursor;
            for _ in 0..count {
                pos = word_forward(lines, pos, big);
            }
            Some(Target::char_ex(pos))
        }
        Motion::WordBackward { big } => {
            let mut pos = cursor;
            for _ in 0..count {
                pos = word_backward(lines, pos, big);
            }
            Some(Target::char_ex(pos))
        }
        Motion::WordEnd { big } => {
            let mut pos = cursor;
            for _ in 0..count {
                pos = word_end(lines, pos, big);
            }
            Some(Target::char_in(pos))
        }
        Motion::WordEndBackward { big } => {
            let mut pos = cursor;
            for _ in 0..count {
                pos = word_end_backward(lines, pos, big);
            }
            Some(Target::char_in(pos))
        }
        Motion::LineStart => Some(Target::char_ex((row, 0))),
        Motion::FirstNonBlank => {
            let c = chars(lines.get(row)?);
            let i = c.iter().position(|ch| !ch.is_whitespace()).unwrap_or(0);
            Some(Target::char_ex((row, i)))
        }
        Motion::LineEnd => {
            let n = len(lines, row + count.saturating_sub(1)).saturating_sub(1);
            let r = (row + count.saturating_sub(1)).min(last_row);
            Some(Target::char_in((r, n)))
        }
        Motion::FileEnd => Some(Target::line((last_row, 0))),
        Motion::GotoLine(n) => Some(Target::line((n.saturating_sub(1).min(last_row), 0))),
        Motion::ParagraphForward => {
            let mut r = row;
            for _ in 0..count {
                r = paragraph(lines, r, true);
            }
            Some(Target::char_ex((r, 0)))
        }
        Motion::ParagraphBackward => {
            let mut r = row;
            for _ in 0..count {
                r = paragraph(lines, r, false);
            }
            Some(Target::char_ex((r, 0)))
        }
        Motion::Find { ch, forward, till } => {
            // The count applies to the character, not to the landing spot:
            // `2t.` stops before the *second* dot. Counting on the landing
            // spot would leave `t` stuck one place short of the first one.
            let line = chars(lines.get(row)?);
            let mut at = col;
            for _ in 0..count {
                at = find_char(&line, at, ch, forward)?;
            }
            let at = if till {
                if forward {
                    at.checked_sub(1)?
                } else {
                    at + 1
                }
            } else {
                at
            };
            Some(if forward {
                Target::char_in((row, at))
            } else {
                Target::char_ex((row, at))
            })
        }
        Motion::MatchingBracket => matching(lines, cursor).map(Target::char_in),
        Motion::ScreenTop => Some(Target::line((screen.0.min(last_row), 0))),
        Motion::ScreenMiddle => {
            let r = (screen.0 + screen.1 / 2).min(last_row);
            Some(Target::line((r, 0)))
        }
        Motion::ScreenBottom => {
            let r = (screen.0 + screen.1.saturating_sub(1)).min(last_row);
            Some(Target::line((r, 0)))
        }
    }
}

fn word_forward(lines: &[String], (row, col): Pos, big: bool) -> Pos {
    let line = chars(&lines[row]);
    let mut i = col;
    if i < line.len() {
        let start = class(line[i], big);
        while i < line.len() && class(line[i], big) == start {
            i += 1;
        }
    }
    while i < line.len() && line[i].is_whitespace() {
        i += 1;
    }
    if i >= line.len() && row + 1 < lines.len() {
        // The start of the next line, skipping its indentation.
        let next = chars(&lines[row + 1]);
        let j = next.iter().position(|c| !c.is_whitespace()).unwrap_or(0);
        return (row + 1, j);
    }
    (row, i.min(line.len()))
}

fn word_backward(lines: &[String], (row, col): Pos, big: bool) -> Pos {
    if col == 0 {
        if row == 0 {
            return (0, 0);
        }
        let prev = chars(&lines[row - 1]);
        return word_backward(lines, (row - 1, prev.len()), big);
    }
    let line = chars(&lines[row]);
    let mut i = col - 1;
    while i > 0 && line[i].is_whitespace() {
        i -= 1;
    }
    if line[i].is_whitespace() {
        return (row, 0);
    }
    let start = class(line[i], big);
    while i > 0 && class(line[i - 1], big) == start {
        i -= 1;
    }
    (row, i)
}

fn word_end(lines: &[String], (row, col): Pos, big: bool) -> Pos {
    let line = chars(&lines[row]);
    let mut i = col + 1;
    while i < line.len() && line[i].is_whitespace() {
        i += 1;
    }
    if i >= line.len() {
        if row + 1 < lines.len() {
            return word_end(lines, (row + 1, 0), big);
        }
        return (row, line.len().saturating_sub(1));
    }
    let start = class(line[i], big);
    while i + 1 < line.len() && class(line[i + 1], big) == start {
        i += 1;
    }
    (row, i)
}

fn word_end_backward(lines: &[String], (row, col): Pos, big: bool) -> Pos {
    let back = word_backward(lines, (row, col), big);
    if back == (row, col) {
        return back;
    }
    let (r, c) = back;
    if c == 0 && r > 0 {
        let prev = chars(&lines[r - 1]);
        return (r - 1, prev.len().saturating_sub(1));
    }
    (r, c.saturating_sub(1))
}

/// The next or previous blank line.
fn paragraph(lines: &[String], row: usize, forward: bool) -> usize {
    let mut r = row;
    loop {
        if forward {
            if r + 1 >= lines.len() {
                return lines.len().saturating_sub(1);
            }
            r += 1;
        } else {
            if r == 0 {
                return 0;
            }
            r -= 1;
        }
        if lines[r].trim().is_empty() && r != row {
            return r;
        }
    }
}

/// The next occurrence of `ch` strictly past `from`, in either direction.
fn find_char(line: &[char], from: usize, ch: char, forward: bool) -> Option<usize> {
    if forward {
        (from + 1..line.len()).find(|&i| line[i] == ch)
    } else {
        (0..from).rev().find(|&i| line[i] == ch)
    }
}

const PAIRS: &[(char, char)] = &[('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')];

/// `%`: the bracket matching the first one at or after the cursor.
fn matching(lines: &[String], (row, col): Pos) -> Option<Pos> {
    let line = chars(lines.get(row)?);
    let (start_col, open, close, forward) = (col..line.len()).find_map(|i| {
        let c = line[i];
        PAIRS.iter().find_map(|&(o, cl)| {
            if c == o {
                Some((i, o, cl, true))
            } else if c == cl {
                Some((i, o, cl, false))
            } else {
                None
            }
        })
    })?;
    let mut depth = 0i32;
    let mut pos = (row, start_col);
    loop {
        let line = chars(&lines[pos.0]);
        let c = *line.get(pos.1)?;
        if c == open {
            depth += if forward { 1 } else { -1 };
        } else if c == close {
            depth += if forward { -1 } else { 1 };
        }
        if depth == 0 {
            return Some(pos);
        }
        pos = if forward {
            step_forward(lines, pos)?
        } else {
            step_backward(lines, pos)?
        };
    }
}

fn step_forward(lines: &[String], (row, col): Pos) -> Option<Pos> {
    if col + 1 < len(lines, row) {
        Some((row, col + 1))
    } else if row + 1 < lines.len() {
        Some((row + 1, 0))
    } else {
        None
    }
}

fn step_backward(lines: &[String], (row, col): Pos) -> Option<Pos> {
    if col > 0 {
        Some((row, col - 1))
    } else if row > 0 {
        Some((row - 1, len(lines, row - 1).saturating_sub(1)))
    } else {
        None
    }
}

// --- Text objects ---------------------------------------------------------

/// `iw`, `a"`, `i(` and the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextObject {
    Word { big: bool, around: bool },
    Quoted { delim: char, around: bool },
    Bracket { open: char, around: bool },
    Paragraph { around: bool },
}

/// The range a text object covers, as (start, end_exclusive) and its kind.
pub fn object_range(lines: &[String], cursor: Pos, object: TextObject) -> Option<(Pos, Pos, Kind)> {
    let (row, col) = cursor;
    match object {
        TextObject::Word { big, around } => {
            let line = chars(lines.get(row)?);
            if line.is_empty() {
                return Some(((row, 0), (row, 0), Kind::Charwise));
            }
            let i = col.min(line.len() - 1);
            let k = class(line[i], big);
            let mut start = i;
            while start > 0 && class(line[start - 1], big) == k {
                start -= 1;
            }
            let mut end = i + 1;
            while end < line.len() && class(line[end], big) == k {
                end += 1;
            }
            if around {
                // `aw` takes the trailing whitespace, or the leading if there
                // is none after it.
                let mut e = end;
                while e < line.len() && line[e].is_whitespace() {
                    e += 1;
                }
                if e == end {
                    while start > 0 && line[start - 1].is_whitespace() {
                        start -= 1;
                    }
                } else {
                    end = e;
                }
            }
            Some(((row, start), (row, end), Kind::Charwise))
        }
        TextObject::Quoted { delim, around } => {
            let line = chars(lines.get(row)?);
            // The pair enclosing the cursor, else the next one on the line.
            let positions: Vec<usize> = (0..line.len()).filter(|&i| line[i] == delim).collect();
            let pair = positions
                .chunks(2)
                .find(|c| c.len() == 2 && (c[1] >= col || c[0] >= col))?;
            let (a, b) = (pair[0], pair[1]);
            Some(if around {
                ((row, a), (row, b + 1), Kind::Charwise)
            } else {
                ((row, a + 1), (row, b), Kind::Charwise)
            })
        }
        TextObject::Bracket { open, around } => {
            let close = PAIRS.iter().find(|(o, _)| *o == open).map(|(_, c)| *c)?;
            let start = enclosing(lines, cursor, open, close, false)?;
            let end = enclosing(lines, cursor, open, close, true)?;
            Some(if around {
                (
                    start,
                    step_forward(lines, end).unwrap_or(end),
                    Kind::Charwise,
                )
            } else {
                (
                    step_forward(lines, start).unwrap_or(start),
                    end,
                    Kind::Charwise,
                )
            })
        }
        TextObject::Paragraph { around } => {
            let mut start = row;
            while start > 0 && !lines[start - 1].trim().is_empty() {
                start -= 1;
            }
            let mut end = row;
            while end + 1 < lines.len() && !lines[end + 1].trim().is_empty() {
                end += 1;
            }
            if around {
                while end + 1 < lines.len() && lines[end + 1].trim().is_empty() {
                    end += 1;
                }
            }
            Some(((start, 0), (end, 0), Kind::Linewise))
        }
    }
}

/// Scan out from the cursor for the bracket that encloses it.
fn enclosing(lines: &[String], cursor: Pos, open: char, close: char, forward: bool) -> Option<Pos> {
    let mut depth = 0i32;
    let mut pos = cursor;
    loop {
        let line = chars(lines.get(pos.0)?);
        if let Some(&c) = line.get(pos.1) {
            if (forward && c == close) || (!forward && c == open) {
                if depth == 0 {
                    return Some(pos);
                }
                depth -= 1;
            } else if ((forward && c == open) || (!forward && c == close)) && pos != cursor {
                depth += 1;
            }
        }
        pos = if forward {
            step_forward(lines, pos)?
        } else {
            step_backward(lines, pos)?
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(text: &str) -> Vec<String> {
        text.split('\n').map(|s| s.to_string()).collect()
    }

    fn go(text: &str, cursor: Pos, motion: Motion, count: usize) -> Option<Pos> {
        apply(&buf(text), cursor, motion, count, (0, 10)).map(|t| t.pos)
    }

    #[test]
    fn word_motions_walk_by_class() {
        let text = "foo bar.baz qux";
        assert_eq!(
            go(text, (0, 0), Motion::WordForward { big: false }, 1),
            Some((0, 4))
        );
        assert_eq!(
            go(text, (0, 4), Motion::WordForward { big: false }, 1),
            Some((0, 7))
        );
        // A big word ignores the punctuation.
        assert_eq!(
            go(text, (0, 0), Motion::WordForward { big: true }, 1),
            Some((0, 4))
        );
        assert_eq!(
            go(text, (0, 4), Motion::WordForward { big: true }, 1),
            Some((0, 12))
        );
    }

    #[test]
    fn a_count_repeats_a_motion() {
        let text = "one two three four";
        assert_eq!(
            go(text, (0, 0), Motion::WordForward { big: false }, 3),
            Some((0, 14))
        );
    }

    #[test]
    fn word_end_lands_on_the_last_character() {
        let text = "foo bar";
        assert_eq!(
            go(text, (0, 0), Motion::WordEnd { big: false }, 1),
            Some((0, 2))
        );
        assert_eq!(
            go(text, (0, 2), Motion::WordEnd { big: false }, 1),
            Some((0, 6))
        );
    }

    #[test]
    fn word_forward_crosses_lines() {
        let text = "foo\n    bar";
        assert_eq!(
            go(text, (0, 0), Motion::WordForward { big: false }, 1),
            Some((1, 4))
        );
    }

    #[test]
    fn find_stops_on_or_before_the_character() {
        let text = "a.b.c";
        let f = Motion::Find {
            ch: '.',
            forward: true,
            till: false,
        };
        assert_eq!(go(text, (0, 0), f, 1), Some((0, 1)));
        assert_eq!(go(text, (0, 0), f, 2), Some((0, 3)));
        let t = Motion::Find {
            ch: '.',
            forward: true,
            till: true,
        };
        assert_eq!(go(text, (0, 0), t, 1), Some((0, 0)));
        let big_f = Motion::Find {
            ch: '.',
            forward: false,
            till: false,
        };
        assert_eq!(go(text, (0, 4), big_f, 1), Some((0, 3)));
    }

    #[test]
    fn percent_matches_across_lines() {
        let text = "fn f() {\n    g();\n}";
        assert_eq!(go(text, (0, 7), Motion::MatchingBracket, 1), Some((2, 0)));
        assert_eq!(go(text, (2, 0), Motion::MatchingBracket, 1), Some((0, 7)));
    }

    #[test]
    fn line_motions_are_linewise() {
        let t = apply(&buf("a\nb\nc"), (0, 0), Motion::Down, 1, (0, 10)).unwrap();
        assert_eq!(t.kind, Kind::Linewise);
        let t = apply(&buf("abc"), (0, 0), Motion::LineEnd, 1, (0, 10)).unwrap();
        assert_eq!((t.kind, t.inclusive, t.pos), (Kind::Charwise, true, (0, 2)));
    }

    #[test]
    fn inner_word_stops_at_the_word() {
        let lines = buf("foo bar baz");
        let (a, b, kind) = object_range(
            &lines,
            (0, 5),
            TextObject::Word {
                big: false,
                around: false,
            },
        )
        .unwrap();
        assert_eq!((a, b, kind), ((0, 4), (0, 7), Kind::Charwise));
    }

    #[test]
    fn a_word_takes_the_trailing_space() {
        let lines = buf("foo bar baz");
        let (a, b, _) = object_range(
            &lines,
            (0, 4),
            TextObject::Word {
                big: false,
                around: true,
            },
        )
        .unwrap();
        assert_eq!((a, b), ((0, 4), (0, 8)));
    }

    #[test]
    fn quoted_objects_find_the_pair() {
        let lines = buf(r#"say "hello there" now"#);
        let (a, b, _) = object_range(
            &lines,
            (0, 8),
            TextObject::Quoted {
                delim: '"',
                around: false,
            },
        )
        .unwrap();
        assert_eq!((a, b), ((0, 5), (0, 16)));
        let (a, b, _) = object_range(
            &lines,
            (0, 8),
            TextObject::Quoted {
                delim: '"',
                around: true,
            },
        )
        .unwrap();
        assert_eq!((a, b), ((0, 4), (0, 17)));
    }

    #[test]
    fn bracket_objects_nest() {
        let lines = buf("f(g(x), y)");
        let (a, b, _) = object_range(
            &lines,
            (0, 4),
            TextObject::Bracket {
                open: '(',
                around: false,
            },
        )
        .unwrap();
        assert_eq!((a, b), ((0, 4), (0, 5)));
        let (a, b, _) = object_range(
            &lines,
            (0, 8),
            TextObject::Bracket {
                open: '(',
                around: false,
            },
        )
        .unwrap();
        assert_eq!((a, b), ((0, 2), (0, 9)));
    }

    #[test]
    fn a_paragraph_is_linewise() {
        let lines = buf("one\ntwo\n\nthree");
        let (a, b, kind) =
            object_range(&lines, (1, 0), TextObject::Paragraph { around: false }).unwrap();
        assert_eq!((a.0, b.0, kind), (0, 1, Kind::Linewise));
    }
}
