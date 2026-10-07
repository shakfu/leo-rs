//! `@entangled <path>`: a markdown file whose named code fences are nodes.
//!
//! A leo-rs kind with no Leo counterpart; the design is in
//! `docs/dev/entangled_leo_backend.md`. Headings are nodes. A *named* fence,
//! as entangled names one, keeps its fence lines in its heading's body with
//! a `<< name >>` reference in place of the code, and the code becomes a child
//! headlined `<< name >>`. Unnamed fences and prose stay text.
//!
//! The scanner follows CommonMark rather than `@auto-md`'s importer, because
//! the file must render on GitHub. What a heading and a fence were in the file
//! is kept in in-memory attributes: the tree is rebuilt from the file on every
//! open and never stored in the `.leo` file, so the attributes never are.

use once_cell::sync::Lazy;
use regex::Regex;

use crate::error::{Error, Result};
use crate::node::{Ua, VnodeId};
use crate::outline::Outline;
use crate::position::Position;
use crate::util;

/// A heading's lines as the file spelt them.
const HEADING: &str = "leo-rs-heading";
/// A heading's text when read, to tell whether the headline was edited.
const HEADING_TEXT: &str = "leo-rs-heading-text";
/// `#`, `=` or `-`: how to write the heading again once its headline changes.
const HEADING_MARK: &str = "leo-rs-heading-mark";
/// A heading's level, 1 to 6.
const HEADING_LEVEL: &str = "leo-rs-heading-level";
/// A fence node's language, from its info string. `Outline::language_at`
/// reads it, since an `@language` line would be written into the markdown.
pub const LANGUAGE: &str = "leo-rs-language";
/// The indent taken off a fence node's code, put back on write.
const INDENT: &str = "leo-rs-fence-indent";

