//! Markdown files as trees: headings are nodes, and chosen code fences are
//! child nodes headlined `<< name >>`. The `@qmd` and `@rmd` kinds are here;
//! `leo-entangled`'s `@entangled` builds on the same scanner and writer.
//!
//! A fence that becomes a node keeps its fence lines in its heading's body,
//! with a `<< name >>` reference in place of the code. Other fences and
//! prose stay text.
//!
//! The scanner follows CommonMark rather than `@auto-md`'s importer, because
//! the file must render on GitHub and in Quarto. What a heading and a fence
//! were in the file is kept in in-memory attributes: the tree is rebuilt from
//! the file on every open and never stored in the `.leo` file, so the
//! attributes never are. The writer reproduces the file byte for byte,
//! except that CRLF line endings and a byte-order mark are dropped; a read
//! reports either.

use regex::Regex;
use std::sync::LazyLock as Lazy;

use leolib::error::{Error, Result};
use leolib::ext::Rename;
use leolib::node::{Ua, VnodeId};
use leolib::util;
use leolib::Outline;
use leolib::Position;

/// A heading's lines as the file spelt them.
const HEADING: &str = "leo-rs-heading";
/// A heading's text when read, to tell whether the headline was edited.
const HEADING_TEXT: &str = "leo-rs-heading-text";
/// `#`, `=` or `-`: how to write the heading again once its headline changes.
const HEADING_MARK: &str = "leo-rs-heading-mark";
/// A heading's level, 1 to 6.
const HEADING_LEVEL: &str = "leo-rs-heading-level";
/// Set on a markdown root whose last read failed or was refused. Until it
/// has cells again, [`CELL_IDS`] keeps what the last good read saved.
const REFUSED: &str = "leo-rs-refused";
/// A heading's depth below the root node when read. A heading moved
/// to another depth has its level moved by as much.
const HEADING_DEPTH: &str = "leo-rs-heading-depth";
pub use leolib::ext::LANGUAGE;
/// The indent taken off a fence node's code, put back on write.
const INDENT: &str = "leo-rs-fence-indent";
/// A markdown root's labelled fence nodes, `name gnx` a line, saved in the
/// `.leo` file so a cell gets its vnode back on every read: a clone of it in
/// an `@clean` tree stays a clone. `str_` makes Leo keep it as text.
const CELL_IDS: &str = "str_leo-rs-cell-ids";

static FENCE_OPEN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^( {0,3})(`{3,}|~{3,})([^\n]*)\n?$").unwrap());
static ATX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^ {0,3}(#{1,6})(?:[ \t]+(.*?))?(?:[ \t]+#+)?[ \t]*\n?$").unwrap());
/// A Pandoc fenced div's opening line: three or more colons, then a class
/// or `{attributes}`, optionally more colons. Quarto's callouts and tabsets.
static DIV_OPEN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^ {0,3}:{3,}[ \t]*(\{[^}]*\}|[^\s:{}]+)[ \t]*:*[ \t]*\n?$").unwrap());
/// A fenced div's closing line: colons only.
static DIV_CLOSE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^ {0,3}:{3,}[ \t]*\n?$").unwrap());
static SETEXT: Lazy<Regex> = Lazy::new(|| Regex::new(r"^ {0,3}(=+|-+)[ \t]*\n?$").unwrap());
static NOT_A_HEADING_TEXT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^ {0,3}([-*+>]|\d{1,9}[.)])(\s|$)|^ {4}|^\s*$").unwrap());
static REFERENCE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[ \t]*(<< .* >>)[ \t]*\n?$").unwrap());

/// How a document names its blocks, as entangled decides it: by the file's
/// extension, as entangled's `Style::from_extension` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Style {
    /// `.md`: `python #main file=out.py`, or Pandoc's `{.python #main file=out.py}`.
    Markdown,
    /// `.qmd`: `{python}` with `#| label: main` and `#| file: out.py` lines.
    Quarto,
    /// `.Rmd`: `{python, label=main, file=out.py}`.
    Knitr,
}

impl Style {
    /// The style a file's extension gives: `.qmd` Quarto, `.rmd` knitr,
    /// anything else markdown.
    pub fn of(path: &str) -> Style {
        let ext = std::path::Path::new(path)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase());
        match ext.as_deref() {
            Some("qmd") => Style::Quarto,
            Some("rmd") => Style::Knitr,
            _ => Style::Markdown,
        }
    }
}

/// A kind's code for a fence node, if it has any: what the writer writes in
/// place of the node's body.
pub type Code<'a> = dyn Fn(VnodeId) -> Option<String> + 'a;

/// The info string of a fence opening line, `{python}` in ```` ```{python} ````;
/// None if `line` opens no fence.
pub fn fence_opener_info(line: &str) -> Option<&str> {
    Some(FENCE_OPEN.captures(line)?.get(3)?.as_str().trim())
}

