//! Tree-sitter colouring, for the languages with a grammar compiled in.
//!
//! The line scanner in `highlight` classifies a word by looking it up in a
//! table, so it can say `keyword` and `library name` and nothing else. A parse
//! tree says which identifier is a function, a type, a field or a decorator.
//! That is the whole reason this module exists; everything else here is the
//! cost of getting a parse out of a Leo body.
//!
//! A body is a fragment: a method with no class above it, a class whose
//! methods are `@others`, a function whose middle is `<< a section >>`. Two of
//! those three parse cleanly -- tree-sitter's error recovery handles a missing
//! enclosing scope well. Leo's own constructs do not, and an error node there
//! mis-reads the lines after it, so `highlight` blanks them before calling in.

use std::cell::RefCell;
use std::ops::Range;

use once_cell::sync::Lazy;
use streaming_iterator::StreamingIterator;
use tree_sitter::{
    InputEdit, Language, Node, ParseOptions, Parser, Point, Query, QueryCursor, Tree,
};

use crate::highlight::{Class, Span};

/// The capture names this colours, and what each is drawn as.
///
/// A capture resolves to the longest of these whose every part it contains, so
/// `function.method` lands on `function` and only a name that needs its own
/// colour is listed. A capture matching nothing here is left plain, which is
/// what `variable`, `operator` and `punctuation` want.
const CAPTURES: &[(&str, Class)] = &[
    ("attribute", Class::Attribute),
    ("comment", Class::Comment),
    ("constant", Class::Number),
    ("constant.builtin", Class::BuiltinConstant),
    ("constructor", Class::Type),
    ("escape", Class::Str),
    ("function", Class::Function),
    ("function.builtin", Class::BuiltinFunction),
    ("keyword", Class::Keyword),
    ("number", Class::Number),
    ("property", Class::Property),
    ("string", Class::Str),
    ("tag", Class::Type),
    ("type", Class::Type),
    ("type.builtin", Class::BuiltinType),
    // CSS names its at-rules rather than calling them keywords.
    ("charset", Class::Keyword),
    ("import", Class::Keyword),
    ("keyframes", Class::Keyword),
    ("media", Class::Keyword),
    ("supports", Class::Keyword),
];

thread_local! {
    // A new parser allocates, which cost more than colouring a short body did.
    // The body pane re-colours on every frame, so keep one.
    static PARSER: RefCell<Parser> = RefCell::new(Parser::new());
    static CURSOR: RefCell<QueryCursor> = RefCell::new(QueryCursor::new());
}

/// One grammar, with its query and what each of the query's captures draws as.
struct Grammar {
    language: Language,
    query: Query,
    classes: Vec<Option<Class>>,
}

fn class_of(capture: &str) -> Option<Class> {
    let parts: Vec<&str> = capture.split('.').collect();
    let mut best = None;
    let mut best_len = 0;
    for (name, class) in CAPTURES {
        let len = name.split('.').count();
        if len > best_len && name.split('.').all(|part| parts.contains(&part)) {
            best = Some(*class);
            best_len = len;
        }
    }
    best
}

fn configure(language: Language, source: &str) -> Grammar {
    let query = Query::new(&language, source).expect("a bundled grammar's own highlight query");
    let classes = query.capture_names().iter().map(|c| class_of(c)).collect();
    Grammar {
        language,
        query,
        classes,
    }
}

/// Build one grammar's query once, on first use.
macro_rules! grammar {
    ($name:literal, $language:expr, $query:expr) => {{
        static GRAMMAR: Lazy<Grammar> = Lazy::new(|| configure($language, $query));
        Some(&*GRAMMAR)
    }};
}

/// tree-sitter-rust tags every literal `@constant.builtin`, numbers included.
const RUST_NUMBERS: &str = "
(integer_literal) @constant.numeric
(float_literal) @constant.numeric
";

/// tree-sitter-c tags its primitives `@type`, as it tags a typedef's name.
/// C++'s query starts with C's, so it gets these too.
const C_BUILTIN_TYPES: &str = "
(primitive_type) @type.builtin
(sized_type_specifier) @type.builtin
";

