//! `@auto` importers: rebuilding an outline from a file with no sentinels.
//!
//! An `@auto` file is the user's own source, untouched by Leo. Nothing in it
//! says where one node ends and the next begins, so the structure has to come
//! from the language. Leo answers that with 34 importers under
//! `leo/plugins/importers`; almost all of them are the same algorithm with a
//! different table of block patterns, which is what [`block`] implements and
//! [`LanguageSpec`] configures.
//!
//! The contract an importer must keep: the nodes it creates, tangled back with
//! no sentinels, reproduce the file it read. [`import_file`] checks that and
//! refuses a tree that fails, because an `@auto` node that does not round-trip
//! overwrites the user's file with something else the next time it is written.

pub mod block;
pub mod lines;
pub mod python;
pub mod rust_lang;

use once_cell::sync::Lazy;
use regex::Regex;

use crate::error::{Error, Result};
use crate::outline::Outline;
use crate::position::Position;
use crate::util;

/// How guide lines are made: which comment and string forms to blank out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuideKind {
    /// Comments and strings, using the language's delimiters.
    Default,
    /// Python: triple-quoted strings and f-string prefixes.
    Python,
    /// JavaScript: also blanks out apparent regular expressions.
    JavaScript,
    /// Perl: also blanks out regular expressions.
    Perl,
    /// Rust: nested block comments, raw strings, lifetimes.
    Rust,
}

/// How the end of a block is found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndOfBlock {
    /// Matching curly brackets. The default.
    Braces,
    /// A dedent, as in Python and coffeescript.
    Indent,
    /// Matching parentheses, as in lisp.
    Parens,
    /// A matching `end` keyword, as in Lua.
    LuaEnd,
    /// The start of the next block, as in Pascal and .ini files.
    NextBlock,
    /// Matching open and close tags, as in XML.
    Tags,
}

/// Which `find_blocks` variant a language needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindBlocks {
    Default,
    /// Python: never nest a `def` inside a `def`.
    Python,
    /// C: a definition whose `{` is on a later line.
    C,
    /// Rust: as C, but the `{` may be several lines down.
    Rust,
}

/// Language-specific work after the tree is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Postprocess {
    /// Move blank lines to the end of the previous sibling. Every language.
    BlankLines,
    Python,
    /// Move the module preamble into the parent.
    Preamble,
    Rust,
}

/// How a block's headline is computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadlineKind {
    /// `kind name`, or the name alone when the kind is empty.
    Default,
    /// Rust: strip generic parameters from the name.
    Rust,
    /// XML: the opening tag line, truncated.
    Tag,
}

/// A whole importer that does not use the block algorithm at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineImporter {
    Org,
    Otl,
    Markdown,
    Treepad,
}

/// Everything that distinguishes one language's importer from another.
pub struct LanguageSpec {
    /// The `@language` directive the importer writes. Leo's `language` ivar.
    pub language: &'static str,
    /// `(kind, pattern)` pairs. Group 1 of the pattern is the block's name.
    pub block_patterns: Vec<(&'static str, Regex)>,
    /// String delimiters, longest first.
    pub string_list: Vec<&'static str>,
    /// Names that look like blocks but are control flow, such as `if`.
    pub compound_statements: Vec<&'static str>,
    /// Blocks smaller than this are not split out. 0 creates every block.
    pub minimum_block_size: usize,
    pub guide_kind: GuideKind,
    pub end_of_block: EndOfBlock,
    pub find_blocks: FindBlocks,
    pub postprocess: Postprocess,
    pub headline: HeadlineKind,
    /// Set for the four importers that do not use blocks at all.
    pub line_importer: Option<LineImporter>,
    /// Never warn about mixed blanks and tabs for these languages.
    pub allow_mixed_whitespace: bool,
    /// File extensions, with the leading period.
    pub extensions: &'static [&'static str],
    /// `@auto-<name>` spellings this importer answers to.
    pub at_auto_names: &'static [&'static str],
}

