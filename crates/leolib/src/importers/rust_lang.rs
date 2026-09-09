//! The Rust importer.
//!
//! Rust needs its own guide-line scanner. The default one would take a
//! lifetime (`'a`) for the start of a string and blank out the rest of the
//! file, and it knows nothing about raw strings or nested block comments.

use once_cell::sync::Lazy;
use regex::Regex;

use crate::importers::block::{Block, Importer};
use crate::util;

/// Guide lines for Rust: nested block comments, raw strings, lifetimes.
///
/// Works over the whole file rather than line by line, because a raw string
/// or a block comment may span any number of lines. Every skipped character
/// becomes a blank and every newline is kept, so the guide lines line up with
/// the originals character for character.
pub fn delete_comments_and_strings(lines: &[String]) -> Vec<String> {
    let s: Vec<char> = lines.concat().chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(s.len());
    let mut i = 0usize;

    // `add` keeps a character; `skip` replaces it with a blank, except for a
    // newline, which must stay a newline or the line counts would diverge.
    macro_rules! add {
        () => {
            if i < s.len() {
                out.push(s[i]);
                i += 1;
            }
        };
    }
    macro_rules! skip {
        () => {
            if i < s.len() {
                out.push(if s[i] == '\n' { '\n' } else { ' ' });
                i += 1;
            }
        };
    }
    macro_rules! skip_n {
        ($n:expr) => {
            for _ in 0..$n {
                skip!();
            }
        };
    }

    while i < s.len() {
        match s[i] {
            '\n' => add!(),
            '\\' => {
                add!();
                add!();
            }
            '\'' => {
                // A character constant, or a lifetime, or neither.
                match match_single_quote(&s, i) {
                    Some(n) => skip_n!(n),
                    None => add!(),
                }
            }
            '"' => {
                skip!(); // The opening quote.
                while i < s.len() {
                    match s[i] {
                        '"' => {
                            skip!();
                            break;
                        }
                        '\\' => skip_n!(2),
                        _ => skip!(),
                    }
                }
            }
            '/' => {
                let next = s.get(i + 1).copied().unwrap_or('\0');
                if next == '/' {
                    skip_n!(2);
                    while i < s.len() {
                        let ch = s[i];
                        skip!();
                        if ch == '\n' {
                            break;
                        }
                    }
                } else if next == '*' {
                    // Block comments nest in Rust.
                    let mut level = 1;
                    skip_n!(2);
                    while i + 1 < s.len() {
                        if s[i] == '/' && s[i + 1] == '*' {
                            level += 1;
                            skip_n!(2);
                        } else if s[i] == '*' && s[i + 1] == '/' {
                            level -= 1;
                            skip_n!(2);
                            if level == 0 {
                                break;
                            }
                        } else {
                            skip!();
                        }
                    }
                } else {
                    add!();
                }
            }
            'r' => {
                // A raw string is 'r', 0..256 '#' characters, then '"'.
                let mut j = 0usize;
                while i + 1 + j < s.len() && s[i + 1 + j] == '#' {
                    j += 1;
                }
                if j > 256 || s.get(i + 1 + j).copied() != Some('"') {
                    add!();
                } else {
                    skip_n!(j + 2);
                    let mut target = vec!['"'];
                    target.extend(std::iter::repeat('#').take(j));
                    while i < s.len() {
                        if s[i..].starts_with(&target[..]) {
                            skip_n!(target.len());
                            break;
                        }
                        skip!();
                    }
                }
            }
            _ => add!(),
        }
    }

    // Split back into lines, keeping the original count.
    let text: String = out.into_iter().collect();
    let mut result = util::split_lines_at_newline(&text);
    while result.len() < lines.len() {
        result.push(String::new());
    }
    result.truncate(lines.len());
    result
}

/// The length of a character constant or lifetime at s[i], if there is one.
fn match_single_quote(s: &[char], i: usize) -> Option<usize> {
    let rest: String = s[i..s.len().min(i + 12)].iter().collect();
    static QUOTE_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
        [
            r"^'\\u\{[0-7][0-7a-fA-F]{3}\}'",
            r"^'\\x[0-7][0-7a-fA-F]'",
            r#"^'\\[\\"'nrt0]'"#,
            r"^'.'",
        ]
        .iter()
        .map(|p| Regex::new(p).unwrap())
        .collect()
    });
    static LIFETIME: Lazy<Regex> = Lazy::new(|| Regex::new(r"^('static|'[a-zA-Z_])[^']").unwrap());
    for pattern in QUOTE_PATTERNS.iter() {
        if let Some(m) = pattern.find(&rest) {
            return Some(m.as_str().chars().count());
        }
    }
    // A lifetime: skip the name, but not whatever follows it.
    LIFETIME.captures(&rest).map(|m| m[1].chars().count())
}

