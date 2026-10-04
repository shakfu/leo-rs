//! Leo's `reformat-paragraph`: `commanderEditCommands.py`, `reformatParagraph`
//! and its `rp_` helpers, with `util.wrap_lines`.
//!
//! The paragraph is wrapped to the `@pagewidth` in effect, which is the one
//! use Leo makes of that directive.

use crate::util;

/// Reformat the paragraph at or around line `row` of `body`.
///
/// Returns the new body and the row of the next paragraph, where Leo leaves
/// the cursor (#1748), or `None` if there is no paragraph there.
pub fn reformat_paragraph(
    body: &str,
    row: usize,
    page_width: i32,
    tab_width: i32,
) -> Option<(String, usize)> {
    let lines = util::split_lines(body);
    let (head, para, tail) = find_bound_paragraph(&lines, row)?;
    let (mut indents, mut leading_ws) = leading_ws(&para, tab_width);
    let result = wrap_all_lines(&mut indents, &mut leading_ws, &para, page_width);
    let s = format!("{}{result}{}", head.concat(), tail.concat());
    // Advance to the next line that is not blank.
    let out = util::split_lines(&s);
    let mut next = head.len() + util::split_lines(&result).len();
    if !result.ends_with('\n') {
        next -= 1;
    }
    while next < out.len() && is_space(&out[next]) {
        next += 1;
    }
    Some((s, next.min(out.len().saturating_sub(1))))
}

/// Python's `str.isspace`: not empty, and nothing but whitespace.
fn is_space(s: &str) -> bool {
    !s.is_empty() && s.chars().all(char::is_whitespace)
}

fn ends_paragraph(s: &str) -> bool {
    s.trim().is_empty()
}

fn single_line_paragraph(s: &str) -> bool {
    s.starts_with('@') || matches!(s.trim(), "\"\"\"" | "'''")
}

/// Leo's `startsParagraph`: a numbered or lettered item, a `-` item, an `@`
/// line, or a docstring delimiter.
fn starts_paragraph(s: &str) -> bool {
    let c: Vec<char> = s.chars().collect();
    let Some(&first) = c.first() else {
        return false;
    };
    if s.trim().is_empty() {
        false
    } else if matches!(s.trim(), "\"\"\"" | "'''") {
        true
    } else if first.is_numeric() {
        let i = c.iter().take_while(|ch| ch.is_numeric()).count();
        matches!(c.get(i), Some(')' | '.'))
    } else if first.is_alphabetic() {
        // Leo indexes past the end of `a.` and raises; the line ends there.
        matches!(c.get(1), Some(')' | '.'))
            && c.get(2).is_none_or(|ch| matches!(ch, ' ' | '\t' | '\n'))
    } else {
        first == '@' || first == '-'
    }
}

type Bound = (Vec<String>, Vec<String>, Vec<String>);

/// Leo's `find_bound_paragraph`: the lines before, of and after the paragraph.
fn find_bound_paragraph(lines: &[String], row: usize) -> Option<Bound> {
    let row = row.min(lines.len());
    let mut head: Vec<String> = lines[..row].to_vec();
    let mut para: Vec<String> = lines[row..].to_vec();
    // If the cursor's line does not start a paragraph, scan backward.
    if para.first().is_some_and(|s| !starts_paragraph(s)) {
        let mut n = 0;
        for s in head.iter().rev() {
            if ends_paragraph(s) || single_line_paragraph(s) {
                break;
            }
            n += 1;
            if starts_paragraph(s) {
                break;
            }
        }
        let moved = head.split_off(head.len() - n);
        para.splice(0..0, moved);
    }
    let mut result = Vec::new();
    let mut started = false;
    let mut end = para.len();
    for (i, s) in para.iter().enumerate() {
        if started {
            if ends_paragraph(s) || starts_paragraph(s) {
                end = i;
                break;
            }
            result.push(s.clone());
        } else if !s.trim().is_empty() {
            result.push(s.clone());
            started = true;
            if single_line_paragraph(s) {
                end = i + 1;
                break;
            }
        } else {
            head.push(s.clone());
        }
    }
    started.then(|| (head, result, para[end..].to_vec()))
}

