//! Script text: jupytext's `languages.py`, `stringparser.py`, `magics.py`
//! and `pep8.py`, for the `.py` extension the `py:percent` format uses.

use std::collections::HashMap;

use once_cell::sync::Lazy;
use regex::Regex;

/// `_SCRIPT_EXTENSIONS`' languages and their line comments, in its order.
const SCRIPT_LANGUAGES: &[(&str, &str)] = &[
    ("python", "#"),
    ("coconut", "#"),
    ("R", "#"),
    ("julia", "#"),
    ("c++", "//"),
    ("scheme", ";;"),
    ("clojure", ";;"),
    ("bash", "#"),
    ("powershell", "#"),
    ("q", "/"),
    ("matlab", "%"),
    ("wolfram language", "(*"),
    ("idl", ";"),
    ("javascript", "//"),
    ("typescript", "//"),
    ("scala", "//"),
    ("rust", "//"),
    ("robotframework", "#"),
    ("csharp", "//"),
    ("fsharp", "//"),
    ("sos", "#"),
    ("java", "//"),
    ("groovy", "//"),
    ("sage", "#"),
    ("ocaml", "(*"),
    ("haskell", "--"),
    ("tcl", "#"),
    ("maxima", "/*"),
    ("gnuplot", "#"),
    ("stata", "//"),
    ("sas", "/*"),
    ("jenner", "/*"),
    ("xonsh", "#"),
    ("logtalk", "%"),
    ("lua", "--"),
    ("go", "//"),
];

/// `_COMMENT`: a language's line comment.
pub fn comment_of(language: &str) -> Option<&'static str> {
    SCRIPT_LANGUAGES
        .iter()
        .find(|(l, _)| *l == language)
        .map(|(_, c)| *c)
}

/// `_JUPYTER_LANGUAGES`: names a `%%` cell magic may give.
pub fn is_jupyter_language(language: &str) -> bool {
    const MAGICS: &[&str] = &[
        "R",
        "bash",
        "sh",
        "python",
        "python2",
        "python3",
        "coconut",
        "javascript",
        "js",
        "perl",
        "html",
        "latex",
        "markdown",
        "pypy",
        "ruby",
        "script",
        "svg",
        "matlab",
        "octave",
        "idl",
        "robotframework",
        "sas",
        "spark",
        "sql",
        "cython",
        "haskell",
        "tcl",
        "gnuplot",
        "wolfram language",
        "c#",
        "f#",
        "cs",
        "fs",
    ];
    MAGICS.contains(&language) || comment_of(language).is_some()
}

/// `usual_language_name`.
pub fn usual_language_name(language: &str) -> String {
    let l = language.to_lowercase();
    match l.as_str() {
        "r" => "R".to_string(),
        _ if l.starts_with("c++") => "c++".to_string(),
        "octave" => "matlab".to_string(),
        "cs" | "c#" => "csharp".to_string(),
        "fs" | "f#" => "fsharp".to_string(),
        "sas" => "SAS".to_string(),
        _ => l,
    }
}

/// `same_language`.
pub fn same_language(a: &str, b: &str) -> bool {
    usual_language_name(a) == usual_language_name(b)
}

/// `comment_lines`: prefix each line with the comment and a space.
pub fn comment_lines(lines: &[String], prefix: &str) -> Vec<String> {
    lines
        .iter()
        .map(|l| match l.is_empty() {
            true => prefix.to_string(),
            false => format!("{prefix} {l}"),
        })
        .collect()
}

/// `uncomment`: remove the prefix and a space, or the prefix alone.
pub fn uncomment(lines: &[String], prefix: &str) -> Vec<String> {
    if prefix.is_empty() {
        return lines.to_vec();
    }
    let with_space = format!("{prefix} ");
    lines
        .iter()
        .map(|l| {
            l.strip_prefix(with_space.as_str())
                .or_else(|| l.strip_prefix(prefix))
                .unwrap_or(l)
                .to_string()
        })
        .collect()
}

