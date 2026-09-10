//! Colouring the body, following the `@language` directives inside it.
//!
//! The language a node is written in comes from the model:
//! `Outline::get_language` implements Leo's four-pass rule over the node, its
//! ancestors and the nearest `@<file>` extension. That is the language the
//! body *starts* in. A body may then change it: Leo's `match_at_language`
//! matches `@language` only at column 0 and switches from that line onward, so
//! one node can hold Python and then C, and this follows it.
//!
//! Two engines. A dozen languages have a tree-sitter grammar compiled in and
//! go through `treesit`, which can tell a function from a field. The rest --
//! Leo knows comment delimiters for some 170 -- go through the line scanner
//! below, which classifies a word by looking it up in `keywords`.
//!
//! Both live here rather than in `leolib` for the reason Leo keeps
//! `leoColorizer` out of its model: colouring is a view's business.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use leolib::outline::set_delims_from_language;
use leolib::Outline;

use crate::keywords;
use crate::treesit;

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
    /// A literal value: a number, a boolean, a named constant.
    Number,
    Keyword,
    /// A library name: jEdit's `keyword2` to `keyword4`.
    Builtin,
    /// A function or method, defined or called.
    Function,
    /// A type, or a constructor of one.
    Type,
    /// A field or attribute of an object.
    Property,
    /// A decorator or annotation: `@property`, `#[derive(Debug)]`.
    Attribute,
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

/// How a language writes strings and block comments.
///
/// The importers keep a `string_list` too, and this used to read it. They
/// answer a different question -- where does a block of code start -- and
/// Rust's is deliberately empty because its importer scans for itself. Read as
/// a colouring rule that says Rust has no strings, so `//` inside a literal
/// opened a comment that ran to the end of the line.
#[derive(Clone, Copy)]
struct Lex {
    /// String delimiters, longest first.
    strings: &'static [&'static str],
    /// A backslash escapes the next character inside a string.
    escapes: bool,
    /// Block comments nest, so the first close does not end the outermost.
    nested_comments: bool,
}

const DEFAULT_LEX: Lex = Lex {
    strings: &["\"", "'"],
    escapes: true,
    nested_comments: false,
};

/// `language`'s lexical shape, where it differs from `DEFAULT_LEX`.
fn lex_for(language: &str) -> Lex {
    /// `'x'` is a character literal, so a lone `'` must not open a string.
    const CHARS: &[&str] = &["\""];
    const TRIPLE: &[&str] = &["\"\"\"", "'''", "\"", "'"];
    match language {
        "python" | "cython" | "coffeescript" => Lex {
            strings: TRIPLE,
            ..DEFAULT_LEX
        },
        "c" | "cplusplus" | "csharp" | "java" | "objective_c" | "go" | "groovy" | "kotlin"
        | "swift" | "clojure" | "elisp" | "lisp" | "erlang" => Lex {
            strings: CHARS,
            ..DEFAULT_LEX
        },
        "rust" | "scala" | "d" | "dart" | "haskell" | "ocaml" | "scheme" => Lex {
            strings: CHARS,
            nested_comments: true,
            ..DEFAULT_LEX
        },
        // `''` is the escape, so a backslash is an ordinary character.
        "sql" | "pascal" | "fortran" | "fortran90" | "ada" | "vbscript" => Lex {
            escapes: false,
            ..DEFAULT_LEX
        },
        _ => DEFAULT_LEX,
    }
}

/// One language's lexical shape, from the model's tables and `lex_for`.
struct Rules {
    line_comment: String,
    block_start: String,
    block_end: String,
    lex: Lex,
    keywords: &'static [&'static str],
    builtins: &'static [&'static str],
}

impl Rules {
    fn for_language(language: &str) -> Self {
        let (line_comment, block_start, block_end) = set_delims_from_language(language);
        let (keywords, builtins) = keywords::for_language(language);
        Self {
            line_comment,
            block_start,
            block_end,
            lex: lex_for(language),
            keywords,
            builtins,
        }
    }

    /// The opening delimiter to count when a block comment nests.
    fn nesting_open(&self) -> &str {
        match self.lex.nested_comments {
            true => &self.block_start,
            false => "",
        }
    }
}

/// What carries over from one line to the next, inside one region.
#[derive(Default)]
struct State {
    /// The delimiter that closes an open string or block comment.
    target: String,
    /// True while the open target is a comment rather than a string.
    target_is_comment: bool,
    /// How many nested block comments are open, when the language nests them.
    depth: usize,
}