static FENCE_OPEN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^( {0,3})(`{3,}|~{3,})([^\n]*)\n?$").unwrap());
static ATX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^ {0,3}(#{1,6})(?:[ \t]+(.*?))?(?:[ \t]+#+)?[ \t]*\n?$").unwrap());
static SETEXT: Lazy<Regex> = Lazy::new(|| Regex::new(r"^ {0,3}(=+|-+)[ \t]*\n?$").unwrap());
static NOT_A_HEADING_TEXT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^ {0,3}([-*+>]|\d{1,9}[.)])(\s|$)|^ {4}|^\s*$").unwrap());
static REFERENCE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[ \t]*(<< .* >>)[ \t]*\n?$").unwrap());

/// What a fence's info string and opening lines say about it.
struct FenceInfo {
    /// The block's name as entangled gives it, or None for an unnamed fence.
    name: Option<String>,
    language: Option<String>,
}

/// Read the `@entangled` file at p into its tree.
///
/// The tree is written back and compared with the file before it is kept.
/// If they differ, the whole file goes into p's body, so a write reproduces
/// it, and the read reports an error.
pub fn read_one_at_entangled_node(o: &mut Outline, p: &Position) -> Result<bool> {
    let path = o.full_path(p);
    let contents = crate::external::read_file_to_string(&path)?;
    let text = contents.replace('\r', "");
    o.detach_subtree(p.v);
    o.node_mut(p.v).b = String::new();
    build(o, p.v, &text);
    let written = write_string(o, p);
    if written.as_deref().ok() != Some(text.as_str()) {
        o.detach_subtree(p.v);
        o.node_mut(p.v).b = contents;
        o.remember_read_path(p, &path);
        let detail = written.map_or_else(|e| e.to_string(), |_| "text differs".to_string());
        return Err(Error::Import {
            path: util::short_file_name(&path),
            detail: format!(
                "the @entangled reader did not reproduce it: {detail}. \
                 The whole file is in the node's body."
            ),
        });
    }
    if contents.contains('\r') {
        let gnx = p.gnx(o).to_string();
        o.import_warnings.insert(
            gnx,
            "CRLF line endings become LF on the next write".to_string(),
        );
    }
    o.remember_read_path(p, &path);
    o.clear_dirty_in_tree(p);
    Ok(true)
}

/// Build root's tree from `text`, which has LF line endings.
fn build(o: &mut Outline, root: VnodeId, text: &str) {
    let owned = util::split_lines(text);
    let lines: Vec<&str> = owned.iter().map(String::as_str).collect();
    // The open headings, innermost last, with their levels; the root is 0.
    let mut stack: Vec<(VnodeId, usize)> = vec![(root, 0)];
    let mut bodies: Vec<(VnodeId, String)> = vec![(root, String::new())];
    let body = |bodies: &mut Vec<(VnodeId, String)>, v: VnodeId| -> usize {
        match bodies.iter().position(|(w, _)| *w == v) {
            Some(i) => i,
            None => {
                bodies.push((v, String::new()));
                bodies.len() - 1
            }
        }
    };
    let mut i = front_matter(&lines);
    if i > 0 {
        bodies[0].1.push_str(&lines[..i].concat());
    }
    while i < lines.len() {
        let line = lines[i];
        let top = stack.last().unwrap().0;
        if let Some(open) = FENCE_OPEN.captures(line) {
            let indent = open[1].to_string();
            let marker = open[2].to_string();
            let info = open[3].trim().to_string();
            // A backtick fence's info string has no backticks.
            if !(marker.starts_with('`') && info.contains('`')) {
                let mut j = i + 1;
                let mut close = None;
                while j < lines.len() {
                    if closes(lines[j], &marker) {
                        close = Some(j);
                        break;
                    }
                    j += 1;
                }
                let code_end = close.unwrap_or(lines.len());
                let code = &lines[i + 1..code_end];
                let fence = fence_info(&info, code);
                let k = body(&mut bodies, top);
                match fence.name {
                    Some(name) => {
                        let child = o.new_child_vnode(top);
                        let headline = format!("<< {name} >>");
                        o.node_mut(child).h = headline.clone();
                        let strip = !indent.is_empty()
                            && code.iter().all(|l| *l == "\n" || l.starts_with(&indent));
                        let code_text: String = code
                            .iter()
                            .map(|l| match strip && *l != "\n" {
                                true => &l[indent.len()..],
                                false => l,
                            })
                            .collect();
                        o.node_mut(child).b = code_text;
                        if let Some(lang) = fence.language {
                            set(o, child, LANGUAGE, &lang);
                        }
                        if strip {
                            set(o, child, INDENT, &indent);
                        }
                        bodies[k].1.push_str(line);
                        bodies[k].1.push_str(&format!("{indent}{headline}\n"));
                    }
                    None => bodies[k].1.push_str(&lines[i..code_end].concat()),
                }
                if let Some(c) = close {
                    bodies[k].1.push_str(lines[c]);
                    i = c + 1;
                } else {
                    i = code_end;
                }
                continue;
            }
        }
        let heading = atx(line)
            .map(|(level, text)| (level, text, 1, "#"))
            .or_else(|| {
                let next = lines.get(i + 1)?;
                let m = SETEXT.captures(next)?;
                if NOT_A_HEADING_TEXT.is_match(line)
                    || FENCE_OPEN.is_match(line)
                    || atx(line).is_some()
                {
                    return None;
                }
                let (level, mark) = match m[1].starts_with('=') {
                    true => (1, "="),
                    false => (2, "-"),
                };
                Some((level, line.trim().to_string(), 2, mark))
            });
        if let Some((level, text, n, mark)) = heading {
            while stack.last().unwrap().1 >= level {
                stack.pop();
            }
            let parent = stack.last().unwrap().0;
            let v = o.new_child_vnode(parent);
            o.node_mut(v).h = text.clone();
            set(o, v, HEADING, &lines[i..i + n].concat());
            set(o, v, HEADING_TEXT, &text);
            set(o, v, HEADING_MARK, mark);
            set(o, v, HEADING_LEVEL, &level.to_string());
            stack.push((v, level));
            body(&mut bodies, v);
            i += n;
            continue;
        }
        let k = body(&mut bodies, top);
        bodies[k].1.push_str(line);
        i += 1;
    }
    for (v, b) in bodies {
        o.node_mut(v).b = b;
    }
}

/// How many lines a YAML front matter block at the very start takes, or 0.
fn front_matter(lines: &[&str]) -> usize {
    if lines.first() != Some(&"---\n") {
        return 0;
    }
    lines
        .iter()
        .skip(1)
        .position(|l| *l == "---\n" || *l == "...\n" || *l == "---" || *l == "...")
        .map_or(0, |k| k + 2)
}

/// An ATX heading's level and text.
fn atx(line: &str) -> Option<(usize, String)> {
    let m = ATX.captures(line)?;
    let text = m.get(2).map_or("", |t| t.as_str()).trim().to_string();
    (!text.is_empty()).then(|| (m[1].len(), text))
}

/// Whether `line` closes a fence opened with `marker`: the same character,
/// at least as many, and nothing else.
fn closes(line: &str, marker: &str) -> bool {
    let c = marker.chars().next().unwrap();
    let t = line.trim_end();
    let lead = t.len() - t.trim_start_matches(' ').len();
    let run = t.trim_start_matches(' ');
    lead <= 3 && run.len() >= marker.len() && run.chars().all(|x| x == c)
}

/// The name and language a fence's info string and opening code lines give,
/// in each of entangled's styles: `python #main file=out.py`,
/// `{.python #main file=out.py}`, `{python, label=main, file=out.py}`, and
/// `{python}` with `#| label: main` or `#| file: out.py` lines.
fn fence_info(info: &str, code: &[&str]) -> FenceInfo {
    let tokens: Vec<&str> = info
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | '{' | '}'))
        .filter(|t| !t.is_empty())
        .collect();
    // A Pandoc class, `.rust`, names the language even after a word such as
    // `text` that keeps GitHub from guessing; otherwise the first word does.
    let language = tokens
        .iter()
        .find_map(|t| t.strip_prefix('.'))
        .or_else(|| {
            tokens
                .first()
                .copied()
                .filter(|t| !t.starts_with('#') && !t.contains('='))
        })
        .map(leo_language)
        .filter(|t| !t.is_empty());
    let mut name = None;
    let mut file = None;
    for t in &tokens {
        if let Some(n) = t.strip_prefix('#') {
            name = name.or(Some(n.to_string()));
        } else if let Some(n) = t.strip_prefix("label=") {
            name = name.or(Some(unquote(n)));
        } else if let Some(f) = t.strip_prefix("file=") {
            file = file.or(Some(unquote(f)));
        }
    }
    for line in code.iter().take_while(|l| l.trim_start().starts_with("#|")) {
        let option = line.trim_start().trim_start_matches("#|").trim();
        if let Some(n) = option.strip_prefix("label:") {
            name = name.or(Some(unquote(n.trim())));
        } else if let Some(f) = option.strip_prefix("file:") {
            file = file.or(Some(unquote(f.trim())));
        }
    }
    FenceInfo {
        name: name.filter(|n| !n.is_empty()).or(file),
        language,
    }
}