/// `StringParser`: whether the next line starts inside a string.
pub struct StringParser {
    ignore: bool,
    python: bool,
    comment: Option<&'static str>,
    single: Option<char>,
    triple: Option<char>,
    triple_start: isize,
}

impl StringParser {
    /// A parser for `language`; None quotes nothing.
    pub fn new(language: Option<&str>) -> StringParser {
        StringParser {
            ignore: language.is_none(),
            python: language != Some("R"),
            comment: language.and_then(comment_of),
            single: None,
            triple: None,
            triple_start: -1,
        }
    }

    /// `is_quoted`.
    pub fn is_quoted(&self) -> bool {
        !self.ignore && (self.single.is_some() || self.triple.is_some())
    }

    /// `read_line`.
    pub fn read_line(&mut self, line: &str) {
        if self.ignore {
            return;
        }
        if let Some(c) = self.comment {
            if !self.is_quoted() && line.trim_start().starts_with(c) {
                return;
            }
        }
        self.triple_start = -1;
        let chars: Vec<char> = line.chars().collect();
        for i in 0..chars.len() {
            let ch = chars[i];
            if self.single.is_none() && self.triple.is_none() {
                if let Some(c) = self.comment {
                    let rest: String = chars[i..].iter().take(c.chars().count()).collect();
                    if c.starts_with(ch) && rest == c {
                        break;
                    }
                }
            }
            if ch != '"' && ch != '\'' {
                continue;
            }
            if i > 0 && chars[i - 1] == '\\' {
                continue;
            }
            if self.single == Some(ch) {
                self.single = None;
                continue;
            }
            if self.single.is_some() || !self.python {
                continue;
            }
            if i >= 2
                && chars[i - 2] == ch
                && chars[i - 1] == ch
                && i as isize >= self.triple_start + 3
            {
                if self.triple == Some(ch) {
                    self.triple = None;
                    self.triple_start = i as isize;
                    continue;
                }
                if self.triple.is_some() {
                    continue;
                }
                self.triple = Some(ch);
                self.triple_start = i as isize;
                continue;
            }
            if self.triple.is_some() {
                continue;
            }
            self.single = Some(ch);
        }
        if self.python {
            self.single = None;
        }
    }
}

struct MagicRes {
    magic: Regex,
    force_escape: Regex,
    no_escape: Regex,
}

static MAGIC: Lazy<HashMap<&'static str, MagicRes>> = Lazy::new(|| {
    let mut m = HashMap::new();
    for (language, comment) in SCRIPT_LANGUAGES {
        let c = regex::escape(comment);
        let re = |s: String| Regex::new(&s).unwrap();
        m.insert(
            *language,
            MagicRes {
                magic: re(format!(r"^\s*({c} |{c})*(%|%%|%%%)[a-zA-Z]")),
                force_escape: re(format!(
                    r"^\s*({c} |{c})*(%|%%|%%%)[a-zA-Z](.*){c}\s*escape"
                )),
                no_escape: re(format!(
                    r"^\s*({c} |{c})*(%|%%|%%%)[a-zA-Z](.*){c}\s*noescape"
                )),
            },
        );
    }
    // Rust magics start with ':'. jupytext assigns the force pattern twice,
    // so the second, `noescape`, is the one in effect.
    let rust = m.get_mut("rust").unwrap();
    rust.magic = Regex::new(r"^(// |//)*:[a-zA-Z]").unwrap();
    rust.force_escape = Regex::new(r"^(// |//)*:[a-zA-Z](.*)//\s*noescape").unwrap();
    m
});

