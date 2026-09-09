//! Colouring the body, following the `@language` directives inside it.
//!
//! The language a node is written in comes from the model:
//! `Outline::get_language` implements Leo's four-pass rule over the node, its
//! ancestors and the nearest `@<file>` extension. That is the language the
//! body *starts* in. A body may then change it: Leo's `match_at_language`
//! matches `@language` only at column 0 and switches from that line onward, so
//! one node can hold Python and then C, and this follows it.
//!
//! The scanner lives here rather than in `leolib` for the reason Leo keeps
//! `leoColorizer` out of its model: colouring is a view's business. It shares
//! the language *data* -- comment delimiters and string delimiters -- with the
//! model rather than restating it.

use leolib::outline::set_delims_from_language;
use leolib::Outline;

use crate::keywords;

/// What a run of characters is, and therefore how it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Plain,
    /// A Leo directive: `@language`, `@others`, `@tabwidth`.
    Directive,
    /// A section reference, `<< like this >>`.
    Section,
    Comment,
    Str,
    Number,
    Keyword,
    /// A library name: jEdit's `keyword2` to `keyword4`.
    Builtin,
}

/// A run of one class, as byte offsets into its line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub class: Class,
}

/// Leo's directives, which are coloured wherever the language is.
const DIRECTIVES: &[&str] = &[
    "@language",
    "@others",
    "@tabwidth",
    "@pagewidth",
    "@encoding",
    "@lineending",
    "@comment",
    "@delims",
    "@section-delims",
    "@first",
    "@last",
    "@path",
    "@nocolor-node",
    "@nocolor",
    "@killcolor",
    "@color",
    "@nowrap",
    "@wrap",
    "@nosearch",
    "@ignore",
    "@all",
    "@raw",
    "@end_raw",
    "@c",
    "@code",
    "@doc",
];

/// One language's lexical shape, from the model's own tables.
struct Rules {
    line_comment: String,
    block_start: String,
    block_end: String,
    /// String delimiters, longest first.
    strings: Vec<String>,
    keywords: &'static [&'static str],
    builtins: &'static [&'static str],
}

impl Rules {
    fn for_language(language: &str) -> Self {
        let (line_comment, block_start, block_end) = set_delims_from_language(language);
        // The importers already know which delimiters open a string in each
        // language -- Python's triple quotes, C's lack of single ones -- so
        // take theirs rather than keeping a second list.
        let strings = leolib::importers::LANGUAGES
            .iter()
            .find(|spec| spec.language == language)
            .map(|spec| spec.string_list.iter().map(|s| s.to_string()).collect())
            .unwrap_or_else(|| vec!["\"".to_string(), "'".to_string()]);
        let (keywords, builtins) = keywords::for_language(language);
        Self {
            line_comment,
            block_start,
            block_end,
            strings,
            keywords,
            builtins,
        }
    }
}

/// What carries over from one line to the next.
#[derive(Default)]
struct State {
    /// The delimiter that closes an open string or block comment.
    target: String,
    /// True while the open target is a comment rather than a string.
    target_is_comment: bool,
    /// `@nocolor` until `@color`, or `@killcolor` for good.
    colouring: bool,
    killed: bool,
}

/// Colour a body, line by line.
///
/// `language` is where the body starts, from `Outline::get_language`. The
/// result has one entry per line, holding only the runs that are not plain.
pub fn highlight(lines: &[String], language: &str) -> Vec<Vec<Span>> {
    let mut rules = Rules::for_language(language);
    let mut state = State {
        colouring: true,
        ..Default::default()
    };
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        out.push(highlight_line(line, &mut rules, &mut state));
    }
    out
}

/// The language declared at `p`, which the body may then change.
///
/// None when nothing above `p` declares one. Leo would answer its
/// `target_language` and colour the node as Python, which paints a prose node
/// wrong: `class` and `import` become keywords and an apostrophe opens a
/// string that swallows the rest of the body. A node nobody declared a
/// language for is left alone.
pub fn language_of(outline: &Outline, p: &leolib::Position) -> Option<String> {
    outline.language_at(p)
}

