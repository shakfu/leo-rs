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

use once_cell::sync::Lazy;
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

use crate::highlight::{Class, Span};

/// The capture names this colours, and what each is drawn as.
///
/// `tree_sitter_highlight` resolves a capture to the longest of these that
/// prefixes it, so `function.method` lands on `function` and only a name that
/// needs its own colour is listed. A capture matching nothing here is left
/// plain, which is what `variable`, `operator` and `punctuation` want.
const CAPTURES: &[(&str, Class)] = &[
    ("attribute", Class::Attribute),
    ("comment", Class::Comment),
    // Grammars disagree about what a literal is: tree-sitter-python tags `1`
    // as `number`, tree-sitter-rust tags it `constant.builtin`. One class for
    // all of them keeps a number the same colour in every language.
    ("constant", Class::Number),
    ("constant.builtin", Class::Number),
    ("constructor", Class::Type),
    ("escape", Class::Str),
    ("function", Class::Function),
    ("function.builtin", Class::Builtin),
    ("keyword", Class::Keyword),
    ("number", Class::Number),
    ("property", Class::Property),
    ("string", Class::Str),
    ("tag", Class::Type),
    ("type", Class::Type),
    ("type.builtin", Class::Builtin),
    // CSS names its at-rules rather than calling them keywords.
    ("charset", Class::Keyword),
    ("import", Class::Keyword),
    ("keyframes", Class::Keyword),
    ("media", Class::Keyword),
    ("supports", Class::Keyword),
];

static NAMES: Lazy<Vec<&'static str>> = Lazy::new(|| CAPTURES.iter().map(|(n, _)| *n).collect());

thread_local! {
    static HIGHLIGHTER: RefCell<Highlighter> = RefCell::new(Highlighter::new());
}

fn configure(name: &str, language: tree_sitter::Language, query: &str) -> HighlightConfiguration {
    let mut config = HighlightConfiguration::new(language, name, query, "", "")
        .expect("a bundled grammar's own highlight query");
    config.configure(&NAMES);
    config
}

/// Build one grammar's configuration once, on first use.
macro_rules! grammar {
    ($name:literal, $language:expr, $query:expr) => {{
        static CONFIG: Lazy<HighlightConfiguration> =
            Lazy::new(|| configure($name, $language, $query));
        Some(&*CONFIG)
    }};
}

/// The grammar for `language`, under Leo's name for it.
fn config_for(language: &str) -> Option<&'static HighlightConfiguration> {
    match language {
        "c" => grammar!(
            "c",
            tree_sitter_c::LANGUAGE.into(),
            tree_sitter_c::HIGHLIGHT_QUERY
        ),
        // C++ and TypeScript ship only what they add to the language they
        // extend, so each query is the two concatenated.
        "cplusplus" => grammar!(
            "cpp",
            tree_sitter_cpp::LANGUAGE.into(),
            &format!(
                "{}{}",
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
            tree_sitter_go::HIGHLIGHTS_QUERY
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
            tree_sitter_python::HIGHLIGHTS_QUERY
        ),
        "rust" => grammar!(
            "rust",
            tree_sitter_rust::LANGUAGE.into(),
            tree_sitter_rust::HIGHLIGHTS_QUERY
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

/// Colour `lines` as `language`, one entry of spans per line.
///
/// None when the language has no grammar. The caller falls back to the line
/// scanner, which is also what the ~170 languages Leo knows delimiters for get.
pub fn highlight(lines: &[String], language: &str) -> Option<Vec<Vec<Span>>> {
    let config = config_for(language)?;
    let text = lines.join("\n");
    let mut starts = Vec::with_capacity(lines.len());
    let mut at = 0usize;
    for line in lines {
        starts.push(at);
        at += line.len() + 1;
    }

    // `Highlighter::new` allocates a parser, which cost more than colouring a
    // short body did. The body pane re-colours on every frame, so keep one.
    HIGHLIGHTER.with(|cell| {
        let mut highlighter = cell.borrow_mut();
        let events = highlighter
            .highlight(config, text.as_bytes(), None, |_| None)
            .ok()?;
        let mut out: Vec<Vec<Span>> = vec![Vec::new(); lines.len()];
        let mut open: Vec<Class> = Vec::new();
        for event in events {
            match event.ok()? {
                HighlightEvent::HighlightStart(h) => open.push(CAPTURES[h.0].1),
                HighlightEvent::HighlightEnd => {
                    open.pop();
                }
                HighlightEvent::Source { start, end } => {
                    if let Some(class) = open.last().copied() {
                        push_run(&mut out, lines, &starts, start, end, class);
                    }
                }
            }
        }
        Some(out)
    })
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
