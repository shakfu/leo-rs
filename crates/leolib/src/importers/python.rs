//! The Python importer.
//!
//! Python has no braces, so the block algorithm needs three things of its own:
//! guide lines that understand triple-quoted strings and f-string prefixes, an
//! end-of-block rule based on indentation, and a post-pass that moves module
//! and class docstrings to where a reader expects them.

use once_cell::sync::Lazy;
use regex::Regex;

use crate::importers::block::Importer;
use crate::util;

static STRING_PAT1: Lazy<Regex> = Lazy::new(|| Regex::new(r#"^([fFrR]*)("""|")"#).unwrap());
static STRING_PAT2: Lazy<Regex> = Lazy::new(|| Regex::new(r"^([fFrR]*)('''|')").unwrap());
static CLASS_PAT: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*class\s+(\w+)").unwrap());

/// Guide lines for Python: drop comments and strings, keeping indentation.
///
/// Unlike the default scanner this *removes* the characters rather than
/// blanking them, and turns a line left with only whitespace into a blank
/// line. A docstring therefore reads as blank, which is what the
/// indentation-based end-of-block rule needs.
pub fn delete_comments_and_strings(lines: &[String]) -> Vec<String> {
    let mut delim = String::new(); // The open string delimiter.
    let mut result = Vec::with_capacity(lines.len());
    for line in lines {
        let mut out = String::new();
        let mut i = 0usize;
        while i < line.len() {
            if !delim.is_empty() {
                let (d, j) = skip_string(&delim, i, line);
                delim = d;
                i = j;
                continue;
            }
            let ch = line[i..].chars().next().unwrap();
            if ch == '#' || ch == '\n' {
                break;
            }
            let rest = &line[i..];
            let m = STRING_PAT1
                .captures(rest)
                .or_else(|| STRING_PAT2.captures(rest));
            match m {
                Some(m) => {
                    let prefix = m.get(1).map(|g| g.as_str()).unwrap_or("");
                    let d = m[2].to_string();
                    i += prefix.len() + d.len();
                    delim = d;
                    if i < line.len() {
                        let (d2, j) = skip_string(&delim, i, line);
                        delim = d2;
                        i = j;
                    }
                }
                None => {
                    out.push(ch);
                    i += ch.len_utf8();
                }
            }
        }
        if line.ends_with('\n') {
            out.push('\n');
        }
        if out.trim().is_empty() {
            out = "\n".to_string();
        }
        result.push(out);
    }
    result
}

/// Skip the rest of a string. Returns ("", i) at its end, (delim, len) if it
/// continues on the next line.
fn skip_string(delim: &str, mut i: usize, line: &str) -> (String, usize) {
    if !line.contains(delim) {
        return (delim.to_string(), line.len());
    }
    while i < line.len() {
        let ch = line[i..].chars().next().unwrap();
        if ch == '\\' {
            i += 2;
            continue;
        }
        if line[i.min(line.len())..].starts_with(delim) {
            return (String::new(), i + delim.len());
        }
        i += ch.len_utf8();
    }
    (delim.to_string(), i)
}

/// Find blocks, refusing to nest one `def` inside another (#3517).
pub fn find_blocks(im: &mut Importer, i1: usize, i2: usize) -> Vec<usize> {
    let (mut i, mut prev_i) = (i1, i1);
    let mut results = Vec::new();
    let prev_block_line = if i1 > 0 {
        im.guide_lines[i1 - 1].clone()
    } else {
        String::new()
    };
    while i < i2 {
        let s = im.guide_lines[i].clone();
        i += 1;
        for pi in 0..im.spec.block_patterns.len() {
            let (kind, name) = {
                let (kind, pattern) = &im.spec.block_patterns[pi];
                match pattern.captures(&s) {
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
            let end = find_end_of_block(im, i, i2);
            // A def indented under another def stays in its parent's body.
            let nested_def = kind == "def"
                && prev_block_line.trim_start().starts_with("def ")
                && im.lws_n(&prev_block_line) < im.lws_n(&s);
            if nested_def {
                i = end;
            } else {
                results.push(im.push_block(kind, &name, prev_i, i, end));
                i = end;
                prev_i = end;
            }
            break;
        }
    }
    results
}

/// The line following the class or def whose header is at `i - 1`.
pub fn find_end_of_block(im: &Importer, mut i: usize, i2: usize) -> usize {
    let def_line = im.guide_lines[i - 1].clone();

    // A def whose parameter list spans lines: scan to ')' then to the ':'.
    if def_line.trim_start().starts_with("def ") && !def_line.contains(')') {
        while i < i2 && !im.guide_lines[i].contains(')') {
            i += 1;
        }
        while i < i2 && !im.guide_lines[i].ends_with(":\n") {
            i += 1;
        }
        i += 1;
    }
    if i >= i2 {
        return i2;
    }

    // Scan for a dedent, ignoring lines inside brackets.
    let (mut non_tail_lines, mut tail_lines) = (0usize, 0usize);
    let (mut curlies, mut parens, mut squares) = (0i32, 0i32, 0i32);
    let lws1 = im.lws_n(&def_line);
    while i < i2 {
        let s = im.guide_lines[i].clone();
        if !s.trim().is_empty() {
            // Test the bracket state at the *start* of the line.
            if im.lws_n(&s) <= lws1 && (curlies, parens, squares) == (0, 0, 0) {
                return if non_tail_lines == 0 {
                    i
                } else {
                    i - tail_lines
                };
            }
            curlies += count(&s, '{') - count(&s, '}');
            parens += count(&s, '(') - count(&s, ')');
            squares += count(&s, '[') - count(&s, ']');
            non_tail_lines += 1;
            tail_lines = 0;
            i += 1;
            continue;
        }
        // A blank, comment or docstring line: consult the *real* line.
        let real = &im.lines[i];
        let stripped = real.trim();
        if stripped.is_empty() {
            tail_lines += 1;
        } else if stripped.starts_with('#') {
            if im.lws_n(real) < lws1 && non_tail_lines > 0 {
                return i - tail_lines;
            }
            tail_lines += 1;
        } else {
            // A docstring line, which the guide lines blanked out.
            non_tail_lines += 1;
            tail_lines = 0;
        }
        i += 1;
    }
    i2
}

fn count(s: &str, ch: char) -> i32 {
    s.matches(ch).count() as i32
}

/// Python's post-pass: headlines, the module preamble and class docstrings.
pub fn postprocess(im: &mut Importer) {
    adjust_headlines(im);
    let patterns: Vec<Regex> = im
        .spec
        .block_patterns
        .iter()
        .map(|(_, p)| p.clone())
        .collect();
    im.move_module_preamble(&patterns);
    move_class_docstrings(im);
    adjust_at_others(im);
}

/// Qualify a method's headline with its class, and mark plain functions.
fn adjust_headlines(im: &mut Importer) {
    let root = im.root.clone();
    for child in root.subtree(im.o) {
        let h = child.h(im.o).to_string();
        let Some(name) = h.strip_prefix("def ") else {
            continue;
        };
        let name = name.trim().to_string();
        let mut found = None;
        for ancestor in child.parents(im.o) {
            if ancestor == root {
                break;
            }
            if let Some(m) = CLASS_PAT.captures(ancestor.h(im.o)) {
                found = Some(m[1].to_string());
                break;
            }
        }
        let new_h = match found {
            Some(class_name) => format!("{class_name}.{name}"),
            None => format!("function: {name}"),
        };
        im.o.node_mut(child.v).h = new_h;
    }
}

/// Move a class docstring from the class's first child up to the class node.
fn move_class_docstrings(im: &mut Importer) {
    let root = im.root.clone();
    for p in root.subtree(im.o) {
        if !p.h(im.o).starts_with("class ") {
            continue;
        }
        let Some(child1) = p.first_child(im.o) else {
            continue;
        };
        let Some(docstring) = find_docstring(im.o.node(child1.v).b.as_str()) else {
            continue;
        };
        // Remove it from the child.
        let child_b = im.o.node(child1.v).b.replacen(&docstring, "", 1);
        im.o.node_mut(child1.v).b = child_b;
        // Insert it after the class line, indented to match @others.
        let class_lines = util::split_lines(&im.o.node(p.v).b);
        let mut n = 0usize;
        let mut found = false;
        while n < class_lines.len() {
            let line = &class_lines[n];
            n += 1;
            if line.trim_start().starts_with("class ") {
                found = true;
                break;
            }
        }
        if !found {
            continue;
        }
        let indent = class_lines
            .iter()
            .find(|line| line.trim_start().starts_with("@others"))
            .map(|line| line.chars().count() - line.trim_start().chars().count())
            .unwrap_or(4);
        let pad = " ".repeat(indent);
        let doc_lines: String = util::split_lines(&docstring)
            .iter()
            .map(|z| {
                if z.trim().is_empty() {
                    "\n".to_string()
                } else {
                    format!("{pad}{z}")
                }
            })
            .collect();
        im.o.node_mut(p.v).b = format!(
            "{}{doc_lines}{}",
            class_lines[..n].concat(),
            class_lines[n..].concat()
        );
    }
}

/// The leading docstring of a body, or None. A regex cannot do this reliably.
fn find_docstring(b: &str) -> Option<String> {
    let delims = ["\"\"\"", "'''"];
    let stripped = b.trim();
    if stripped.is_empty() {
        return None;
    }
    let delim = delims.iter().find(|d| stripped.starts_with(**d))?;
    let lines = util::split_lines(b);
    if lines[0].matches(delim).count() == 2 {
        return Some(lines[0].clone());
    }
    let mut i = 1usize;
    while i < lines.len() {
        if lines[i].contains(delim) {
            i += 1;
            // Take the blank lines that follow the docstring with it.
            while i < lines.len() && lines[i].trim().is_empty() {
                i += 1;
            }
            return Some(lines[..i].concat());
        }
        i += 1;
    }
    None
}

/// Put a blank line before `@others` in a class, taking it from the first child.
fn adjust_at_others(im: &mut Importer) {
    let root = im.root.clone();
    for p in root.subtree(im.o) {
        if !p.h(im.o).starts_with("class") || !p.has_children(im.o) {
            continue;
        }
        let Some(child) = p.first_child(im.o) else {
            continue;
        };
        if !im.o.node(child.v).b.starts_with('\n') {
            continue;
        }
        let lines = util::split_lines(&im.o.node(p.v).b);
        for (i, line) in lines.iter().enumerate() {
            if line.trim_start().starts_with("@others") {
                im.o.node_mut(p.v).b = format!("{}\n{}", lines[..i].concat(), lines[i..].concat());
                let child_b = im.o.node(child.v).b[1..].to_string();
                im.o.node_mut(child.v).b = child_b;
                break;
            }
        }
    }
}