static HELP_OR_BASH: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*(?:(?:# |#)+\s*)?(\?|!)\s*[A-Za-z.~$\\/{}]").unwrap());
static MAGIC_CMD: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"^(# |#)*(cat|cd|cp|mv|rm|rmdir|mkdir|copy|ddir|echo|ls|ldir|mkdir|ren|rmdir)($|\s$|\s[^=,])",
    )
    .unwrap()
});
static MAGIC_HELP: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*(# )*[^\s]*\?\s*$").unwrap());
static MAGIC_ASSIGN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(# |#)*\s*([a-zA-Z_][a-zA-Z_$0-9]*)\s*=\s*(%|%%|%%%|!)[a-zA-Z](.*)").unwrap()
});
static CONTINUATION: Lazy<Regex> = Lazy::new(|| Regex::new(r"^.*\\\s*$").unwrap());
static ESCAPED_CODE_START: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(# |#)*(#|# )\+").unwrap());

/// `is_magic`, with the global escape flag on.
pub fn is_magic(line: &str, language: &str, explicitly_code: bool) -> bool {
    let language = usual_language_name(language);
    let excluded = ["octave", "matlab", "sas", "jenner", "logtalk"];
    let Some(res) = MAGIC.get(language.as_str()) else {
        return false;
    };
    if excluded.contains(&language.as_str()) {
        return false;
    }
    if res.force_escape.is_match(line) {
        return true;
    }
    if res.no_escape.is_match(line) {
        return false;
    }
    if res.magic.is_match(line) {
        return true;
    }
    if language != "python" {
        return false;
    }
    HELP_OR_BASH.is_match(line)
        || MAGIC_ASSIGN.is_match(line)
        || (explicitly_code && MAGIC_HELP.is_match(line))
        || MAGIC_CMD.is_match(line)
}

fn split_indent(line: &str) -> (&str, &str) {
    let rest = line.trim_start();
    (&line[..line.len() - rest.len()], rest)
}

/// `comment_magic`: escape Jupyter magics with the language's comment.
pub fn comment_magic(source: &mut [String], language: &str, explicitly_code: bool) {
    let comment = comment_of(language).unwrap_or("#");
    let mut parser = StringParser::new(Some(language));
    let mut next_is_magic = false;
    for line in source.iter_mut() {
        let original = line.clone();
        if !parser.is_quoted() && (next_is_magic || is_magic(&original, language, explicitly_code))
        {
            let (indent, rest) = match next_is_magic {
                true => ("", original.as_str()),
                false => split_indent(&original),
            };
            *line = format!("{indent}{comment} {rest}");
            next_is_magic = language == "python" && CONTINUATION.is_match(&original);
        }
        parser.read_line(&original);
    }
}

/// `unesc`: remove one comment from a line.
fn unesc(line: &str, language: &str) -> String {
    let comment = comment_of(language).unwrap_or("#");
    let (indent, rest) = split_indent(line);
    if let Some(r) = rest.strip_prefix(&format!("{comment} ")) {
        return format!("{indent}{r}");
    }
    if let Some(r) = rest.strip_prefix(comment) {
        return format!("{indent}{r}");
    }
    line.to_string()
}

/// `uncomment_magic`: the inverse of [`comment_magic`].
pub fn uncomment_magic(source: &mut [String], language: &str, explicitly_code: bool) {
    let mut parser = StringParser::new(Some(language));
    let mut next_is_magic = false;
    for line in source.iter_mut() {
        let original = line.clone();
        if !parser.is_quoted() && (next_is_magic || is_magic(&original, language, explicitly_code))
        {
            *line = unesc(&original, language);
            next_is_magic = language == "python" && CONTINUATION.is_match(&original);
        }
        parser.read_line(&original);
    }
}

/// `unescape_code_start` for `.py`.
pub fn unescape_code_start(source: &mut [String], language: &str) {
    let mut parser = StringParser::new(Some(language));
    for line in source.iter_mut() {
        let original = line.clone();
        if !parser.is_quoted() && ESCAPED_CODE_START.is_match(&original) {
            let unescaped = unesc(&original, language);
            if ESCAPED_CODE_START.is_match(&unescaped) {
                *line = unescaped;
            }
        }
        parser.read_line(&original);
    }
}

fn blank(s: &str) -> bool {
    s.trim().is_empty()
}

fn starts_def(line: &str) -> bool {
    line.starts_with("def ") || line.starts_with("async ") || line.starts_with("class ")
}

/// `next_instruction_is_function_or_class`.
fn next_instruction_is_function_or_class<'a>(lines: impl Iterator<Item = &'a str>) -> bool {
    let mut parser = StringParser::new(Some("python"));
    let mut previous_blank = false;
    for (i, line) in lines.enumerate() {
        if parser.is_quoted() {
            parser.read_line(line);
            previous_blank = blank(line);
            continue;
        }
        parser.read_line(line);
        if blank(line) {
            if i > 0 && previous_blank {
                return false;
            }
            previous_blank = true;
            continue;
        }
        previous_blank = false;
        if starts_def(line) {
            return true;
        }
        if line.starts_with(['#', '@', ' ', ')']) {
            continue;
        }
        return false;
    }
    false
}