fn highlight_line(line: &str, rules: &mut Rules, state: &mut State) -> Vec<Span> {
    if state.killed {
        return Vec::new();
    }
    // Leo matches directives only at the start of a line.
    if let Some(spans) = directive_line(line, rules, state) {
        return spans;
    }
    if !state.colouring {
        return Vec::new();
    }
    if let Some(span) = section_reference(line) {
        return vec![span];
    }
    scan(line, rules, state)
}

/// A line whose first characters are a Leo directive.
///
/// `@language` switches the rules from here on, which is the whole point of
/// this module; `@nocolor` and `@color` bracket a region that is left plain.
fn directive_line(line: &str, rules: &mut Rules, state: &mut State) -> Option<Vec<Span>> {
    let name = DIRECTIVES.iter().find(|d| starts_word(line, d)).copied()?;
    match name {
        "@language" => {
            let rest = line[name.len()..].trim();
            let word: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !word.is_empty() {
                *rules = Rules::for_language(&word);
                // A language change cannot leave a string open behind it.
                state.target.clear();
            }
        }
        "@nocolor" | "@nocolor-node" => state.colouring = false,
        "@color" => state.colouring = true,
        "@killcolor" => state.killed = true,
        _ => {}
    }
    Some(vec![Span {
        start: 0,
        end: line.trim_end().len(),
        class: Class::Directive,
    }])
}

/// True if `word` starts `line` and is not glued to more word characters.
fn starts_word(line: &str, word: &str) -> bool {
    let Some(rest) = line.strip_prefix(word) else {
        return false;
    };
    match rest.chars().next() {
        None => true,
        Some(c) => !(c.is_alphanumeric() || c == '_' || c == '-'),
    }
}

/// A line holding nothing but a section reference.
fn section_reference(line: &str) -> Option<Span> {
    let trimmed = line.trim();
    if !(trimmed.starts_with("<<") && trimmed.ends_with(">>") && trimmed.len() > 4) {
        return None;
    }
    let start = line.len() - line.trim_start().len();
    Some(Span {
        start,
        end: start + trimmed.len(),
        class: Class::Section,
    })
}

/// Comments, strings, numbers and keywords in one line of code.
fn scan(line: &str, rules: &Rules, state: &mut State) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0usize;
    let mut run_start: Option<usize> = None;

    // A string or comment left open by the previous line.
    if !state.target.is_empty() {
        let class = if state.target_is_comment {
            Class::Comment
        } else {
            Class::Str
        };
        match find_close(line, 0, &state.target) {
            Some(end) => {
                spans.push(Span {
                    start: 0,
                    end,
                    class,
                });
                i = end;
                state.target.clear();
            }
            None => {
                return vec![Span {
                    start: 0,
                    end: line.trim_end_matches('\n').len(),
                    class,
                }];
            }
        }
    }

    while i < bytes.len() {
        let rest = &line[i..];
        if rest.starts_with('\n') {
            break;
        }
        // A line comment runs to the end of the line.
        if !rules.line_comment.is_empty() && rest.starts_with(&rules.line_comment) {
            flush_word(line, &mut run_start, i, rules, &mut spans);
            spans.push(Span {
                start: i,
                end: line.trim_end_matches('\n').len(),
                class: Class::Comment,
            });
            return spans;
        }
        // A block comment may run past the end of the line.
        if !rules.block_start.is_empty() && rest.starts_with(&rules.block_start) {
            flush_word(line, &mut run_start, i, rules, &mut spans);
            let from = i + rules.block_start.len();
            match find_close(line, from, &rules.block_end) {
                Some(end) => {
                    spans.push(Span {
                        start: i,
                        end,
                        class: Class::Comment,
                    });
                    i = end;
                }
                None => {
                    spans.push(Span {
                        start: i,
                        end: line.trim_end_matches('\n').len(),
                        class: Class::Comment,
                    });
                    state.target = rules.block_end.clone();
                    state.target_is_comment = true;
                    return spans;
                }
            }
            continue;
        }
        // A string, taking the longest delimiter that matches.
        if let Some(delim) = rules.strings.iter().find(|d| rest.starts_with(d.as_str())) {
            flush_word(line, &mut run_start, i, rules, &mut spans);
            let from = i + delim.len();
            match find_close(line, from, delim) {
                Some(end) => {
                    spans.push(Span {
                        start: i,
                        end,
                        class: Class::Str,
                    });
                    i = end;
                }
                None => {
                    spans.push(Span {
                        start: i,
                        end: line.trim_end_matches('\n').len(),
                        class: Class::Str,
                    });
                    state.target = delim.clone();
                    state.target_is_comment = false;
                    return spans;
                }
            }
            continue;
        }
        let ch = rest.chars().next().unwrap();
        if ch.is_alphanumeric() || ch == '_' {
            if run_start.is_none() {
                run_start = Some(i);
            }
            i += ch.len_utf8();
            continue;
        }
        flush_word(line, &mut run_start, i, rules, &mut spans);
        i += ch.len_utf8();
    }
    flush_word(
        line,
        &mut run_start,
        line.trim_end_matches('\n').len(),
        rules,
        &mut spans,
    );
    spans
}