/// The opening line of the fence whose node is f: in f's parent's body, the
/// line before f's `<< name >>` reference.
pub fn fence_line(o: &Outline, f: &Position) -> Option<String> {
    let parent = f.parent(o)?;
    let lines = util::split_lines(parent.b(o));
    let reference = f.h(o).trim();
    let k = lines
        .iter()
        .position(|l| REFERENCE.captures(l).is_some_and(|m| &m[1] == reference))?;
    lines.get(k.checked_sub(1)?).cloned()
}

/// Which fences become nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Policy {
    /// `@entangled`: a fence entangled names, by `#name`, a label, or its
    /// `file=` or `include=` target.
    Named,
    /// `@qmd`, `@rmd`: an executable cell (`{python}`, `{r}`), and a fence
    /// with a `#name` or label. An unnamed cell is headlined by its
    /// language and number, `<< python cell 3 >>`.
    Cells,
}

/// What a fence's info string and opening lines say about it.
#[non_exhaustive]
pub struct FenceInfo {
    /// The block's name as entangled gives it, or None for an unnamed fence.
    pub name: Option<String>,
    /// The name its `#name` or label gives it, if any.
    pub label: Option<String>,
    /// knitr's positional label, `setup` in `{r setup, echo=FALSE}`, which
    /// entangled does not read.
    pub chunk_label: Option<String>,
    /// Leo's name for the fence's language, `python` for `py`.
    pub language: Option<String>,
    /// `include=path` or `include=path#anchor`, in a `.md` file only: knitr
    /// and Quarto already have an `include` option, a yes or no.
    pub include: Option<String>,
}

/// Read the file of the markdown-tree node at p into its tree, as `policy`
/// chooses its nodes. Returns notes for the read report.
///
/// The tree is written back and compared with the file before it is kept.
/// If they differ, or two fences share a name, the whole file goes into p's
/// body, so a write reproduces it, and the read reports an error.
pub fn read(o: &mut Outline, p: &Position, policy: Policy, style: Style) -> Result<Vec<String>> {
    let path = o.full_path(p);
    set(o, p.v, REFUSED, "");
    let contents = leolib::external::read_file_to_string(&path)?;
    let mut notes = read_contents(o, p, &contents, &path, policy, style)?;
    if has_bom(&path) {
        notes.push("the byte-order mark is dropped on the next write".to_string());
    }
    Ok(notes)
}

/// [`read`], of `contents` as the file at `path` holds them.
fn read_contents(
    o: &mut Outline,
    p: &Position,
    contents: &str,
    path: &str,
    policy: Policy,
    style: Style,
) -> Result<Vec<String>> {
    let text = contents.replace("\r\n", "\n");
    let ids = get(o, p.v, CELL_IDS)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.rsplit_once(' '))
        .map(|(name, gnx)| (name.to_string(), gnx.to_string()))
        .collect();
    o.detach_subtree_keeping_clones(p.v);
    o.node_mut(p.v).b = String::new();
    build(o, p.v, &text, style, policy, &ids);
    // The check below must write as any later write does.
    o.node_mut(p.v).uas.remove(REFUSED);
    let kind = p
        .h(o)
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string();
    let refuse = |o: &mut Outline, detail: String| {
        o.detach_subtree_keeping_clones(p.v);
        o.node_mut(p.v).b = contents.to_string();
        set(o, p.v, REFUSED, "");
        o.remember_read_path(p, path);
        Err(Error::Import {
            path: util::short_file_name(path),
            detail: format!("{detail}. The whole file is in the node's body."),
        })
    };
    if text.contains('\r') {
        return refuse(o, "a line ends in CR alone".to_string());
    }
    // A name is one block: Leo refuses a section defined twice, and so does
    // this, though entangled joins the parts.
    if let Some((name, n)) = duplicate_name(o, p) {
        return refuse(
            o,
            format!("block `{name}` is defined by {n} fences; give each a name of its own"),
        );
    }
    let written = write_tree(o, p, &|_| None);
    if written.as_deref().ok() != Some(text.as_str()) {
        let detail = written.map_or_else(|e| e.to_string(), |_| "text differs".to_string());
        return refuse(
            o,
            format!("the {kind} reader did not reproduce it: {detail}"),
        );
    }
    let mut notes = Vec::new();
    if contents.contains("\r\n") {
        notes.push("CRLF line endings become LF on the next write".to_string());
    }
    Ok(notes)
}

/// Entry points for the fuzz targets in `fuzz/`, behind the `fuzzing`
/// feature. Not an API.
#[cfg(feature = "fuzzing")]
pub mod fuzz {
    use super::*;