/// A run of lines in one language, as a half-open range.
struct Region {
    start: usize,
    end: usize,
    language: String,
}

/// A body's colouring, and the text it was made from.
///
/// `highlight` parses the whole body, and the body pane redraws on every key.
/// A node holding a function costs microseconds; an `@edit` node holds a whole
/// file in one body, where the parse costs tens of milliseconds. The key is a
/// hash of the input, so any edit invalidates it.
#[derive(Default)]
pub struct Colouring {
    key: Option<u64>,
    spans: Rc<Vec<Vec<Span>>>,
}

impl Colouring {
    /// The colouring of `lines`, recomputed only when they have changed.
    pub fn of(&mut self, lines: &[String], language: &str) -> Rc<Vec<Vec<Span>>> {
        let mut hasher = DefaultHasher::new();
        language.hash(&mut hasher);
        lines.hash(&mut hasher);
        let key = Some(hasher.finish());
        if key != self.key {
            self.key = key;
            self.spans = Rc::new(highlight(lines, language));
        }
        Rc::clone(&self.spans)
    }
}

/// Colour a body.
///
/// `language` is where the body starts, from `Outline::get_language`. The
/// result has one entry per line, holding only the runs that are not plain.
///
/// Two passes. The first claims Leo's own lines -- directives and section
/// references -- and cuts the body into regions, one per `@language` and one
/// either side of a `@nocolor` block. The second colours each region, with
/// tree-sitter where a grammar exists and the line scanner where it does not.
pub fn highlight(lines: &[String], language: &str) -> Vec<Vec<Span>> {
    let mut out: Vec<Vec<Span>> = vec![Vec::new(); lines.len()];
    let (masked, regions) = plan(lines, language, &mut out);
    for region in regions {
        let slice = &masked[region.start..region.end];
        let spans = treesit::highlight(slice, &region.language).unwrap_or_else(|| {
            let rules = Rules::for_language(&region.language);
            let mut state = State::default();
            slice
                .iter()
                .map(|line| scan(line, &rules, &mut state))
                .collect()
        });
        for (k, line_spans) in spans.into_iter().enumerate() {
            if out[region.start + k].is_empty() {
                out[region.start + k] = line_spans;
            }
        }
    }
    out
}

/// Claim Leo's own lines and cut the rest into single-language regions.
///
/// Returns the body with every claimed line blanked. A parser meeting
/// `@others` or `<< a section >>` reports an error there and mis-reads the
/// lines after it; spaces of the same byte width keep every offset intact.
fn plan(lines: &[String], language: &str, out: &mut [Vec<Span>]) -> (Vec<String>, Vec<Region>) {
    let mut masked: Vec<String> = Vec::with_capacity(lines.len());
    let mut regions: Vec<Region> = Vec::new();
    let mut start: Option<usize> = Some(0);
    let mut lang = language.to_string();
    let mut colouring = true;
    let mut killed = false;
    let mut close = |start: &mut Option<usize>, end: usize, lang: &str| {
        if let Some(from) = start.take() {
            if from < end {
                regions.push(Region {
                    start: from,
                    end,
                    language: lang.to_string(),
                });
            }
        }
    };

    for (i, line) in lines.iter().enumerate() {
        if killed {
            masked.push(blanked(line));
            continue;
        }
        if let Some((name, at)) = directive_at(line) {
            out[i] = vec![Span {
                start: at,
                end: line.trim_end().len(),
                class: Class::Directive,
            }];
            masked.push(blanked(line));
            match name {
                "@language" => {
                    let word: String = line[at + name.len()..]
                        .trim()
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    close(&mut start, i, &lang);
                    if !word.is_empty() {
                        lang = word;
                    }
                    // A language change inside a `@nocolor` block does not
                    // end it; only `@color` does.
                    if colouring {
                        start = Some(i + 1);
                    }
                }
                "@nocolor" | "@nocolor-node" => {
                    close(&mut start, i, &lang);
                    colouring = false;
                }
                "@color" => {
                    close(&mut start, i, &lang);
                    colouring = true;
                    start = Some(i + 1);
                }
                "@killcolor" => {
                    close(&mut start, i, &lang);
                    killed = true;
                }
                _ => {}
            }
            continue;
        }
        if start.is_none() {
            masked.push(blanked(line));
            continue;
        }
        if let Some(span) = section_reference(line) {
            out[i] = vec![span];
            masked.push(blanked(line));
            continue;
        }
        masked.push(line.clone());
    }
    close(&mut start, lines.len(), &lang);
    (masked, regions)
}