/// Leo's `rp_get_leading_ws`: the indentation of the first two lines.
fn leading_ws(lines: &[String], tab_width: i32) -> ([usize; 2], [String; 2]) {
    let mut indents = [0, 0];
    let mut ws = [String::new(), String::new()];
    for i in 0..2 {
        if let Some(line) = lines.get(i) {
            ws[i] = util::get_leading_ws(line).to_string();
            indents[i] = compute_width(&ws[i], tab_width);
        }
    }
    indents[1] = indents[0].max(indents[1]);
    if lines.len() == 1 {
        ws[1] = ws[0].clone();
    }
    (indents, ws)
}

/// Leo's `computeWidth`: the columns s takes, with tabs to `tab_width`.
fn compute_width(s: &str, tab_width: i32) -> usize {
    let tab = tab_width.unsigned_abs().max(1) as usize;
    let mut w = 0;
    for ch in s.chars() {
        match ch {
            '\t' => w += tab - (w % tab),
            '\n' => break,
            _ => w += 1,
        }
    }
    w
}

/// Leo's `rp_wrap_all_lines`.
fn wrap_all_lines(
    indents: &mut [usize; 2],
    leading_ws: &mut [String; 2],
    lines: &[String],
    page_width: i32,
) -> String {
    let trailing_nl = lines.last().is_some_and(|l| l.ends_with('\n'));
    let lines: Vec<&str> = lines
        .iter()
        .map(|l| l.strip_suffix('\n').unwrap_or(l))
        .collect();
    if let Some(s) = lines.first().filter(|s| starts_paragraph(s)) {
        // A hanging indent: later lines align with the item's text.
        let c: Vec<char> = s.chars().collect();
        let mut i = 0;
        if c[0].is_numeric() {
            i = c.iter().take_while(|ch| ch.is_numeric()).count();
            if matches!(c.get(i), Some(')' | '.')) {
                i += 1;
            }
        } else if c[0].is_alphabetic() {
            if matches!(c.get(1), Some(')' | '.')) {
                i = 2;
            }
        } else if c[0] == '-' {
            i = 1;
        }
        let mut j = i + 1;
        while j < c.len() && matches!(c[j], ' ' | '\t') {
            j += 1;
        }
        if j > indents[1] {
            indents[1] = j;
            leading_ws[1] = " ".repeat(j);
        }
    }
    let page_width = page_width.max(0) as usize;
    let first = page_width.saturating_sub(indents[0]);
    let result = wrap_lines(&lines, page_width.saturating_sub(indents[1]), first);
    let mut out: Vec<String> = Vec::new();
    for (k, line) in result.iter().enumerate() {
        out.push(format!("{}{line}", leading_ws[usize::from(k > 0)]));
    }
    let mut s = out.join("\n");
    if trailing_nl {
        s.push('\n');
    }
    s
}