    /// Read `text` as an `@qmd` file, or an `@rmd` one, into a fresh outline.
    /// A read that is kept must write back to `text`, CRLF aside.
    pub fn read(text: &str, rmd: bool) {
        let (kind, style) = match rmd {
            true => ("@rmd fuzz.Rmd", Style::Knitr),
            false => ("@qmd fuzz.qmd", Style::Quarto),
        };
        let mut o = Outline::new_empty();
        let p = o.root_position().expect("a new outline has a node");
        o.set_headline(&p, kind);
        if read_contents(&mut o, &p, text, "fuzz", Policy::Cells, style).is_ok() {
            let written = write_string(&o, &p).expect("a kept read writes");
            assert_eq!(written, text.replace("\r\n", "\n"));
        }
    }
}

/// Whether the file at `path` starts with a UTF-8 byte-order mark, which
/// [`leolib::external::read_file_to_string`] removes.
fn has_bom(path: &str) -> bool {
    use std::io::Read;
    let mut head = [0u8; 3];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut head))
        .is_ok_and(|()| head == [0xEF, 0xBB, 0xBF])
}

/// Build root's tree from `text`, which has LF line endings.
fn build(
    o: &mut Outline,
    root: VnodeId,
    text: &str,
    style: Style,
    policy: Policy,
    ids: &std::collections::HashMap<String, String>,
) {
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
    // Executable cells so far, to number the unnamed ones.
    let mut cells = 0;
    // Fenced divs open at this line.
    let mut divs = 0usize;
    // Inside an HTML comment that spans lines: its lines are text.
    let mut comment = false;
    let mut i = front_matter(&lines);
    if i > 0 {
        bodies[0].1.push_str(&lines[..i].concat());
    }
    while i < lines.len() {
        let line = lines[i];
        let top = stack.last().unwrap().0;
        // CommonMark's HTML block 2: a line starting `<!--`, to `-->`.
        let lead = line.len() - line.trim_start_matches(' ').len();
        let opens_comment =
            lead <= 3 && line[lead..].starts_with("<!--") && !line[lead + 4..].contains("-->");
        if comment || opens_comment {
            comment = match comment {
                true => !line.contains("-->"),
                false => true,
            };
            let k = body(&mut bodies, top);
            bodies[k].1.push_str(line);
            i += 1;
            continue;
        }
        if let Some(open) = FENCE_OPEN.captures(line) {
            let indent = open[1].to_string();
            let marker = open[2].to_string();
            let info = open[3].trim().to_string();
            // A backtick fence's info string has no backticks. An opener on
            // the file's last line, with no newline, has no code: it is text.
            if !(marker.starts_with('`') && info.contains('`')) && line.ends_with('\n') {
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
                let fence = fence_info(&info, code, style);
                let k = body(&mut bodies, top);
                let cell = is_cell(&info);
                cells += usize::from(cell);
                let name = match policy {
                    Policy::Named => fence.name,
                    Policy::Cells => fence.label.or(fence.chunk_label).or_else(|| {
                        let language = fence.language.as_deref().unwrap_or("code");
                        cell.then(|| format!("{language} cell {cells}"))
                    }),
                };
                match name {
                    Some(name) => {
                        let child = match ids.get(&name) {
                            Some(gnx) => {
                                // Never the root or an open heading: that would
                                // make a cycle, from a map edited by hand.
                                let open = |v: VnodeId| v == root || stack.iter().any(|s| s.0 == v);
                                let v = match o.find_gnx(gnx) {
                                    Some(v) if !open(v) => v,
                                    Some(_) => o.new_vnode(None),
                                    None => o.new_vnode(Some(gnx)),
                                };
                                // What the last read noted is this read's to say.
                                o.node_mut(v).uas.retain(|k, _| !k.starts_with("leo-rs-"));
                                o.link_as_last_child_raw(top, v);
                                v
                            }
                            None => o.new_child_vnode(top),
                        };
                        let headline = format!("<< {name} >>");
                        o.node_mut(child).h = headline.clone();
                        // A line that is just the indent would strip to an
                        // empty line, which the writer leaves unindented.
                        let just_indent = format!("{indent}\n");
                        let strip = !indent.is_empty()
                            && code.iter().all(|l| *l == "\n" || l.starts_with(&indent))
                            && !code.contains(&just_indent.as_str());
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
        // A heading inside a fenced div is the div's (a callout's title, a
        // tabset's tab), so it stays text: a node there would put the
        // div's opening and closing lines in different bodies.
        if DIV_OPEN.is_match(line) {
            divs += 1;
        } else if divs > 0 && DIV_CLOSE.is_match(line) {
            divs -= 1;
        }
        let heading = atx(line)
            .map(|(level, text)| (level, text, 1, "#"))
            .or_else(|| {
                let next = lines.get(i + 1)?;
                let m = SETEXT.captures(next)?;
                if NOT_A_HEADING_TEXT.is_match(line)
                    || FENCE_OPEN.is_match(line)
                    || DIV_CLOSE.is_match(line)
                    || DIV_OPEN.is_match(line)
                    || atx(line).is_some()
                {
                    return None;
                }
                let (level, mark) = match m[1].starts_with('=') {
                    true => (1, "="),
                    false => (2, "-"),
                };
                Some((level, line.trim().to_string(), 2, mark))
            })
            .filter(|_| divs == 0);
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
            set(o, v, HEADING_DEPTH, &(stack.len() - 1).to_string());
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

/// Whether a fence's info string makes it an executable cell, as Quarto
/// and knitr read one: a braced engine name, `{python}` or `{r, echo=FALSE}`,
/// and not a Pandoc class (`{.python}`) or raw block (`{=html}`).
fn is_cell(info: &str) -> bool {
    let Some(inner) = info.trim().strip_prefix('{') else {
        return false;
    };
    let engine = inner.trim_start();
    engine.starts_with(|c: char| c.is_ascii_alphabetic())
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
/// read as `style` reads them. entangled skips a fence named in another
/// style's way, so this does too: such a fence stays text.
pub fn fence_info(info: &str, code: &[&str], style: Style) -> FenceInfo {
    let tokens = info_tokens(info);
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
    let mut include = None;
    for t in &tokens {
        match style {
            Style::Markdown => {
                if let Some(n) = t.strip_prefix('#') {
                    name = name.or(Some(n.to_string()));
                } else if let Some(f) = t.strip_prefix("file=") {
                    file = file.or(Some(unquote(f)));
                } else if let Some(i) = t.strip_prefix("include=") {
                    include = include.or(Some(unquote(i)));
                }
            }
            // knitr reads options only from a braced chunk header.
            Style::Knitr if info.trim_start().starts_with('{') => {
                if let Some(n) = t.strip_prefix("label=") {
                    name = name.or(Some(unquote(n)));
                } else if let Some(f) = t.strip_prefix("file=") {
                    file = file.or(Some(unquote(f)));
                }
            }
            Style::Knitr | Style::Quarto => {}
        }
    }
    let options = match style {
        Style::Quarto => code,
        _ => &[],
    };
    for line in options
        .iter()
        .take_while(|l| l.trim_start().starts_with("#|"))
    {
        let option = line.trim_start().trim_start_matches("#|").trim();
        if let Some(n) = option.strip_prefix("label:") {
            name = name.or(Some(unquote(n.trim())));
        } else if let Some(f) = option.strip_prefix("file:") {
            file = file.or(Some(unquote(f.trim())));
        }
    }
    let label = name.filter(|n| !n.is_empty());
    let braced = info.trim_start().starts_with('{');
    let chunk_label = match (style, tokens.get(1)) {
        (Style::Knitr, Some(t)) if braced && !t.contains('=') => Some(unquote(t)),
        _ => None,
    };
    FenceInfo {
        chunk_label,
        // An `include=` fence is a node even unnamed: it shows its file.
        name: label.clone().or(file).or(include.clone()),
        label,
        language,
        include,
    }
}

/// An info string's words, split at whitespace, `,`, `{` and `}` outside
/// quotes: knitr's `label='a b'` is one word.
fn info_tokens(info: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut start = None;
    let mut quote = None;
    for (i, c) in info.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                start.get_or_insert(i);
            }
            None if c.is_whitespace() || matches!(c, ',' | '{' | '}') => {
                if let Some(s) = start.take() {
                    tokens.push(&info[s..i]);
                }
            }
            None => {
                start.get_or_insert(i);
            }
        }
    }
    if let Some(s) = start {
        tokens.push(&info[s..]);
    }
    tokens
}

/// Leo's name for a fence's language: `python` for `python` or `py`,
/// `cplusplus` for `cpp`, `shell` for `sh`.
fn leo_language(word: &str) -> String {
    // Extensions first: Leo's tables also list `cpp` as a language, but
    // leo-rs's colouring and servers know C++ as `cplusplus`.
    let word = word.to_lowercase();
    // In a fence, `r` is R; as a file extension Leo reads it as REBOL.
    if word == "r" {
        return word;
    }
    match leolib::langdata::extension_dict().get(word.as_str()) {
        Some(lang) => lang.to_string(),
        None => word,
    }
}

fn unquote(s: &str) -> String {
    s.trim_matches(|c| c == '"' || c == '\'').to_string()
}

#[doc(hidden)]
pub fn set(o: &mut Outline, v: VnodeId, key: &str, value: &str) {
    o.node_mut(v)
        .uas
        .insert(key.to_string(), Ua::Text(value.to_string()));
}

#[doc(hidden)]
pub fn get<'a>(o: &'a Outline, v: VnodeId, key: &str) -> Option<&'a str> {
    match o.node(v).uas.get(key)? {
        Ua::Text(s) => Some(s),
        Ua::Opaque(_) => None,
    }
}

/// A block name two or more fence nodes in the tree at p share, and how
/// many: each block has one fence, as each Leo section has one definition.
fn duplicate_name(o: &Outline, p: &Position) -> Option<(String, usize)> {
    let mut seen: Vec<(String, usize)> = Vec::new();
    for q in p.self_and_subtree(o) {
        if headline_name(q.h(o)).is_none() || get(o, q.v, HEADING).is_some() {
            continue;
        }
        let name = headline_name(q.h(o)).unwrap_or_default().to_string();
        match seen.iter_mut().find(|(n, _)| *n == name) {
            Some((_, count)) => *count += 1,
            None => seen.push((name, 1)),
        }
    }
    seen.into_iter().find(|(_, n)| *n > 1)
}

/// The file the tree at p writes. A fence's code is what `code` gives for
/// its node, or else the node's body. Errors name p's kind.
pub fn write_tree(o: &Outline, p: &Position, code: &Code) -> Result<String> {
    // A refused read left the whole file in the body: write it as it is.
    if get(o, p.v, REFUSED).is_some() && o.node(p.v).children.is_empty() {
        return Ok(o.node(p.v).b.clone());
    }
    let kind = p.h(o).split_whitespace().next().unwrap_or_default();
    let named = |e: Error| match e {
        Error::Write { detail } => Error::Write {
            detail: format!("{kind}, {detail}"),
        },
        e => e,
    };
    if let Some((name, n)) = duplicate_name(o, p) {
        return Err(named(write_error(
            o,
            p.v,
            &format!("block `{name}` is defined by {n} fences; give each a name of its own"),
        )));
    }
    let mut out = String::new();
    write_node(o, p.v, 0, 0, code, &mut out).map_err(named)?;
    Ok(out)
}

/// The tree at v, `depth` below the root node, under a heading of
/// `parent_level`.
fn write_node(
    o: &Outline,
    v: VnodeId,
    depth: usize,
    parent_level: usize,
    code: &Code,
    out: &mut String,
) -> Result<()> {
    let (level, headings) = write_own(o, v, depth, parent_level, code, out)?;
    for h in headings {
        write_node(o, h, depth + 1, level, code, out)?;
    }
    Ok(())
}

/// The level of the heading at v, `depth` below the root node and
/// under a heading of `parent_level`: as read, moved by as many levels as
/// the node moved in depth; one below its parent for a heading added here.
fn level_of(o: &Outline, v: VnodeId, depth: usize, parent_level: usize) -> usize {
    let read = |key| get(o, v, key).and_then(|n: &str| n.parse::<usize>().ok());
    let level = match (read(HEADING_LEVEL), read(HEADING_DEPTH)) {
        (Some(level), Some(was)) => (level + depth).saturating_sub(was),
        _ => parent_level + 1,
    };
    level.clamp(1, 6)
}

/// One node's own text: its heading, and its body with its fences' code put
/// back. Returns its level and its child headings, for the caller to write.
fn write_own(
    o: &Outline,
    v: VnodeId,
    depth: usize,
    parent_level: usize,
    code: &Code,
    out: &mut String,
) -> Result<(usize, Vec<VnodeId>)> {
    let node = o.node(v);
    if depth > 0 && node.h.trim().is_empty() {
        return Err(write_error(o, v, "a heading node needs a headline"));
    }
    let level = match depth {
        0 => 0,
        _ => {
            let level = level_of(o, v, depth, parent_level);
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
                // A fence holds one body. Children, which a clone of the cell
                // can gain in an `@clean` tree, would be left out of it.
                if !o.node(f).children.is_empty() {
                    return Err(write_error(
                        o,
                        v,
                        &format!("{name} has children; a fence holds only its own body"),
                    ));
                }
                // A code line that closes the fence would end the cell there.
                let marker = FENCE_OPEN
                    .captures(lines[i])
                    .map_or("```".into(), |m| m[2].to_string());
                let supplied = code(f);
                let body = supplied.as_deref().unwrap_or(&o.node(f).b);
                if body.lines().any(|l| closes(l, &marker)) {
                    return Err(write_error(
                        o,
                        v,
                        &format!(
                            "the code of {name} has a line that would close its fence, {marker}"
                        ),
                    ));
                }
                // The closing fence, if there is one, must start its own line.
                fence_code(o, f, i + 2 < lines.len(), code, out);
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
            &format!(
                "{} has no reference in its parent's body; put its fence lines and a `{}` line there, or type the cell in the body and refresh",
                o.node(f).h,
                o.node(f).h.trim()
            ),
        ));
    }
    Ok((level, headings))
}

/// A heading at `level`: as the file had it while its text and level are
/// unchanged, else written fresh in the same style. Underlining has only two
/// levels, so an underlined heading moved deeper becomes `###`. A node the
/// reader did not make is an ATX heading.
fn heading(o: &Outline, v: VnodeId, level: usize, out: &mut String) {
    // A body edited to end without a newline would join the heading to it.
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    let h = o.node(v).h.trim();
    let same_level = get(o, v, HEADING_LEVEL) == Some(level.to_string().as_str());
    if same_level && get(o, v, HEADING_TEXT) == Some(h) {
        if let Some(lines) = get(o, v, HEADING) {
            out.push_str(lines);
            return;
        }
    }
    // Underlined text must still read as a heading's: `- todo` would be a
    // list item and a rule.
    let underlined = matches!(get(o, v, HEADING_MARK), Some("=" | "-"))
        && !NOT_A_HEADING_TEXT.is_match(h)
        && !FENCE_OPEN.is_match(h)
        && !DIV_OPEN.is_match(h)
        && !DIV_CLOSE.is_match(h)
        && atx(h).is_none();
    match (underlined, level) {
        (true, 1 | 2) => {
            let mark = if level == 1 { "=" } else { "-" };
            out.push_str(&format!("{h}\n{}\n", mark.repeat(h.chars().count().max(3))));
        }
        _ => out.push_str(&format!("{} {h}\n", "#".repeat(level))),
    }
}

/// A fence node's code, with its indent put back, ending in a newline if a
/// closing fence follows.
fn fence_code(o: &Outline, f: VnodeId, closed: bool, code: &Code, out: &mut String) {
    let indent = get(o, f, INDENT).unwrap_or("");
    // The kind's code for it, as `@entangled` reads an `include=` file.
    let supplied = code(f);
    let body = supplied.as_ref().unwrap_or(&o.node(f).b);
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
        detail: format!("under \"{}\": {detail}", o.node(v).h),
    }
}