/// Find blocks, allowing a definition whose `{` is several lines down.
pub fn find_blocks(im: &mut Importer, i1: usize, i2: usize) -> Vec<usize> {
    let min_size = im.spec.minimum_block_size;
    let (mut i, mut prev_i) = (i1, i1);
    let mut results = Vec::new();
    while i < i2 {
        let line = im.guide_lines[i].clone();
        let progress = i;
        i += 1;
        for pi in 0..im.spec.block_patterns.len() {
            let (kind, name) = {
                let (kind, pattern) = &im.spec.block_patterns[pi];
                match pattern.captures(&line) {
                    None => continue,
                    Some(m) => (
                        *kind,
                        m.get(1)
                            .map(|g| g.as_str().trim())
                            .unwrap_or("")
                            .to_string(),
                    ),
                }
            };
            // Rescan from the matching line for the line that opens the block.
            match find_curly_bracket_line(im, i - 1, i2) {
                None => {
                    i = progress + 1;
                    continue;
                }
                Some(j) => i = j + 1,
            }
            let end = im.find_end_of_block(i, i2);
            if min_size == 0 || end.saturating_sub(prev_i) > min_size {
                results.push(im.push_block(kind, &name, prev_i, i, end));
                i = end;
                prev_i = end;
            } else {
                i = end;
            }
            break;
        }
        if i <= progress {
            i = progress + 1;
        }
    }
    results
}

/// The line ending with `{`, or None if the definition is a one-liner.
fn find_curly_bracket_line(im: &Importer, mut i: usize, i2: usize) -> Option<usize> {
    while i < i2 {
        let line = im.guide_lines[i].trim_end();
        if line.ends_with(';') || line.ends_with('}') {
            return None; // A one-line definition.
        }
        if line.ends_with('{') {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Rust headlines: strip generic parameters, which are noise in an outline.
pub fn compute_headline(block: &Block) -> String {
    let name_s = if block.name.is_empty() {
        String::new()
    } else {
        let s = block.name.replace('{', "");
        let mut level = 0i32;
        let mut result = String::new();
        for ch in s.chars() {
            match ch {
                '<' => level += 1,
                '>' => level -= 1,
                _ if level == 0 => result.push(ch),
                _ => {}
            }
        }
        result.replace("  ", " ").trim().to_string()
    };
    if name_s.is_empty() {
        format!("unnamed {}", block.kind)
    } else {
        format!("{} {name_s}", block.kind)
    }
}

/// Move the leading `use` statements, and the comments around them, to the root.
pub fn postprocess(im: &mut Importer) {
    let root = im.root.clone();
    let Some(child1) = root.first_child(im.o) else {
        return;
    };
    let preamble_start = util::split_lines(&im.o.node(child1.v).b)
        .len()
        .saturating_sub(1);
    let preamble_lines = &im.lines[..preamble_start.min(im.lines.len())];

    // Only comment, blank and `use` lines belong to the module, and only if
    // there is a `use` among them: otherwise the comments introduce the first
    // node, not the file.
    let mut found_use = false;
    let mut cut = preamble_lines.len();
    for (i, line) in preamble_lines.iter().enumerate() {
        let stripped = line.trim();
        if stripped.starts_with("use") {
            found_use = true;
        } else if stripped.starts_with("///") {
            if found_use {
                cut = i;
                break;
            }
        } else if !stripped.is_empty() {
            cut = i;
            break;
        }
    }
    if !found_use {
        return;
    }
    let preamble: String = im.lines[..cut].concat();
    if preamble.trim().is_empty() {
        return;
    }
    let parent_b = im.o.node(root.v).b.clone();
    im.o.node_mut(root.v).b = format!("{preamble}{parent_b}");
    let child_b = im.o.node(child1.v).b.replacen(&preamble, "", 1);
    im.o.node_mut(child1.v).b = child_b;

    // Move the child's leading blank lines up, before the @others directive.
    while im.o.node(child1.v).b.starts_with('\n') {
        let parent_b = im.o.node(root.v).b.clone();
        im.o.node_mut(root.v).b = if parent_b.contains("@others") {
            parent_b.replacen("@others", "\n@others", 1)
        } else {
            format!("{parent_b}\n")
        };
        let child_b = im.o.node(child1.v).b[1..].to_string();
        im.o.node_mut(child1.v).b = child_b;
    }
}