impl LanguageSpec {
    fn new(language: &'static str, extensions: &'static [&'static str]) -> Self {
        Self {
            language,
            block_patterns: Vec::new(),
            string_list: vec!["\"", "'"],
            compound_statements: Vec::new(),
            minimum_block_size: 0,
            guide_kind: GuideKind::Default,
            end_of_block: EndOfBlock::Braces,
            find_blocks: FindBlocks::Default,
            postprocess: Postprocess::BlankLines,
            headline: HeadlineKind::Default,
            line_importer: None,
            allow_mixed_whitespace: false,
            extensions,
            at_auto_names: &[],
        }
    }

    fn patterns(mut self, patterns: &[(&'static str, &str)]) -> Self {
        self.block_patterns = patterns
            .iter()
            .map(|(kind, pat)| (*kind, compile(pat)))
            .collect();
        self
    }
}

/// True if `name` is one of Leo's directives.
pub fn is_global_directive(name: &str) -> bool {
    crate::atfile_write::GLOBAL_DIRECTIVES.contains(&name)
}

/// Compile a Python `re` pattern used with `.match()`, which anchors at the
/// start of the string but not at its end.
fn compile(pattern: &str) -> Regex {
    Regex::new(&format!("^(?:{pattern})")).unwrap_or_else(|e| panic!("{pattern}: {e}"))
}

/// Every language this crate can import.
///
/// Leo derives this from the modules present in `leo/plugins/importers`. Here
/// it is a table, so a language that is absent is absent deliberately.
pub static LANGUAGES: Lazy<Vec<LanguageSpec>> = Lazy::new(build_languages);

fn build_languages() -> Vec<LanguageSpec> {
    let mut out = Vec::new();

    // --- Python and its relatives ---------------------------------------
    let python_patterns: &[(&str, &str)] = &[
        ("class", r"\s*class\s+(\w+)"),
        ("async def", r"\s*async\s+def\s+(\w+)\s*\("),
        ("def", r"\s*def\s+(\w+)\s*\("),
    ];
    let mut python =
        LanguageSpec::new("python", &[".py", ".pyw", ".pyi", ".codon"]).patterns(python_patterns);
    python.string_list = vec!["\"\"\"", "'''", "\"", "'"];
    python.guide_kind = GuideKind::Python;
    python.end_of_block = EndOfBlock::Indent;
    python.find_blocks = FindBlocks::Python;
    python.postprocess = Postprocess::Python;
    out.push(python);

    let mut cython = LanguageSpec::new("cython", &[".pyx"]).patterns(&[
        (
            "async class",
            r"\s*async\s+class\s+([\w_]+)\s*(\(.*?\))?(.*?):",
        ),
        ("class", r"\s*class\s+([\w_]+)\s*(\(.*?\))?(.*?):"),
        ("cdef", r"\s*cdef\s+([\w_ ]+)"),
        ("cpdef", r"\s*cpdef\s+([\w_ ]+)"),
        ("def", r"\s*def\s+([\w_ ]+)"),
    ]);
    cython.string_list = vec!["\"\"\"", "'''", "\"", "'"];
    cython.guide_kind = GuideKind::Python;
    cython.end_of_block = EndOfBlock::Indent;
    cython.find_blocks = FindBlocks::Python;
    cython.postprocess = Postprocess::Python;
    out.push(cython);

    let mut coffee = LanguageSpec::new("coffeescript", &[".coffee"]).patterns(&[
        ("class", r"^\s*class\s+([\w]+)"),
        ("def", r"^\s*(.+?):.*?->"),
        ("def", r"^\s*(.+?)=.*?->"),
    ]);
    coffee.string_list = vec!["\"\"\"", "'''", "\"", "'"];
    coffee.guide_kind = GuideKind::Python;
    coffee.end_of_block = EndOfBlock::Indent;
    coffee.find_blocks = FindBlocks::Python;
    coffee.postprocess = Postprocess::Python;
    out.push(coffee);

    // --- Brace languages -------------------------------------------------
    let c_patterns: &[(&str, &str)] = &[
        ("class", r".*?\bclass\s+(\w+)\s*\{"),
        ("func", r".*?\b(\w+)\s*\(.*?\)\s*(const)?\s*\{"),
        ("namespace", r".*?\bnamespace\s+(\w+)?\s*\{"),
        ("struct", r".*?\bstruct\s+(\w+)?\s*(:.*?)?\{"),
    ];
    let c_compound = vec![
        "case", "catch", "class", "do", "else", "for", "if", "switch", "try", "while",
    ];
    for (language, extensions) in [
        (
            "c",
            &[".c", ".cc", ".c++", ".cpp", ".cxx", ".h", ".hpp", ".hxx"][..],
        ),
        ("csharp", &[".cs", ".c#"][..]),
    ] {
        let mut spec = LanguageSpec::new(language, extensions).patterns(c_patterns);
        spec.string_list = vec!["\""]; // Not single quotes: 'x' is a character.
        spec.compound_statements = c_compound.clone();
        spec.find_blocks = FindBlocks::C;
        out.push(spec);
    }

    let mut java = LanguageSpec::new("java", &[".java"]).patterns(&[
        (
            "interface",
            r"^\s*interface\s+(\w+.*?)\s*((implements|throws).*?)?\{",
        ),
        ("", r"^\s*(.*?\bclass\s+\w+)"),
        ("", r"^\s*(\w+.*?)\(.*?\)\s*((implements|throws).*?)?\{"),
    ]);
    java.compound_statements = vec!["else", "for", "if", "switch", "while"];
    java.postprocess = Postprocess::Preamble;
    out.push(java);

    let js_patterns: &[(&str, &str)] = &[
        ("function", r"\s*?\(?function\b\s*([\w\.]*)\s*\(.*?\{"),
        ("function", r"\s*([\w.]+)\s*\:\s*\(*\s*function\s*\(.*?\{"),
        (
            "function",
            r"\s*\bvar\s+([\w\.]+)\s*=\s*\(*\s*function\s*\(.*?\{",
        ),
        ("function", r"\s*([\w\.]+)\s*=\s*\(*\s*function\s*\(.*?\{"),
    ];
    let mut js =
        LanguageSpec::new("javascript", &[".js", ".jsx", ".mjs", ".cjs"]).patterns(js_patterns);
    js.guide_kind = GuideKind::JavaScript;
    out.push(js);

    // TypeScript's table is ordered: the first match wins, so `interface` and
    // `class` must precede the bare `name (...) {` patterns.
    let kinds = r"(async|public|private|static)";
    let ts_patterns: Vec<(&'static str, String)> = vec![
        ("", r"(interface\s+\w+)".to_string()),
        ("", r"(class\s+\w+)".to_string()),
        ("", r"export\s+(class\s+\w+)".to_string()),
        ("", r"export\s+enum\s+(\w+)".to_string()),
        ("", r"export\s+const\s+enum\s+(\w+)".to_string()),
        ("", r"export\s+function\s+(\w+)".to_string()),
        ("", r"export\s+interface\s+(\w+)".to_string()),
        ("", r"function\s+(\w+)".to_string()),
        ("", r"(constructor).*\{".to_string()),
        ("", format!(r"{kinds}\s*function\s+(\w+)")),
        ("", format!(r"{kinds}\s+{kinds}\s+function\s+(\w+)")),
        ("", format!(r"{kinds}\s+{kinds}\s+(\w+)\s*\(.*\).*\{{")),
        ("", format!(r"{kinds}\s+(\w+)\s*\(.*\).*\{{")),
    ];
    let mut ts = LanguageSpec::new("typescript", &[".ts", ".tsx"]);
    // TypeScript names its block by a group whose number varies with the
    // pattern; the last group is always the name.
    ts.block_patterns = ts_patterns
        .iter()
        .map(|(kind, pat)| (*kind, compile(pat)))
        .collect();
    ts.guide_kind = GuideKind::JavaScript;
    out.push(ts);

    out.push(
        LanguageSpec::new("dart", &[".dart"])
            .patterns(&[("function", r"^\s*([\w\s]+)\s*\(.*?\)\s*\{")]),
    );

    out.push(LanguageSpec::new("php", &[".php"]));

    // Pug has no block patterns at all: the whole file is one node.
    out.push(LanguageSpec::new("pug", &[".pug", ".jade"]));

    // --- Rust -------------------------------------------------------------
    let mut rust = LanguageSpec::new("rust", &[".rs"]).patterns(&[
        ("enum", r"\s*enum\s+(\w+)\s*\{"),
        ("enum", r"\s*pub\s+enum\s+(\w+)\s*\{"),
        ("macro", r"\s*(\w+)\!\s*\{"),
        ("use", r"\s*use.*?\{"),
        ("fn", r"\s*fn\s+(\w+)"),
        ("fn", r"\s*pub\s+fn\s+(\w+)"),
        ("fn", r"\s*pub\s*\(\s*crate\s*\)\s*fn\s+(\w+)"),
        ("fn", r"\s*pub\s*\(\s*self\s*\)\s*fn\s+(\w+)"),
        ("fn", r"\s*pub\s*\(\s*super\s*\)\s*fn\s+(\w+)"),
        ("fn", r"\s*pub\s*\(\s*in\s*crate::.*?\)\s*fn\s+(\w+)"),
        ("fn", r"\s*pub\s*\(\s*in\s*self::.*?\)\s*fn\s+(\w+)"),
        ("fn", r"\s*pub\s*\(\s*in\s*super::.*?\)\s*fn\s+(\w+)"),
        ("impl", r"\s*impl\b(.*?)$"),
        ("mod", r"\s*mod\s+(\w+)"),
        ("struct", r"\s*struct\b(.*?)$"),
        ("struct", r"\s*pub\s+struct\b(.*?)$"),
        ("trait", r"\s*trait\b(.*?)$"),
        ("trait", r"\s*pub\s+trait\b(.*?)$"),
    ]);
    rust.string_list = Vec::new(); // Rust's own scanner does the work.
    rust.guide_kind = GuideKind::Rust;
    rust.find_blocks = FindBlocks::Rust;
    rust.postprocess = Postprocess::Rust;
    rust.headline = HeadlineKind::Rust;
    rust.allow_mixed_whitespace = true; // Ruff mixes blanks and tabs.
    out.push(rust);

    // --- Languages with their own end-of-block rule ----------------------
    let mut lua = LanguageSpec::new("lua", &[".lua"]).patterns(&[
        ("function", r"\s*function\s+([\w\.]+)\s*\("),
        ("function", r".*?([\w\.]+)\s*\(function\b\s*\("),
    ]);
    lua.end_of_block = EndOfBlock::LuaEnd;
    out.push(lua);

    let mut pascal = LanguageSpec::new("pascal", &[".pas"]).patterns(&[
        ("constructor", r"^\s*\bconstructor\s+([\w_\.]+)"),
        ("destructor", r"^\s*\bdestructor\s+([\w_\.]+)"),
        ("function", r"^\s*\bfunction\s+([\w_\.]+)"),
        ("procedure", r"^\s*\bprocedure\s+([\w_\.]+)"),
        ("unit", r"^\s*\bunit\s+([\w_\.]+)"),
    ]);
    pascal.end_of_block = EndOfBlock::NextBlock;
    out.push(pascal);

    let mut ini = LanguageSpec::new("ini", &[".ini"]).patterns(&[("section", r"^\s*(\[.*\])")]);
    ini.end_of_block = EndOfBlock::NextBlock;
    out.push(ini);

    let mut elisp = LanguageSpec::new("lisp", &[".el", ".clj", ".cljs", ".cljc"])
        .patterns(&[("defun", r"\s*\(\s*\bdefun\s+([\w_-]+)")]);
    elisp.string_list = vec!["\""];
    elisp.end_of_block = EndOfBlock::Parens;
    out.push(elisp);

    let mut scheme = LanguageSpec::new("scheme", &[".scm"]).patterns(&[
        (
            "define-library",
            r"\s*\(\s*\bdefine-library\s*\(?\s*([\w_-]+)",
        ),
        (
            "define-module",
            r"\s*\(\s*\bdefine-module\s*\(?\s*([\w_-]+)",
        ),
        (
            "define-public",
            r"\s*\(\s*\bdefine-public\s*\(?\s*([\w_-]+)",
        ),
        ("define", r"\s*\(\s*\bdefine\s*\(?([\w_-]+)"),
    ]);
    scheme.string_list = vec!["\""];
    scheme.end_of_block = EndOfBlock::Parens;
    out.push(scheme);

    out.push(LanguageSpec::new("tcl", &[".tcl"]).patterns(&[("proc", r"\s*\bproc\s+(\w+)")]));

    let mut perl =
        LanguageSpec::new("perl", &[".pl", ".pm"]).patterns(&[("sub", r"\s*sub\s+(\w+)")]);
    perl.guide_kind = GuideKind::Perl;
    out.push(perl);

    // --- XML and HTML -----------------------------------------------------
    // Block patterns come from the caller's tag list, which is empty by
    // default -- exactly as in Leo, where they come from the
    // `@data import_xml_tags` setting. With no tags the file is one node.
    for (language, extensions) in [("xml", &[".xml"][..]), ("html", &[".html", ".htm"][..])] {
        let mut spec = LanguageSpec::new(language, extensions);
        spec.end_of_block = EndOfBlock::Tags;
        spec.headline = HeadlineKind::Tag;
        spec.minimum_block_size = 2; // Helps with one-line elements.
        out.push(spec);
    }

    // --- Line-oriented importers -----------------------------------------
    let mut org = LanguageSpec::new("org", &[".org"]);
    org.line_importer = Some(LineImporter::Org);
    org.at_auto_names = &["@auto-org", "@auto-org-mode"];
    out.push(org);

    let mut otl = LanguageSpec::new("otl", &[".otl"]);
    otl.line_importer = Some(LineImporter::Otl);
    otl.at_auto_names = &["@auto-otl", "@auto-vim-outline"];
    otl.allow_mixed_whitespace = true; // Tabs are part of the otl format.
    out.push(otl);

    let mut md = LanguageSpec::new("md", &[".md", ".markdown"]);
    md.line_importer = Some(LineImporter::Markdown);
    md.at_auto_names = &["@auto-md", "@auto-markdown"];
    out.push(md);

    let mut treepad = LanguageSpec::new("plain", &[".hjt"]);
    treepad.line_importer = Some(LineImporter::Treepad);
    out.push(treepad);

    out
}

/// The importer for an `@auto` node: by `@auto-<name>` first, then extension.
pub fn spec_for(headline: &str, path: &str) -> Option<&'static LanguageSpec> {
    let h = headline.trim();
    for spec in LANGUAGES.iter() {
        for name in spec.at_auto_names {
            if util::match_word(h, 0, name) {
                return Some(spec);
            }
        }
    }
    let (_, ext) = util::os_path_splitext(path);
    let ext = ext.to_lowercase();
    LANGUAGES
        .iter()
        .find(|spec| spec.extensions.contains(&ext.as_str()))
}

/// What an import did, for a caller that wants to check or report it.
#[derive(Debug, Default)]
pub struct ImportReport {
    pub language: String,
    pub nodes: usize,
    /// Set when leading tabs were converted to blanks, or the reverse. The
    /// file is then rewritten the next time this @auto node is written.
    pub regularized_whitespace: bool,
    /// The exact text the imported tree must write back. Compare it against
    /// [`write_string`] to know whether the import lost anything.
    pub text: String,
    /// False for the four line-oriented formats, whose writers do not promise
    /// to reproduce their input: the markdown writer always writes `#`
    /// headings, whatever the file used.
    pub round_trips: bool,
}

/// Import `contents` into the `@auto` node at `parent`.
///
/// The tree is checked against the text it came from before it is kept: an
/// importer that loses or reorders a line would make the next write of this
/// `@auto` node overwrite the user's file with something else. On failure the
/// whole file goes into `parent.b`, which is what Leo does when an importer
/// raises.
pub fn import_string(
    o: &mut Outline,
    parent: &Position,
    contents: &str,
    path: &str,
) -> Result<ImportReport> {
    let spec = spec_for(parent.h(o), path).ok_or_else(|| Error::Import {
        path: util::short_file_name(path),
        detail: "no @auto importer for it".to_string(),
    })?;
    let mut report = block::import(o, parent, contents, spec);
    report.round_trips = spec.line_importer.is_none();
    Ok(report)
}

/// The text an `@auto` node writes back to its file.
///
/// Leo dispatches to one of six writers here and otherwise falls back to a
/// sentinel-free tangle. The four line-oriented importers need their writer,
/// because they consume the section lines they read; everything else is the
/// fallback.
pub fn write_string(o: &Outline, parent: &Position, path: &str) -> Result<String> {
    match spec_for(parent.h(o), path).and_then(|spec| spec.line_importer) {
        Some(kind) => Ok(lines::write(o, parent, kind)),
        // Leo 5.6: allow undefined section references in all @auto files.
        None => crate::atfile_write::write_to_string(o, parent, false, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_importer_is_chosen_by_extension() {
        assert_eq!(spec_for("@auto x.py", "x.py").unwrap().language, "python");
        assert_eq!(spec_for("@auto x.rs", "x.rs").unwrap().language, "rust");
        assert_eq!(
            spec_for("@auto x.JS", "x.JS").unwrap().language,
            "javascript"
        );
        assert!(spec_for("@auto x.unknown", "x.unknown").is_none());
    }

    #[test]
    fn an_at_auto_name_wins_over_the_extension() {
        // @auto-md on a .txt file is still markdown.
        assert_eq!(
            spec_for("@auto-md notes.txt", "notes.txt")
                .unwrap()
                .language,
            "md"
        );
    }
}