/// The name in a fence node's headline, `<< name >>`.
pub fn headline_name(h: &str) -> Option<&str> {
    let n = h.trim().strip_prefix("<< ")?.strip_suffix(" >>")?.trim();
    (!n.is_empty()).then_some(n)
}

/// The file the markdown tree at p writes.
///
/// Refuses rather than drop code: a reference with no fence node, or a fence
/// node no reference places, is an error.
pub fn write_string(o: &Outline, p: &Position) -> Result<String> {
    write_tree(o, p, &|_| None)
}

/// The markdown a heading node of a markdown kind's file
/// stands for, without its child headings: its heading line, and its body
/// with each fence's code in place of its `<< name >>` reference. None for a
/// node in no such tree, for a fence node, or for a tree the writer refuses.
pub fn node_markdown(o: &Outline, p: &Position) -> Option<String> {
    node_markdown_with(o, p, &|_| None)
}

/// [`node_markdown`], with a fence's code from `code` where it gives any.
pub fn node_markdown_with(o: &Outline, p: &Position, code: &Code) -> Option<String> {
    let root = markdown_root(o, p)?;
    if is_fence_node(o, p) {
        return None;
    }
    // The levels down the path from the root node to p.
    let mut path = p.self_and_parents(o);
    path.truncate(p.level() - root.level());
    let mut parent_level = 0;
    // p's ancestors below the root node, outermost first.
    for (k, q) in path.iter().skip(1).rev().enumerate() {
        parent_level = level_of(o, q.v, k + 1, parent_level);
    }
    let mut out = String::new();
    let depth = p.level() - root.level();
    write_own(o, p.v, depth, parent_level, code, &mut out).ok()?;
    Some(out)
}