/// Leo's `wrap_lines`: the words of `lines`, filled to `page_width`, the first
/// line to `first_line_width`. A width of zero means `page_width`, and neither
/// goes below 10.
pub fn wrap_lines(lines: &[&str], page_width: usize, first_line_width: usize) -> Vec<String> {
    let page_width = page_width.max(10);
    let first_line_width = if first_line_width == 0 {
        page_width
    } else {
        first_line_width.max(10)
    };
    let mut width = first_line_width;
    let mut result = Vec::new();
    let mut line = String::new();
    let mut len = 0;
    for s in lines {
        for word in s.split([' ', '\t']).filter(|w| !w.is_empty()) {
            let word_len = word.chars().count();
            let needed = word_len + usize::from(len > 0);
            if needed + len <= width {
                if len > 0 {
                    line.push(' ');
                }
                line.push_str(word);
                len += needed;
            } else {
                if len > 0 {
                    result.push(std::mem::take(&mut line));
                    width = page_width;
                }
                line = word.to_string();
                len = word_len;
                // A word longer than the page has a line of its own.
                if len > page_width {
                    result.push(std::mem::take(&mut line));
                    width = page_width;
                    len = 0;
                }
            }
        }
    }
    if len > 0 {
        result.push(line);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reformat(body: &str, row: usize, width: i32) -> (String, usize) {
        reformat_paragraph(body, row, width, -4).unwrap()
    }

    #[test]
    fn a_paragraph_is_filled_to_the_page_width() {
        let body = "one two three\nfour five six seven eight\n";
        let (s, next) = reformat(body, 0, 14);
        assert_eq!(s, "one two three\nfour five six\nseven eight\n");
        assert_eq!(next, 2);
    }

    #[test]
    fn only_the_cursors_paragraph_changes_and_the_cursor_moves_past_it() {
        let body = "a b\nc d\n\n\nx y\nz\n";
        let (s, next) = reformat(body, 1, 40);
        assert_eq!(s, "a b c d\n\n\nx y\nz\n");
        assert_eq!(next, 3);
    }

    #[test]
    fn wrapping_matches_leos() {
        // Leo's `rp_get_leading_ws` and `rp_wrap_all_lines` on the same input.
        let cases: [(&[&str], i32, i32, &str); 7] = [
            (
                &["1. alpha beta gamma delta\n"],
                14,
                -4,
                "1. alpha beta\n   gamma delta\n",
            ),
            (
                &["  one two three four\n", "    five\n"],
                14,
                -4,
                "  one two\n    three four\n    five\n",
            ),
            (
                &["one two three\n", "four five six seven eight\n"],
                14,
                -4,
                "one two three\nfour five six\nseven eight\n",
            ),
            (
                &["- item with\twords that wrap around\n"],
                20,
                -4,
                "- item with words\n  that wrap around\n",
            ),
            (
                &["a) item with words that wrap around"],
                20,
                -4,
                "a) item with words\n   that wrap around",
            ),
            (
                &["\tx yy zzz wwww vvvvv uuuuuu ttttttt\n"],
                20,
                8,
                "\tx yy zzz\n\twwww vvvvv\n\tuuuuuu\n\tttttttt\n",
            ),
            (
                &["End. Next sentence here! And more? yes\n"],
                15,
                -4,
                "End. Next\nsentence here!\nAnd more? yes\n",
            ),
        ];
        for (lines, width, tab, want) in cases {
            let lines: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
            let (mut indents, mut ws) = leading_ws(&lines, tab);
            assert_eq!(wrap_all_lines(&mut indents, &mut ws, &lines, width), want);
        }
    }

    #[test]
    fn the_first_two_lines_set_the_indentation() {
        let body = "  one two three four\n    five\n";
        let (s, _) = reformat(body, 1, 14);
        assert_eq!(s, "  one two\n    three four\n    five\n");
    }

    #[test]
    fn an_at_line_is_a_paragraph_of_its_own() {
        let body = "@language python\none\ntwo\n";
        let (s, next) = reformat(body, 0, 40);
        assert_eq!(s, body);
        assert_eq!(next, 1);
        let (s, _) = reformat(body, 2, 40);
        assert_eq!(s, "@language python\none two\n");
    }

    #[test]
    fn a_blank_body_has_no_paragraph() {
        assert!(reformat_paragraph("\n\n", 0, 80, 4).is_none());
        assert!(reformat_paragraph("", 0, 80, 4).is_none());
    }

    #[test]
    fn a_word_longer_than_the_page_has_its_own_line() {
        assert_eq!(
            wrap_lines(&["a bbbbbbbbbbbbbbb c"], 10, 10),
            vec!["a", "bbbbbbbbbbbbbbb", "c"]
        );
    }
}