/// Leo's name for a fence's language: `python` for `python` or `py`,
/// `cplusplus` for `cpp`, `shell` for `sh`.
fn leo_language(word: &str) -> String {
    // Extensions first: Leo's tables also list `cpp` as a language, but
    // leo-rs's colouring and servers know C++ as `cplusplus`.
    let word = word.to_lowercase();
    match crate::langdata::extension_dict().get(word.as_str()) {
        Some(lang) => lang.to_string(),
        None => word,
    }
}

fn unquote(s: &str) -> String {
    s.trim_matches(|c| c == '"' || c == '\'').to_string()
}

fn set(o: &mut Outline, v: VnodeId, key: &str, value: &str) {
    o.node_mut(v)
        .uas
        .insert(key.to_string(), Ua::Text(value.to_string()));
}

fn get<'a>(o: &'a Outline, v: VnodeId, key: &str) -> Option<&'a str> {
    match o.node(v).uas.get(key)? {
        Ua::Text(s) => Some(s),
        Ua::Opaque(_) => None,
    }
}

/// The `@entangled` file the tree at p writes.
///
/// Refuses rather than drop code: a reference with no fence node, or a fence
/// node no reference places, is an error.
pub fn write_string(o: &Outline, p: &Position) -> Result<String> {
    let mut out = String::new();
    write_node(o, p.v, 0, &mut out)?;
    Ok(out)
}