/// The registered kind's node at or above p: the root a markdown kind
/// read p from.
pub fn markdown_root(o: &Outline, p: &Position) -> Option<Position> {
    p.self_and_parents(o)
        .into_iter()
        .find(|a| o.kinds().find(a.h(o)).is_some())
}

/// Whether p is a fence node: a `<< name >>` node under a markdown kind's
/// node that is not a heading.
pub fn is_fence_node(o: &Outline, p: &Position) -> bool {
    headline_name(p.h(o)).is_some()
        && get(o, p.v, HEADING).is_none()
        && p.parent(o).is_some_and(|q| markdown_root(o, &q).is_some())
}

/// A notebook kind: a file whose executable cells are nodes, read with one
/// tool's fence rules whatever the file's extension.
pub struct Cells {
    directive: &'static str,
    style: Style,
}

/// `@qmd`: a Quarto document. Cells are labelled by `#| label:`.
pub const QMD: Cells = Cells {
    directive: "@qmd",
    style: Style::Quarto,
};

/// `@rmd`: an R Markdown (knitr) document. Cells are labelled by the
/// chunk header, `{r setup}` or `label=`.
pub const RMD: Cells = Cells {
    directive: "@rmd",
    style: Style::Knitr,
};

impl leolib::ext::FileKind for Cells {
    fn directive(&self) -> &'static str {
        self.directive
    }
    fn read(&self, o: &mut Outline, p: &Position) -> Result<bool> {
        let path = o.full_path(p);
        for note in read(o, p, Policy::Cells, self.style)? {
            o.add_import_warning(p, note);
        }
        o.remember_read_path(p, &path);
        o.clear_dirty_in_tree(p);
        Ok(true)
    }
    fn write(&self, o: &Outline, p: &Position) -> Result<String> {
        write_string(o, p)
    }
    fn before_save(&self, o: &mut Outline, p: &Position) {
        save_cell_ids(o, p);
    }
    fn stores_body(&self) -> bool {
        false
    }
    fn read_first(&self) -> bool {
        true
    }
    fn plan_rename(
        &self,
        o: &Outline,
        p: &Position,
        headline: &str,
    ) -> Option<std::result::Result<leolib::ext::Rename, String>> {
        plan_rename(o, p, headline).transpose()
    }
    fn nodes_are_documents(&self) -> bool {
        true
    }
    fn node_markdown(&self, o: &Outline, p: &Position) -> Option<String> {
        node_markdown(o, p)
    }
}

