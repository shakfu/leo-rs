//! String, path and scanning helpers.
//!
//! Ports of the functions in `leo/leolib/util.py` that the model needs.
//! Indices are byte offsets. Every function here only ever splits at ASCII
//! characters, so byte offsets and character offsets agree wherever it
//! matters and non-ASCII text passes through untouched.

use std::path::{Component, Path, PathBuf};

/// Split s into lines, keeping the line endings, as Python's `str.splitlines(True)`.
pub fn split_lines(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] == b'\n' {
            out.push(s[start..=i].to_string());
            start = i + 1;
        }
        i += 1;
    }
    if start < s.len() {
        out.push(s[start..].to_string());
    }
    out
}

/// Split at '\n' only, keeping the newline.
///
/// Unlike [`split_lines`], which follows Python's `str.splitlines` and also
/// breaks at form feeds and other line separators. The importers need every
/// such character preserved inside a line, or the text they hand back would
/// differ from the file they read.
pub fn split_lines_at_newline(s: &str) -> Vec<String> {
    if s.is_empty() {
        return Vec::new();
    }
    let mut parts: Vec<&str> = s.split('\n').collect();
    if parts.last() == Some(&"") {
        parts.pop();
    }
    let mut lines: Vec<String> = parts.iter().map(|z| format!("{z}\n")).collect();
    if !s.ends_with('\n') {
        if let Some(last) = lines.last_mut() {
            last.pop();
        }
    }
    lines
}

/// Split at '\n', dropping the line endings, as Python's `splitlines(False)`.
///
/// A trailing newline does not produce a final empty line.
pub fn split_lines_no_ends(s: &str) -> Vec<&str> {
    if s.is_empty() {
        return Vec::new();
    }
    let mut parts: Vec<&str> = s.split('\n').collect();
    if parts.last() == Some(&"") {
        parts.pop();
    }
    parts
}

/// The leading blanks and tabs of s.
pub fn get_leading_ws(s: &str) -> &str {
    let i = s
        .as_bytes()
        .iter()
        .position(|c| *c != b' ' && *c != b'\t')
        .unwrap_or(s.len());
    &s[..i]
}

/// Rewrite s's leading whitespace to `tab_width`'s preferred character.
pub fn optimize_leading_whitespace(line: &str, tab_width: i32) -> String {
    let (i, width) = skip_leading_ws_with_indent(line, 0, tab_width);
    format!(
        "{}{}",
        compute_leading_whitespace(width, tab_width),
        &line[i..]
    )
}

pub fn is_ws(ch: u8) -> bool {
    ch == b'\t' || ch == b' '
}

pub fn is_nl(s: &str, i: usize) -> bool {
    let b = s.as_bytes();
    i < b.len() && (b[i] == b'\n' || b[i] == b'\r')
}

/// True if `pattern` occurs at s[i..]. No word-boundary check.
///
/// Compares bytes, not a substring: `i` comes from scanning code that counts
/// ASCII delimiters, and slicing `&str` there would panic the moment a line
/// holds a multi-byte character.
pub fn matches_at(s: &str, i: usize, pattern: &str) -> bool {
    !pattern.is_empty() && s.as_bytes()[i.min(s.len())..].starts_with(pattern.as_bytes())
}

fn is_word_char(ch: u8) -> bool {
    ch.is_ascii_alphanumeric() || ch == b'_'
}

fn is_word_char1(ch: u8) -> bool {
    ch.is_ascii_alphabetic() || ch == b'_'
}

/// True if s[i..] starts the whole word `pattern`, as `g.match_word`.
pub fn match_word(s: &str, i: usize, pattern: &str) -> bool {
    if pattern.is_empty() || !matches_at(s, i, pattern) {
        return false;
    }
    let pb = pattern.as_bytes();
    let sb = s.as_bytes();
    if is_word_char1(pb[0]) && i > 0 && is_word_char(sb[i - 1]) {
        return false;
    }
    if is_word_char(pb[pb.len() - 1]) {
        let j = i + pattern.len();
        if j < sb.len() && is_word_char(sb[j]) {
            return false;
        }
    }
    true
}

/// Index just past the next newline, or len(s).
pub fn skip_line(s: &str, i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    match s[i..].find('\n') {
        Some(j) => i + j + 1,
        None => s.len(),
    }
}

/// Index of the next newline, or len(s).
pub fn skip_to_end_of_line(s: &str, i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    match s[i..].find('\n') {
        Some(j) => i + j,
        None => s.len(),
    }
}