fn write_node(o: &Outline, v: VnodeId, level: usize, out: &mut String) -> Result<()> {
    let node = o.node(v);
    let level = match level {
        0 => 0,
        _ => {
            let level = get(o, v, HEADING_LEVEL)
                .and_then(|l| l.parse().ok())
                .unwrap_or(level)
                .min(6);
            heading(o, v, level, out);
            level
        }
    };
    let (fences, headings): (Vec<VnodeId>, Vec<VnodeId>) = node
        .children
        .iter()
        .partition(|&&c| o.node(c).h.starts_with("<< ") && get(o, c, HEADING).is_none());
    let mut fences = fences.into_iter();
    let owned = util::split_lines(&node.b);
    let lines: Vec<&str> = owned.iter().map(String::as_str).collect();
    let mut i = 0;
    while i < lines.len() {
        out.push_str(lines[i]);
        let opens = FENCE_OPEN.is_match(lines[i]);
        if let (true, Some(next)) = (opens, lines.get(i + 1)) {
            if let Some(m) = REFERENCE.captures(next) {
                let name = &m[1];
                let Some(f) = fences.next() else {
                    return Err(write_error(o, v, &format!("no fence node for {name}")));
                };
                if o.node(f).h.trim() != name {
                    return Err(write_error(
                        o,
                        v,
                        &format!("{name} is placed where {} should be", o.node(f).h),
                    ));
                }
                // The closing fence, if there is one, must start its own line.
                fence_code(o, f, i + 2 < lines.len(), out);
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    if let Some(f) = fences.next() {
        return Err(write_error(
            o,
            v,
            &format!("{} has no reference in its parent's body", o.node(f).h),
        ));
    }
    for h in headings {
        write_node(o, h, level + 1, out)?;
    }
    Ok(())
}

/// A heading as the file had it, or written fresh in the same style once
/// its headline changed. A node the reader did not make is an ATX heading
/// one level below its parent.
fn heading(o: &Outline, v: VnodeId, level: usize, out: &mut String) {
    let h = o.node(v).h.trim();
    if get(o, v, HEADING_TEXT) == Some(h) {
        if let Some(lines) = get(o, v, HEADING) {
            out.push_str(lines);
            return;
        }
    }
    match get(o, v, HEADING_MARK) {
        Some(mark @ ("=" | "-")) => {
            out.push_str(&format!("{h}\n{}\n", mark.repeat(h.chars().count().max(3))));
        }
        _ => out.push_str(&format!("{} {h}\n", "#".repeat(level.max(1)))),
    }
}

/// A fence node's code, with its indent put back, ending in a newline if a
/// closing fence follows.
fn fence_code(o: &Outline, f: VnodeId, closed: bool, out: &mut String) {
    let indent = get(o, f, INDENT).unwrap_or("");
    let body = &o.node(f).b;
    for line in util::split_lines(body) {
        if line != "\n" {
            out.push_str(indent);
        }
        out.push_str(&line);
    }
    if closed && !body.is_empty() && !body.ends_with('\n') {
        out.push('\n');
    }
}

fn write_error(o: &Outline, v: VnodeId, detail: &str) -> Error {
    Error::Write {
        detail: format!("@entangled, under \"{}\": {detail}", o.node(v).h),
    }
}