/// The fence rules the markdown root at `root` was read with: its kind's,
/// for `@qmd` and `@rmd`, else its file's extension's, as `@entangled`
/// chooses them.
fn style_at(o: &Outline, root: &Position) -> Style {
    let h = root.h(o);
    [QMD, RMD]
        .into_iter()
        .find(|k| !leolib::node::find_at_file_name(h, &[k.directive]).is_empty())
        .map_or_else(|| Style::of(&o.full_path(root)), |k| k.style)
}

/// Set the labelled cells' gnxs the `.leo` file saves for the markdown root at p.
pub fn save_cell_ids(o: &mut Outline, p: &Position) {
    let ids = cell_ids(o, p);
    if ids.is_empty() && get(o, p.v, REFUSED).is_some() {
        return;
    }
    let uas = &mut o.node_mut(p.v).uas;
    match ids.is_empty() {
        true => uas.remove(CELL_IDS),
        false => uas.insert(CELL_IDS.to_string(), Ua::Text(ids)),
    };
}

/// Whether `name` is one the reader gives an unnamed cell, `python cell 3`.
fn is_unnamed_cell(name: &str) -> bool {
    let mut words = name.rsplitn(3, ' ');
    let n = words.next().unwrap_or_default();
    words.next() == Some("cell") && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())
}