pub fn skip_ws(s: &str, mut i: usize) -> usize {
    let b = s.as_bytes();
    while i < b.len() && is_ws(b[i]) {
        i += 1;
    }
    i
}

pub fn skip_nl(s: &str, i: usize) -> usize {
    if matches_at(s, i, "\r\n") {
        return i + 2;
    }
    if matches_at(s, i, "\n") || matches_at(s, i, "\r") {
        return i + 1;
    }
    i
}

/// `(i, indent)`: i is past the leading whitespace, indent its width in columns.
pub fn skip_leading_ws_with_indent(s: &str, mut i: usize, tab_width: i32) -> (usize, i32) {
    let mut count = 0i32;
    let b = s.as_bytes();
    let w = tab_width.abs().max(1);
    while i < b.len() {
        match b[i] {
            b' ' => {
                count += 1;
                i += 1;
            }
            b'\t' => {
                count += w - (count % w);
                i += 1;
            }
            _ => break,
        }
    }
    (i, count)
}

/// Whitespace `width` columns wide, using tabs when tab_width > 1.
pub fn compute_leading_whitespace(width: i32, tab_width: i32) -> String {
    if width <= 0 {
        return String::new();
    }
    if tab_width > 1 {
        let tabs = width / tab_width;
        let blanks = width % tab_width;
        "\t".repeat(tabs as usize) + &" ".repeat(blanks as usize)
    } else {
        " ".repeat(width as usize)
    }
}

/// Index of `pattern` within the line starting at i, or -1 as `Option::None`.
pub fn find_on_line(s: &str, i: usize, pattern: &str) -> Option<usize> {
    let j = match s[i..].find('\n') {
        Some(k) => i + k,
        None => s.len(),
    };
    s[i..j].find(pattern).map(|k| i + k)
}

pub fn angle_brackets(s: &str) -> String {
    format!("<<{s}>>")
}

pub fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let t: String = s.chars().take(n.saturating_sub(3)).collect();
    format!("{t}...")
}

/// Split the delimiters following an `@comment` directive.
///
/// Returns (single, block-start, block-end). The "REM hack" (underscore for a
/// blank, double underscore for a newline) and `@0x` hex encoding are honoured
/// because real outlines use them.
pub fn set_delims_from_string(s: &str) -> (String, String, String) {
    let fail = (String::new(), String::new(), String::new());
    let tag = "@comment";
    let mut i = 0usize;
    if match_word(s, i, tag) {
        i += tag.len();
    }
    let mut delims = [String::new(), String::new(), String::new()];
    let mut count = 0usize;
    let b = s.as_bytes();
    while count < 3 && i < s.len() {
        i = skip_ws(s, i);
        let j = i;
        while i < b.len() && !is_ws(b[i]) && !is_nl(s, i) {
            i += 1;
        }
        if j == i {
            break;
        }
        delims[count] = s[j..i].to_string();
        count += 1;
    }
    if count == 2 {
        // delims[0] is always the single-line delim.
        delims[2] = delims[1].clone();
        delims[1] = delims[0].clone();
        delims[0] = String::new();
    }
    for d in delims.iter_mut() {
        if d.is_empty() {
            continue;
        }
        if let Some(hex) = d.strip_prefix("@0x") {
            if hex.is_empty() {
                return fail;
            }
            match unhexlify(hex) {
                Some(bytes) => match String::from_utf8(bytes) {
                    Ok(text) => *d = text,
                    Err(_) => return fail,
                },
                None => return fail,
            }
        } else {
            *d = d.replace("__", "\n").replace('_', " ");
        }
    }
    let [a, b_, c] = delims;
    (a, b_, c)
}