/// Classify the word that just ended, if it is one worth colouring.
fn flush_word(
    line: &str,
    run_start: &mut Option<usize>,
    end: usize,
    rules: &Rules,
    spans: &mut Vec<Span>,
) {
    let Some(start) = run_start.take() else {
        return;
    };
    if end <= start {
        return;
    }
    let word = &line[start..end];
    let class = if word.starts_with(|c: char| c.is_ascii_digit()) {
        Class::Number
    } else if rules.keywords.contains(&word) {
        Class::Keyword
    } else if rules.builtins.contains(&word) {
        Class::Builtin
    } else {
        return;
    };
    spans.push(Span { start, end, class });
}

/// The offset just past the closing delimiter, honouring backslash escapes.
fn find_close(line: &str, from: usize, close: &str) -> Option<usize> {
    if close.is_empty() {
        return None;
    }
    let mut i = from;
    while i < line.len() {
        let rest = &line[i..];
        if let Some(after) = rest.strip_prefix('\\') {
            // The escape takes the next character with it.
            i += 1 + after.chars().next().map(|c| c.len_utf8()).unwrap_or(0);
            continue;
        }
        if rest.starts_with(close) {
            return Some(i + close.len());
        }
        i += rest.chars().next().unwrap().len_utf8();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.split('\n').map(|s| s.to_string()).collect()
    }

    /// The classified runs of one line, as (text, class).
    fn classes<'a>(line: &'a str, spans: &[Span]) -> Vec<(&'a str, Class)> {
        spans
            .iter()
            .map(|s| (&line[s.start..s.end], s.class))
            .collect()
    }

    #[test]
    fn python_comments_strings_and_keywords() {
        let src = lines("def f(x):  # note\n    return \"hi\"");
        let out = highlight(&src, "python");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![("def", Class::Keyword), ("# note", Class::Comment)]
        );
        assert_eq!(
            classes(&src[1], &out[1]),
            vec![("return", Class::Keyword), ("\"hi\"", Class::Str)]
        );
    }

    #[test]
    fn a_hash_inside_a_string_is_not_a_comment() {
        let src = lines("s = \"# not a comment\"");
        let out = highlight(&src, "python");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![("\"# not a comment\"", Class::Str)]
        );
    }

    #[test]
    fn a_quote_inside_a_comment_does_not_open_a_string() {
        let src = lines("# it's fine\nx = 1");
        let out = highlight(&src, "python");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![("# it's fine", Class::Comment)]
        );
        // The next line is scanned normally, not as the rest of a string.
        assert_eq!(classes(&src[1], &out[1]), vec![("1", Class::Number)]);
    }

    #[test]
    fn a_triple_quoted_string_runs_across_lines() {
        let src = lines("x = \"\"\"one\ntwo\"\"\"\ny = 2");
        let out = highlight(&src, "python");
        assert_eq!(out[0][0].class, Class::Str);
        assert_eq!(classes(&src[1], &out[1]), vec![("two\"\"\"", Class::Str)]);
        assert_eq!(classes(&src[2], &out[2]), vec![("2", Class::Number)]);
    }

    #[test]
    fn a_block_comment_runs_across_lines() {
        let src = lines("int x; /* one\ntwo */ int y;");
        let out = highlight(&src, "c");
        // Leo's C mode tags `int` keyword3, a type, which is a Builtin here.
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![("int", Class::Builtin), ("/* one", Class::Comment)]
        );
        assert_eq!(
            classes(&src[1], &out[1]),
            vec![("two */", Class::Comment), ("int", Class::Builtin)]
        );
    }

    #[test]
    fn an_at_language_directive_switches_the_rules_from_that_line() {
        // The point of the module: one node, two languages.
        let src = lines("# a python comment\n@language c\n// a c comment\nint x;");
        let out = highlight(&src, "python");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![("# a python comment", Class::Comment)]
        );
        assert_eq!(
            classes(&src[1], &out[1]),
            vec![("@language c", Class::Directive)]
        );
        assert_eq!(
            classes(&src[2], &out[2]),
            vec![("// a c comment", Class::Comment)]
        );
        assert_eq!(classes(&src[3], &out[3])[0], ("int", Class::Builtin));
    }

    #[test]
    fn a_language_directive_must_start_the_line() {
        // Leo's match_at_language returns 0 unless i == 0.
        let src = lines("x = 1  # @language c\n// still python");
        let out = highlight(&src, "python");
        assert_eq!(out[0].last().unwrap().class, Class::Comment);
        // The rules did not change, so C's `//` is not a comment here.
        assert!(out[1].iter().all(|s| s.class != Class::Comment));
    }

    #[test]
    fn nocolor_and_color_bracket_a_plain_region() {
        let src = lines("def a():\n@nocolor\ndef b():\n@color\ndef c():");
        let out = highlight(&src, "python");
        assert_eq!(out[0][0].class, Class::Keyword);
        assert_eq!(out[1][0].class, Class::Directive);
        assert!(out[2].is_empty(), "@nocolor did not stop the colouring");
        assert_eq!(out[3][0].class, Class::Directive);
        assert_eq!(out[4][0].class, Class::Keyword);
    }

    #[test]
    fn killcolor_stops_for_good() {
        let src = lines("@killcolor\ndef a():\n@color\ndef b():");
        let out = highlight(&src, "python");
        assert_eq!(out[0][0].class, Class::Directive);
        assert!(out[1..].iter().all(|line| line.is_empty()));
    }

    #[test]
    fn leo_constructs_are_coloured_whatever_the_language() {
        let src = lines("@others\n    << a section >>\n@tabwidth -4");
        let out = highlight(&src, "python");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![("@others", Class::Directive)]
        );
        assert_eq!(
            classes(&src[1], &out[1]),
            vec![("<< a section >>", Class::Section)]
        );
        assert_eq!(
            classes(&src[2], &out[2]),
            vec![("@tabwidth -4", Class::Directive)]
        );
    }

    #[test]
    fn an_unknown_language_still_colours_what_it_can() {
        // No keyword table, but the delimiters come from the model's tables.
        let src = lines("-- a comment\nx = 1");
        let out = highlight(&src, "haskell");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![("-- a comment", Class::Comment)]
        );
        assert_eq!(classes(&src[1], &out[1]), vec![("1", Class::Number)]);
    }

    #[test]
    fn a_language_with_no_data_at_all_is_left_plain() {
        let src = lines("some prose, and 1 number");
        let out = highlight(&src, "not_a_language");
        assert_eq!(classes(&src[0], &out[0]), vec![("1", Class::Number)]);
    }

    #[test]
    fn an_escaped_quote_does_not_end_the_string() {
        let src = lines("s = \"a \\\" b\" + 1");
        let out = highlight(&src, "python");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![("\"a \\\" b\"", Class::Str), ("1", Class::Number)]
        );
    }

    #[test]
    fn spans_never_overlap_and_stay_inside_the_line() {
        let src = lines("def f():  # x\n    return \"a\" + 1  # y\n@language c\nint x = 0; // z");
        for (line, spans) in src.iter().zip(highlight(&src, "python")) {
            let mut last = 0;
            for s in &spans {
                assert!(s.start >= last, "overlap in {line:?}: {spans:?}");
                assert!(s.end <= line.len(), "past the end of {line:?}: {s:?}");
                assert!(s.start < s.end, "empty span in {line:?}: {s:?}");
                last = s.end;
            }
        }
    }
}