/// The `name gnx` lines for the markdown root at p: its labelled fence nodes.
/// An unnamed cell's number changes when a cell is added, so it has no line.
fn cell_ids(o: &Outline, p: &Position) -> String {
    p.self_and_subtree(o)
        .iter()
        .filter(|q| q.v != p.v && get(o, q.v, HEADING).is_none())
        .filter_map(|q| Some((headline_name(q.h(o))?, q.gnx(o))))
        // An unnamed cell's number changes when a cell is added.
        .filter(|(name, _)| !is_unnamed_cell(name))
        .map(|(name, gnx)| format!("{name} {gnx}\n"))
        .collect()
}

/// Plan renaming the fence node p to the name in
/// `new_headline` (`<< name >>`, or the bare name). None if p is not a fence
/// node or the name is unchanged; an error says why the rename is refused.
pub fn plan_rename(
    o: &Outline,
    p: &Position,
    new_headline: &str,
) -> std::result::Result<Option<Rename>, String> {
    if !is_fence_node(o, p) {
        return Ok(None);
    }
    let old = headline_name(p.h(o)).expect("a fence node").to_string();
    let root = markdown_root(o, p).expect("a fence node");
    let style = style_at(o, &root);
    // An unnamed cell, `<< python cell 3 >>`: renaming it gives it a label.
    // A label may itself hold a space, `#| label: "a b"`, so ask the fence.
    let unnamed = fence_line(o, p).is_some_and(|line| {
        let code = util::split_lines(p.b(o));
        let code: Vec<&str> = code.iter().map(String::as_str).collect();
        let info = fence_opener_info(&line).unwrap_or_default();
        let f = fence_info(info, &code, style);
        ![f.label, f.chunk_label, f.name].contains(&Some(old.clone()))
    });
    let new = headline_name(new_headline)
        .unwrap_or(new_headline.trim())
        .to_string();
    if new == old {
        return Ok(None);
    }
    if new.is_empty() || new.contains(char::is_whitespace) || new.contains(['<', '>']) {
        return Err(format!(
            "a cell name has no spaces or angle brackets: {new:?}"
        ));
    }
    let taken = root
        .self_and_subtree(o)
        .iter()
        .any(|q| is_fence_node(o, q) && headline_name(q.h(o)) == Some(new.as_str()));
    if taken {
        return Err(format!("a block named `{new}` is already in this document"));
    }
    let mut edits: Vec<(Position, Option<String>, Option<String>)> = Vec::new();
    let mut fences = 0;
    // Every part of the block in its document: the name in its fence line,
    // or in a Quarto `#| label:` line, and the reference to it.
    for h in root.self_and_subtree(o) {
        if is_fence_node(o, &h) {
            continue;
        }
        let owned = util::split_lines(h.b(o));
        let mut body = String::new();
        let mut changed = false;
        let mut i = 0;
        while i < owned.len() {
            let line = &owned[i];
            let next = owned.get(i + 1);
            let named = next
                .and_then(|n| REFERENCE.captures(n))
                .map(|m| m[1].to_string());
            if FENCE_OPEN.is_match(line) && named.as_deref() == Some(&format!("<< {old} >>")) {
                let child = h
                    .children(o)
                    .into_iter()
                    .filter(|c| is_fence_node(o, c) && c.h(o).trim() == format!("<< {old} >>"))
                    .nth(
                        edits
                            .iter()
                            .filter(|(q, _, _)| q.parent(o).as_ref() == Some(&h))
                            .count(),
                    );
                let (renamed_line, child_body) = match (unnamed, style) {
                    (true, Style::Quarto) => (
                        line.clone(),
                        child
                            .as_ref()
                            .map(|c| format!("#| label: {new}\n{}", c.b(o))),
                    ),
                    (true, _) => (add_label(line, &new, style), None),
                    (false, _) => (
                        rename_in_info(line, &old, &new, style),
                        child
                            .as_ref()
                            .map(|c| rename_quarto_label(c.b(o), &old, &new)),
                    ),
                };
                let quarto = child_body
                    .as_ref()
                    .is_some_and(|b| Some(b.as_str()) != child.as_ref().map(|c| c.b(o)));
                if renamed_line == *line && !quarto {
                    return Err(format!(
                        "<< {old} >> has no #name: it is named by its file= or include= target; change that in the fence line instead"
                    ));
                }
                body.push_str(&renamed_line);
                let reference =
                    next.unwrap()
                        .replacen(&format!("<< {old} >>"), &format!("<< {new} >>"), 1);
                body.push_str(&reference);
                if let Some(c) = child {
                    let b = child_body.filter(|_| quarto);
                    edits.push((c, Some(format!("<< {new} >>")), b));
                }
                fences += 1;
                changed = true;
                i += 2;
                continue;
            }
            body.push_str(line);
            i += 1;
        }
        if changed {
            edits.push((h, None, Some(body)));
        }
    }
    Ok(Some(Rename {
        old,
        new,
        fences,
        references: 0,
        note: None,
        edits,
    }))
}