/// tree-sitter-go tags a predeclared type as any other type name.
const GO_BUILTIN_TYPES: &str = r#"
((type_identifier) @type.builtin
 (#match? @type.builtin "^(any|bool|byte|comparable|complex64|complex128|error|float32|float64|int|int8|int16|int32|int64|rune|string|uint|uint8|uint16|uint32|uint64|uintptr)$"))
"#;

/// tree-sitter-python tags a builtin type in an annotation as any other.
/// Called, the same names are `@function.builtin`.
const PYTHON_BUILTIN_TYPES: &str = r#"
(type (identifier) @type.builtin
 (#match? @type.builtin "^(bool|bytearray|bytes|complex|dict|float|frozenset|int|list|memoryview|object|range|set|slice|str|tuple|type)$"))
"#;

/// The grammar for `language`, under Leo's name for it.
fn grammar_for(language: &str) -> Option<&'static Grammar> {
    match language {
        // A pattern appended after a grammar's own wins over it.
        "c" => grammar!(
            "c",
            tree_sitter_c::LANGUAGE.into(),
            &format!("{}{C_BUILTIN_TYPES}", tree_sitter_c::HIGHLIGHT_QUERY)
        ),
        // C++ and TypeScript ship only what they add to the language they
        // extend, so each query is the two concatenated.
        "cplusplus" => grammar!(
            "cpp",
            tree_sitter_cpp::LANGUAGE.into(),
            &format!(
                "{}{C_BUILTIN_TYPES}{}",
                tree_sitter_c::HIGHLIGHT_QUERY,
                tree_sitter_cpp::HIGHLIGHT_QUERY
            )
        ),
        "css" => grammar!(
            "css",
            tree_sitter_css::LANGUAGE.into(),
            tree_sitter_css::HIGHLIGHTS_QUERY
        ),
        "go" => grammar!(
            "go",
            tree_sitter_go::LANGUAGE.into(),
            &format!("{}{GO_BUILTIN_TYPES}", tree_sitter_go::HIGHLIGHTS_QUERY)
        ),
        "html" => grammar!(
            "html",
            tree_sitter_html::LANGUAGE.into(),
            tree_sitter_html::HIGHLIGHTS_QUERY
        ),
        "java" => grammar!(
            "java",
            tree_sitter_java::LANGUAGE.into(),
            tree_sitter_java::HIGHLIGHTS_QUERY
        ),
        "javascript" => grammar!(
            "javascript",
            tree_sitter_javascript::LANGUAGE.into(),
            tree_sitter_javascript::HIGHLIGHT_QUERY
        ),
        "json" => grammar!(
            "json",
            tree_sitter_json::LANGUAGE.into(),
            tree_sitter_json::HIGHLIGHTS_QUERY
        ),
        "python" => grammar!(
            "python",
            tree_sitter_python::LANGUAGE.into(),
            &format!(
                "{}{PYTHON_BUILTIN_TYPES}",
                tree_sitter_python::HIGHLIGHTS_QUERY
            )
        ),
        "rust" => grammar!(
            "rust",
            tree_sitter_rust::LANGUAGE.into(),
            &format!("{}{}", tree_sitter_rust::HIGHLIGHTS_QUERY, RUST_NUMBERS)
        ),
        "shell" | "shellscript" => grammar!(
            "bash",
            tree_sitter_bash::LANGUAGE.into(),
            tree_sitter_bash::HIGHLIGHT_QUERY
        ),
        "typescript" => grammar!(
            "typescript",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            &format!(
                "{}{}",
                tree_sitter_javascript::HIGHLIGHT_QUERY,
                tree_sitter_typescript::HIGHLIGHTS_QUERY
            )
        ),
        _ => None,
    }
}

/// One region's last parse, kept so the next can reuse it.
pub(crate) struct Parsed {
    language: String,
    text: String,
    tree: Tree,
    spans: Vec<Vec<Span>>,
}

/// Colour `lines` as `language`, one entry of spans per line.
///
/// None when the language has no grammar. The caller falls back to the line
/// scanner, which is also what the ~170 languages Leo knows delimiters for get.
pub fn highlight(lines: &[String], language: &str) -> Option<Vec<Vec<Span>>> {
    let grammar = grammar_for(language)?;
    let text = lines.join("\n");
    let tree = parse(grammar, &text, None, &|| false)?;
    Some(query_lines(grammar, &tree, &text, lines, 0..lines.len()))
}

/// True if `language` has a grammar.
pub(crate) fn has_grammar(language: &str) -> bool {
    grammar_for(language).is_some()
}

/// Bring `kept` up to `lines` and return their colouring, reusing its tree and
/// the spans of the lines an edit cannot have changed.
///
/// `lines` must be in a language with a grammar. None if `cancelled` stopped
/// the parse, which leaves `kept` as it was.
pub(crate) fn reparse<'a>(
    kept: &'a mut Option<Parsed>,
    lines: &[String],
    language: &str,
    cancelled: &dyn Fn() -> bool,
) -> Option<&'a [Vec<Span>]> {
    let grammar = grammar_for(language)?;
    let text = lines.join("\n");
    let old = kept.as_ref().filter(|old| old.language == language);
    let next = match old {
        Some(old) if old.text == text => None,
        Some(old) => {
            let edit = edit_between(&old.text, &text);
            let mut edited = old.tree.clone();
            edited.edit(&edit);
            let mut tree = parse(grammar, &text, Some(&edited), cancelled)?;
            // Error recovery depends on the parse's history: an incremental
            // tree can differ from a fresh one, and its colours stay wrong
            // until a later edit. Only a tree holding an error differs.
            if tree.root_node().has_error() {
                tree = parse(grammar, &text, None, cancelled)?;
            }
            let mut changed = edit.start_byte..edit.new_end_byte;
            for range in edited.changed_ranges(&tree) {
                changed.start = changed.start.min(range.start_byte);
                changed.end = changed.end.max(range.end_byte);
            }
            let Range { start, end } = changed_lines(&tree, lines, changed);
            let fresh = query_lines(grammar, &tree, &text, lines, start..end);
            let tail = lines.len() - end;
            let mut spans = Vec::with_capacity(lines.len());
            spans.extend_from_slice(&old.spans[..start]);
            spans.extend(fresh);
            spans.extend_from_slice(&old.spans[old.spans.len() - tail..]);
            Some((tree, spans))
        }
        None => {
            let tree = parse(grammar, &text, None, cancelled)?;
            let spans = query_lines(grammar, &tree, &text, lines, 0..lines.len());
            Some((tree, spans))
        }
    };
    if let Some((tree, spans)) = next {
        *kept = Some(Parsed {
            language: language.to_string(),
            text,
            tree,
            spans,
        });
    }
    kept.as_ref().map(|k| k.spans.as_slice())
}

fn parse(
    grammar: &Grammar,
    text: &str,
    old: Option<&Tree>,
    cancelled: &dyn Fn() -> bool,
) -> Option<Tree> {
    let bytes = text.as_bytes();
    PARSER.with(|cell| {
        let mut parser = cell.borrow_mut();
        parser.set_language(&grammar.language).ok()?;
        let mut stop = |_: &tree_sitter::ParseState| cancelled();
        let tree = parser.parse_with_options(
            &mut |i, _| bytes.get(i..).unwrap_or_default(),
            old,
            Some(ParseOptions::new().progress_callback(&mut stop)),
        );
        // A stopped parser would otherwise resume on the next call's text.
        if tree.is_none() {
            parser.reset();
        }
        tree
    })
}

/// The one edit that turns `old` into `new`: what lies between their common
/// prefix and suffix, on char boundaries. A multi-site change is one wide edit.
fn edit_between(old: &str, new: &str) -> InputEdit {
    let (a, b) = (old.as_bytes(), new.as_bytes());
    let mut prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    while !(old.is_char_boundary(prefix) && new.is_char_boundary(prefix)) {
        prefix -= 1;
    }
    let room = a.len().min(b.len()) - prefix;
    let mut suffix = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take(room)
        .take_while(|(x, y)| x == y)
        .count();
    while !(old.is_char_boundary(a.len() - suffix) && new.is_char_boundary(b.len() - suffix)) {
        suffix -= 1;
    }
    let start = point_after(Point::new(0, 0), &a[..prefix]);
    InputEdit {
        start_byte: prefix,
        old_end_byte: a.len() - suffix,
        new_end_byte: b.len() - suffix,
        start_position: start,
        old_end_position: point_after(start, &a[prefix..a.len() - suffix]),
        new_end_position: point_after(start, &b[prefix..b.len() - suffix]),
    }
}

fn point_after(from: Point, bytes: &[u8]) -> Point {
    let newlines = bytes.iter().filter(|b| **b == b'\n').count();
    match bytes.iter().rposition(|b| *b == b'\n') {
        Some(last) => Point::new(from.row + newlines, bytes.len() - last - 1),
        None => Point::new(from.row, from.column + bytes.len()),
    }
}

/// The whole lines to query again for a change to bytes `changed`.
///
/// Widened to whole top-level nodes: a pattern may match across siblings, and
/// `changed_ranges` reports only the sibling that changed.
fn changed_lines(tree: &Tree, lines: &[String], changed: Range<usize>) -> Range<usize> {
    let mut starts = Vec::with_capacity(lines.len());
    let mut at = 0usize;
    for line in lines {
        starts.push(at);
        at += line.len() + 1;
    }
    let line_of = |byte: usize| starts.partition_point(|s| *s <= byte).saturating_sub(1);
    let root = tree.root_node();
    let mut cursor = root.walk();
    let children: Vec<Node> = root.children(&mut cursor).collect();
    let (mut first, mut last) = (line_of(changed.start), line_of(changed.end));
    loop {
        let (from, to) = (starts[first], starts[last] + lines[last].len());
        let (mut a, mut b) = (first, last);
        for child in children
            .iter()
            .filter(|c| c.start_byte() <= to && c.end_byte() >= from)
        {
            a = a.min(line_of(child.start_byte()));
            b = b.max(line_of(child.end_byte()));
        }
        if (a, b) == (first, last) {
            return first..last + 1;
        }
        (first, last) = (a, b);
    }
}

/// The colouring of lines `wanted` of `text`, which is `lines` joined.
///
/// Drives the captures as `tree_sitter_highlight` does: a later pattern on the
/// same node replaces an earlier one and ends that earlier match, and the
/// innermost open capture colours each byte.
fn query_lines(
    grammar: &Grammar,
    tree: &Tree,
    text: &str,
    lines: &[String],
    wanted: Range<usize>,
) -> Vec<Vec<Span>> {
    let mut starts = Vec::with_capacity(wanted.len());
    let mut at = 0usize;
    for (i, line) in lines.iter().enumerate() {
        if wanted.contains(&i) {
            starts.push(at);
        }
        at += line.len() + 1;
    }
    let shown = &lines[wanted.clone()];
    let mut out: Vec<Vec<Span>> = vec![Vec::new(); shown.len()];
    let range = match shown.last() {
        Some(last) => starts[0]..starts[starts.len() - 1] + last.len(),
        None => return out,
    };

    CURSOR.with(|cell| {
        let mut cursor = cell.borrow_mut();
        // The range decides which matches come back. A whole query asks for
        // everything, since a run's lines outside `wanted` are dropped anyway.
        match wanted.len() == lines.len() {
            true => cursor.set_byte_range(0..usize::MAX),
            false => cursor.set_byte_range(range),
        };
        let mut byte = 0usize;
        let mut open: Vec<(usize, Class)> = Vec::new();
        let mut removed: Vec<u32> = Vec::new();
        // The capture waiting to see whether a later pattern replaces it.
        let mut pending: Option<(Node, Option<Class>, u32)> = None;

        let mut emit = |byte: &mut usize, to: usize, class: Option<Class>| {
            if *byte < to {
                if let Some(class) = class {
                    push_run(&mut out, shown, &starts, *byte, to, class);
                }
                *byte = to;
            }
        };
        let top = |open: &[(usize, Class)]| open.last().map(|(_, class)| *class);

        let mut captures = cursor.captures(&grammar.query, tree.root_node(), text.as_bytes());
        while let Some((found, index)) = captures.next() {
            if removed.contains(&found.id()) {
                continue;
            }
            let capture = found.captures[*index];
            let class = grammar.classes[capture.index as usize];
            if let Some((node, _, id)) = pending {
                if node == capture.node {
                    removed.push(id);
                    pending = Some((node, class, found.id()));
                    continue;
                }
                if let Some(Some(class)) = pending.map(|(_, class, _)| class) {
                    emit(&mut byte, node.start_byte(), top(&open));
                    open.push((node.end_byte(), class));
                }
            }
            let start = capture.node.start_byte();
            while let Some((end, _)) = open.last().copied().filter(|(end, _)| *end <= start) {
                emit(&mut byte, end, top(&open));
                open.pop();
            }
            pending = Some((capture.node, class, found.id()));
        }
        if let Some((node, Some(class), _)) = pending {
            emit(&mut byte, node.start_byte(), top(&open));
            open.push((node.end_byte(), class));
        }
        while let Some((end, _)) = open.last().copied() {
            emit(&mut byte, end, top(&open));
            open.pop();
        }
    });
    out
}

/// Cut one classified run of the joined text back into per-line spans.
///
/// A block comment or a triple-quoted string is one run over several lines,
/// and `Span` offsets are relative to a line.
fn push_run(
    out: &mut [Vec<Span>],
    lines: &[String],
    starts: &[usize],
    start: usize,
    end: usize,
    class: Class,
) {
    let mut i = match starts.binary_search(&start) {
        Ok(i) => i,
        Err(i) => i.saturating_sub(1),
    };
    while i < lines.len() && starts[i] < end {
        let base = starts[i];
        let a = start.max(base) - base;
        let b = end.min(base + lines[i].len()).saturating_sub(base);
        if b > a {
            match out[i].last_mut() {
                Some(prev) if prev.class == class && prev.end == a => prev.end = b,
                _ => out[i].push(Span {
                    start: a,
                    end: b,
                    class,
                }),
            }
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_edit_turns_old_into_new_on_char_boundaries_with_true_points() {
        // Every pair of strings up to length 3 over a mixed-width alphabet.
        let alphabet = ["a", "\n", "\u{e9}", "\u{e8}", "\u{4e2d}"];
        let mut all = vec![String::new()];
        for _ in 0..3 {
            let mut more = Vec::new();
            for s in all.iter().rev().take(125) {
                for c in alphabet {
                    more.push(format!("{s}{c}"));
                }
            }
            all.extend(more);
        }
        let point = |s: &str, at: usize| point_after(Point::new(0, 0), &s.as_bytes()[..at]);
        for old in &all {
            for new in &all {
                let e = edit_between(old, new);
                let rebuilt = format!(
                    "{}{}{}",
                    &old[..e.start_byte],
                    &new[e.start_byte..e.new_end_byte],
                    &old[e.old_end_byte..]
                );
                assert_eq!(&rebuilt, new);
                assert_eq!(e.start_position, point(old, e.start_byte));
                assert_eq!(e.old_end_position, point(old, e.old_end_byte));
                assert_eq!(e.new_end_position, point(new, e.new_end_byte));
            }
        }
    }

    #[test]
    fn an_edit_between_equal_texts_is_empty() {
        let e = edit_between("same", "same");
        assert_eq!(e.start_byte, e.old_end_byte);
        assert_eq!(e.start_byte, e.new_end_byte);
    }
}