pub fn unhexlify(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(s.len() / 2);
    let mut i = 0;
    while i < b.len() {
        let hi = (b[i] as char).to_digit(16)?;
        let lo = (b[i + 1] as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
        i += 2;
    }
    Some(out)
}

pub fn hexlify(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Convert the name of a line ending to the line ending itself.
pub fn get_output_newline(name: &str) -> String {
    match name.to_lowercase().as_str() {
        "nl" | "lf" => "\n".to_string(),
        "cr" => "\r".to_string(),
        "crlf" => "\r\n".to_string(),
        "platform" => {
            if cfg!(windows) {
                "\r\n".to_string()
            } else {
                "\n".to_string()
            }
        }
        _ => "\n".to_string(),
    }
}

/// Strip quotes and angle brackets from a path written in a directive.
pub fn strip_path_cruft(path: &str) -> String {
    let p = path.trim();
    if p.len() > 2 {
        let first = p.chars().next().unwrap();
        let last = p.chars().last().unwrap();
        if (first == '<' && last == '>')
            || (first == '"' && last == '"')
            || (first == '\'' && last == '\'')
        {
            return p[1..p.len() - 1].trim().to_string();
        }
    }
    p.to_string()
}

fn expand_user(path: &str) -> String {
    if path == "~" {
        return home_dir();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return format!("{}/{}", home_dir(), rest);
    }
    path.to_string()
}

pub fn home_dir() -> String {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| String::from("/"))
}

/// Normalize without resolving symlinks, as `os.path.normpath` does.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Absolute, expanded, normalized path with forward slashes. Never resolves symlinks.
pub fn finalize(path: &str) -> String {
    if path.is_empty() {
        return String::new();
    }
    let expanded = expand_user(path);
    let p = Path::new(&expanded);
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(p)
    };
    normalize(&abs).to_string_lossy().replace('\\', "/")
}

/// Join the non-empty args, then finalize. Later absolute args win, as `os.path.join`.
pub fn finalize_join(args: &[&str]) -> String {
    let parts: Vec<String> = args
        .iter()
        .filter(|a| !a.is_empty())
        .map(|a| expand_user(a))
        .collect();
    if parts.is_empty() {
        return String::new();
    }
    let mut acc = PathBuf::from(&parts[0]);
    for p in &parts[1..] {
        acc.push(p);
    }
    finalize(&acc.to_string_lossy())
}

pub fn os_path_dirname(path: &str) -> String {
    Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

pub fn os_path_basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// (root, ext) as `os.path.splitext`: ext keeps its leading period.
pub fn os_path_splitext(path: &str) -> (String, String) {
    let base = os_path_basename(path);
    match base.rfind('.') {
        Some(i) if i > 0 => (
            path[..path.len() - (base.len() - i)].to_string(),
            base[i..].to_string(),
        ),
        _ => (path.to_string(), String::new()),
    }
}

pub fn short_file_name(path: &str) -> String {
    os_path_basename(path)
}

/// Escape text for an XML element's content.
///
/// `&`, `<` and `>` only -- a quote is not special in element content, and Leo
/// does not escape it, so escaping it here would change every file it writes.
/// Control characters other than tab, newline and return are dropped: they are
/// not valid XML at all, and Leo's `entities` table deletes them.
pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\t' | '\n' | '\r' => out.push(ch),
            c if (c as u32) < 32 => {}
            c => out.push(c),
        }
    }
    out
}

/// Escape a value for an XML attribute, as `xml.sax.saxutils.quoteattr`.
pub fn xml_quoteattr(s: &str) -> String {
    let mut body = xml_escape(s);
    body = body
        .replace('\n', "&#10;")
        .replace('\r', "&#13;")
        .replace('\t', "&#9;");
    if body.contains('"') {
        if body.contains('\'') {
            format!("\"{}\"", body.replace('"', "&quot;"))
        } else {
            format!("'{body}'")
        }
    } else {
        format!("\"{body}\"")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_lines_keeps_endings() {
        assert_eq!(split_lines("a\nb\n"), vec!["a\n", "b\n"]);
        assert_eq!(split_lines("a\nb"), vec!["a\n", "b"]);
        assert_eq!(split_lines(""), Vec::<String>::new());
    }

    #[test]
    fn match_word_needs_boundaries() {
        assert!(match_word("@others\n", 0, "@others"));
        assert!(!match_word("@othersx", 0, "@others"));
        assert!(match_word("  @others", 2, "@others"));
    }

    #[test]
    fn delims_from_string_handles_rem_hack() {
        assert_eq!(
            set_delims_from_string("#"),
            ("#".to_string(), String::new(), String::new())
        );
        assert_eq!(
            set_delims_from_string("/* */"),
            (String::new(), "/*".to_string(), "*/".to_string())
        );
        assert_eq!(set_delims_from_string("REM_").0, "REM ".to_string());
    }

    #[test]
    fn leading_ws_counts_tabs_as_columns() {
        assert_eq!(skip_leading_ws_with_indent("\t x", 0, 4), (2, 5));
    }
}