/// `line`, an unnamed cell's opening line, with `name` added as its label:
/// `{r name, echo=FALSE}` in R Markdown, `{python #name}` in markdown.
fn add_label(line: &str, name: &str, style: Style) -> String {
    static ENGINE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"^([ \t]*(?:`{3,}|~{3,})[ \t]*\{[ \t]*[A-Za-z][\w.+-]*)").unwrap()
    });
    let label = match style {
        Style::Knitr => format!(" {name}"),
        _ => format!(" #{name}"),
    };
    ENGINE
        .replacen(line, 1, format!("${{1}}{label}"))
        .to_string()
}

/// `line`, a fence's opening line, with `#old` or `label=old` renamed.
fn rename_in_info(line: &str, old: &str, new: &str, style: Style) -> String {
    let pattern = format!(
        r#"(^|[\s{{,])(#|label=["']?){}(["']?)(\s|[,}}]|$)"#,
        regex::escape(old)
    );
    let re = Regex::new(&pattern).expect("an escaped name");
    let renamed = re
        .replacen(line, 1, |c: &regex::Captures| {
            format!("{}{}{new}{}{}", &c[1], &c[2], &c[3], &c[4])
        })
        .to_string();
    if renamed != line || style != Style::Knitr {
        return renamed;
    }
    // knitr's positional label: `{r old, echo=FALSE}`.
    let pattern = format!(
        r#"(\{{[ \t]*[A-Za-z][\w.+-]*[ \t,]+["']?){}(["']?[ \t]*[,}}])"#,
        regex::escape(old)
    );
    let re = Regex::new(&pattern).expect("an escaped name");
    re.replacen(line, 1, |c: &regex::Captures| {
        format!("{}{new}{}", &c[1], &c[2])
    })
    .to_string()
}

/// A fence's code with a leading Quarto `#| label: old` renamed.
fn rename_quarto_label(code: &str, old: &str, new: &str) -> String {
    let mut out = String::new();
    let mut options = true;
    for line in util::split_lines(code) {
        let t = line.trim_start();
        options = options && t.starts_with("#|");
        let label = t.trim_start_matches("#|").trim_start();
        match options
            && label.strip_prefix("label:").map(|n| unquote(n.trim())) == Some(old.to_string())
        {
            true => {
                let at = line.find("label:").expect("a label line") + "label:".len();
                let (key, value) = line.split_at(at);
                out.push_str(key);
                out.push_str(&value.replacen(old, new, 1));
            }
            false => out.push_str(&line),
        }
    }
    out
}