/// The line's width in spaces, so a parser skips it without losing offsets.
fn blanked(line: &str) -> String {
    " ".repeat(line.len())
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

/// The directive starting `line`, and the column it starts at.
///
/// Leo's `directiveKind4` matches a directive at column 0, and `@others` and
/// `@all` after leading whitespace as well. Those two are the ones that sit
/// inside a class body, and left unclaimed a parser reads `@others` as a
/// decorator on whatever follows it.
fn directive_at(line: &str) -> Option<(&'static str, usize)> {
    if let Some(name) = DIRECTIVES.iter().find(|d| starts_word(line, d)).copied() {
        return Some((name, 0));
    }
    let indent = line.len() - line.trim_start().len();
    ["@others", "@all"]
        .into_iter()
        .find(|d| starts_word(&line[indent..], d))
        .map(|name| (name, indent))
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
        let (class, close) = match state.target_is_comment {
            true => (
                Class::Comment,
                find_block_close(line, 0, rules.nesting_open(), &state.target, state.depth),
            ),
            false => (
                Class::Str,
                find_close(line, 0, &state.target, rules.lex.escapes).ok_or(0),
            ),
        };
        match close {
            Ok(end) => {
                spans.push(Span {
                    start: 0,
                    end,
                    class,
                });
                i = end;
                state.target.clear();
                state.depth = 0;
            }
            Err(depth) => {
                state.depth = depth;
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
            match find_block_close(line, from, rules.nesting_open(), &rules.block_end, 1) {
                Ok(end) => {
                    spans.push(Span {
                        start: i,
                        end,
                        class: Class::Comment,
                    });
                    i = end;
                }
                Err(depth) => {
                    spans.push(Span {
                        start: i,
                        end: line.trim_end_matches('\n').len(),
                        class: Class::Comment,
                    });
                    state.target = rules.block_end.clone();
                    state.target_is_comment = true;
                    state.depth = depth;
                    return spans;
                }
            }
            continue;
        }
        // A string, taking the longest delimiter that matches.
        if let Some(delim) = rules.lex.strings.iter().find(|d| rest.starts_with(**d)) {
            flush_word(line, &mut run_start, i, rules, &mut spans);
            let from = i + delim.len();
            match find_close(line, from, delim, rules.lex.escapes) {
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
                    state.target = (*delim).to_string();
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

/// The offset just past the string's closing delimiter.
///
/// `escapes` is false for the languages that double the quote instead, where
/// a backslash is an ordinary character and skipping past it would run the
/// string on to the end of the body.
fn find_close(line: &str, from: usize, close: &str, escapes: bool) -> Option<usize> {
    if close.is_empty() {
        return None;
    }
    let mut i = from;
    while i < line.len() {
        let rest = &line[i..];
        if escapes {
            if let Some(after) = rest.strip_prefix('\\') {
                // The escape takes the next character with it.
                i += 1 + after.chars().next().map(|c| c.len_utf8()).unwrap_or(0);
                continue;
            }
        }
        if rest.starts_with(close) {
            return Some(i + close.len());
        }
        i += rest.chars().next().unwrap().len_utf8();
    }
    None
}

/// The offset just past the block comment's close, or the depth still open.
///
/// `open` is empty when the language's comments do not nest, which makes the
/// first close end the comment. Backslashes are ordinary here: `/* \*/` ends
/// the comment in C, and treating the escape as a string's would have run it
/// on.
fn find_block_close(
    line: &str,
    from: usize,
    open: &str,
    close: &str,
    mut depth: usize,
) -> Result<usize, usize> {
    if close.is_empty() {
        return Err(depth);
    }
    let mut i = from;
    while i < line.len() {
        let rest = &line[i..];
        if rest.starts_with(close) {
            depth -= 1;
            i += close.len();
            if depth == 0 {
                return Ok(i);
            }
            continue;
        }
        if !open.is_empty() && rest.starts_with(open) {
            depth += 1;
            i += open.len();
            continue;
        }
        i += rest.chars().next().unwrap().len_utf8();
    }
    Err(depth)
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
            vec![
                ("def", Class::Keyword),
                ("f", Class::Function),
                ("# note", Class::Comment)
            ]
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
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![("int", Class::Type), ("/* one", Class::Comment)]
        );
        assert_eq!(
            classes(&src[1], &out[1]),
            vec![("two */", Class::Comment), ("int", Class::Type)]
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
        assert_eq!(classes(&src[3], &out[3])[0], ("int", Class::Type));
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

    #[test]
    fn rust_strings_are_coloured_and_hide_a_line_comment() {
        // The importers give Rust an empty string_list, which left `//` inside
        // a literal opening a comment that ran to the end of the line.
        let src = lines("let s = \"a // b\"; // c");
        let out = highlight(&src, "rust");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![
                ("let", Class::Keyword),
                ("\"a // b\"", Class::Str),
                ("// c", Class::Comment)
            ]
        );
    }

    #[test]
    fn a_rust_lifetime_does_not_open_a_string() {
        let src = lines("fn f<'a>(s: &'a str) -> u8 { 1 }");
        let out = highlight(&src, "rust");
        assert!(out[0].iter().all(|s| s.class != Class::Str));
        assert_eq!(out[0].last().unwrap().class, Class::Number);
    }

    #[test]
    fn rust_block_comments_nest() {
        let src = lines("/* a /* b */ still */ let x = 1;");
        let out = highlight(&src, "rust");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![
                ("/* a /* b */ still */", Class::Comment),
                ("let", Class::Keyword),
                ("1", Class::Number)
            ]
        );
    }

    #[test]
    fn a_nested_comment_carries_its_depth_across_lines() {
        let src = lines("/* a /* b\nc */ d */ let x = 1;");
        let out = highlight(&src, "rust");
        assert_eq!(out[0][0].class, Class::Comment);
        assert_eq!(
            classes(&src[1], &out[1]),
            vec![
                ("c */ d */", Class::Comment),
                ("let", Class::Keyword),
                ("1", Class::Number)
            ]
        );
    }

    #[test]
    fn c_block_comments_do_not_nest() {
        let src = lines("/* a /* b */ int x;");
        let out = highlight(&src, "c");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![("/* a /* b */", Class::Comment), ("int", Class::Type)]
        );
    }

    #[test]
    fn a_backslash_does_not_end_a_c_block_comment_early_or_late() {
        // A backslash is not an escape inside a comment, so `*/` still closes.
        let src = lines("/* a \\*/ int x;");
        let out = highlight(&src, "c");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![("/* a \\*/", Class::Comment), ("int", Class::Type)]
        );
    }

    #[test]
    fn a_language_without_backslash_escapes_closes_its_string() {
        // SQL doubles the quote instead, so a trailing backslash is ordinary.
        let src = lines("select 'c:\\' , 1");
        let out = highlight(&src, "sql");
        assert_eq!(out[0].iter().filter(|s| s.class == Class::Str).count(), 1);
        assert_eq!(out[0].last().unwrap().class, Class::Number);
    }

    #[test]
    fn cplusplus_char_literals_do_not_open_a_string() {
        // Leo's language name is `cplusplus`; the importers only register `c`,
        // so this fell through to the default list that treats `'` as a quote.
        let src = lines("char c = 'x'; int n = 1;");
        let out = highlight(&src, "cplusplus");
        assert!(out[0].iter().all(|s| s.class != Class::Str));
        assert_eq!(out[0].last().unwrap().class, Class::Number);
    }

    // Leo knows delimiters for some 170 languages and only a dozen have a
    // grammar compiled in, so the line scanner still colours most of them.
    // These pin its own rules to languages tree-sitter does not claim.

    #[test]
    fn the_scanner_nests_the_comments_of_a_language_that_does() {
        let src = lines("{- a {- b -} still -} data X = 1");
        let out = highlight(&src, "haskell");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![
                ("{- a {- b -} still -}", Class::Comment),
                ("data", Class::Keyword),
                ("1", Class::Number)
            ]
        );
    }

    #[test]
    fn the_scanner_does_not_nest_the_comments_of_a_language_that_does_not() {
        let src = lines("/* a /* b */ int x;");
        let out = highlight(&src, "objective_c");
        assert_eq!(out[0][0].class, Class::Comment);
        assert_eq!(&src[0][out[0][0].start..out[0][0].end], "/* a /* b */");
    }

    #[test]
    fn the_scanner_treats_a_char_literal_as_no_string_at_all() {
        let src = lines("char c = 'x'; int n = 1;");
        let out = highlight(&src, "objective_c");
        assert!(out[0].iter().all(|s| s.class != Class::Str));
        assert_eq!(out[0].last().unwrap().class, Class::Number);
    }

    // A parse tree says what a table lookup cannot: which identifier is a
    // function, a type or a field. These cover the languages with a grammar
    // and the masking that gets a Leo body through a parser.

    #[test]
    fn a_body_that_is_only_a_method_body_is_still_coloured() {
        // The common Leo shape: no class above it, no `def`, just the body.
        let src = lines("self.count += 1\nreturn self.total(\"x\")  # done");
        let out = highlight(&src, "python");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![("count", Class::Property), ("1", Class::Number)]
        );
        assert_eq!(
            classes(&src[1], &out[1]),
            vec![
                ("return", Class::Keyword),
                ("total", Class::Property),
                ("\"x\"", Class::Str),
                ("# done", Class::Comment)
            ]
        );
    }

    #[test]
    fn a_parse_tree_tells_a_type_from_a_function_from_a_field() {
        let src = lines("struct P { n: u8 }\nfn f(p: P) -> u8 { g(p.n) }");
        let out = highlight(&src, "rust");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![
                ("struct", Class::Keyword),
                ("P", Class::Type),
                ("n", Class::Property),
                ("u8", Class::Builtin)
            ]
        );
        assert_eq!(
            classes(&src[1], &out[1]),
            vec![
                ("fn", Class::Keyword),
                ("f", Class::Function),
                ("P", Class::Type),
                ("u8", Class::Builtin),
                ("g", Class::Function),
                ("n", Class::Property)
            ]
        );
    }

    #[test]
    fn an_attribute_is_told_from_the_code_it_sits_on() {
        let src = lines("#[derive(Debug)]\nstruct P;");
        let out = highlight(&src, "rust");
        assert_eq!(out[0][0].class, Class::Attribute);
        assert_eq!(classes(&src[1], &out[1])[1], ("P", Class::Type));
    }

    #[test]
    fn an_indented_at_others_is_a_directive_and_spoils_nothing_below_it() {
        // Leo's `directiveKind4` allows leading whitespace before `@others`,
        // and unclaimed it reads as a decorator on the `def` beneath it.
        let src = lines("class Widget:\n    @others\n    def later(self):\n        return 1");
        let out = highlight(&src, "python");
        assert_eq!(
            classes(&src[1], &out[1]),
            vec![("@others", Class::Directive)]
        );
        assert_eq!(
            classes(&src[2], &out[2]),
            vec![("def", Class::Keyword), ("later", Class::Function)]
        );
    }

    #[test]
    fn a_section_reference_spoils_nothing_below_it() {
        // Unmasked, the parser reads `>> return` as one expression and
        // `return` stops being a keyword.
        let src = lines("def f():\n    << do the work >>\n    return 1");
        let out = highlight(&src, "python");
        assert_eq!(
            classes(&src[1], &out[1]),
            vec![("<< do the work >>", Class::Section)]
        );
        assert_eq!(
            classes(&src[2], &out[2]),
            vec![("return", Class::Keyword), ("1", Class::Number)]
        );
    }

    #[test]
    fn a_grammar_colours_one_region_and_the_scanner_the_next() {
        let src = lines("def f():\n    pass\n@language haskell\n{- a comment -}\ndata X = 1");
        let out = highlight(&src, "python");
        assert_eq!(
            classes(&src[0], &out[0]),
            vec![("def", Class::Keyword), ("f", Class::Function)]
        );
        assert_eq!(
            classes(&src[3], &out[3]),
            vec![("{- a comment -}", Class::Comment)]
        );
        assert_eq!(classes(&src[4], &out[4])[0], ("data", Class::Keyword));
    }

    #[test]
    fn the_kept_colouring_follows_an_edit_and_a_language_change() {
        let mut colouring = Colouring::default();
        let src = lines("def f():");
        assert_eq!(colouring.of(&src, "python")[0][0].class, Class::Keyword);
        let src = lines("# f");
        assert_eq!(colouring.of(&src, "python")[0][0].class, Class::Comment);
        // The same text again, in a language that has no comment delimiter.
        assert!(colouring.of(&src, "not_a_language")[0].is_empty());
    }

    #[test]
    fn a_language_change_inside_a_nocolor_block_does_not_end_it() {
        // The change still lands: `@color` resumes in C, not in Python.
        let src = lines("@nocolor\ndef a():\n@language c\nint x;\n@color\nint y;");
        let out = highlight(&src, "python");
        assert!(out[1].is_empty(), "{:?}", out[1]);
        assert!(out[3].is_empty(), "{:?}", out[3]);
        assert_eq!(classes(&src[5], &out[5])[0], ("int", Class::Type));
    }
}