/// `cell_ends_with_function_or_class`.
fn cell_ends_with_function_or_class(lines: &[String]) -> bool {
    let mut non_quoted = Vec::new();
    let mut parser = StringParser::new(Some("python"));
    for line in lines {
        if !parser.is_quoted() {
            non_quoted.push(line);
        }
        parser.read_line(line);
    }
    non_quoted.reverse();
    for (i, line) in non_quoted.iter().enumerate() {
        if blank(line) {
            if i > 0 && blank(non_quoted[i - 1]) {
                return false;
            }
            continue;
        }
        if line.starts_with(['#', ' ', ')']) {
            continue;
        }
        return starts_def(line);
    }
    false
}

/// `cell_ends_with_code`.
fn cell_ends_with_code(lines: &[String]) -> bool {
    lines
        .last()
        .is_some_and(|l| !blank(l) && !l.starts_with('#'))
}

/// `cell_has_code`.
fn cell_has_code<'a>(lines: impl Iterator<Item = &'a str>) -> bool {
    let mut previous_blank = false;
    for (i, line) in lines.enumerate() {
        let s = line.trim();
        if s.starts_with('#') {
            previous_blank = false;
            continue;
        }
        if s.is_empty() {
            if i > 0 && previous_blank {
                return false;
            }
            previous_blank = true;
            continue;
        }
        return true;
    }
    false
}

/// `pep8_lines_between_cells` for `.py`. `next` is the text after `prev`,
/// as far as it goes; it is read only as far as the answer needs.
pub fn pep8_lines_between_cells<'a>(
    prev: &[String],
    next: impl Iterator<Item = &'a str> + Clone,
) -> usize {
    if next.clone().next().is_none() {
        return 1;
    }
    if prev.is_empty() {
        return 0;
    }
    if cell_ends_with_function_or_class(prev) {
        return if cell_has_code(next) { 2 } else { 1 };
    }
    if cell_ends_with_code(prev) && next_instruction_is_function_or_class(next) {
        return 2;
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn magics_are_commented_and_back() {
        let mut src = v(&[
            "%matplotlib inline",
            "x = 1",
            "!ls",
            "a = !pwd",
            "s = '''",
            "%time",
            "'''",
        ]);
        comment_magic(&mut src, "python", true);
        assert_eq!(
            src,
            v(&[
                "# %matplotlib inline",
                "x = 1",
                "# !ls",
                "# a = !pwd",
                "s = '''",
                "%time",
                "'''"
            ])
        );
        uncomment_magic(&mut src, "python", true);
        assert_eq!(src[0], "%matplotlib inline");
        assert_eq!(src[3], "a = !pwd");
    }

    #[test]
    fn two_blank_lines_follow_a_function() {
        let p = |a: &[&str], b: &[&'static str]| pep8_lines_between_cells(&v(a), b.iter().copied());
        assert_eq!(p(&["def f():", "    pass"], &["f()"]), 2);
        assert_eq!(p(&["x = 1"], &["def g():"]), 2);
        assert_eq!(p(&["x = 1"], &["y = 2"]), 1);
        assert_eq!(p(&["x = 1"], &[]), 1);
    }
}
