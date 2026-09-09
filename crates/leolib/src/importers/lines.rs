//! Line-oriented importers: org, otl, markdown and treepad.
//!
//! These four formats spell their own outline structure -- `*` levels, tab
//! depth, `#` headings, `<node>` records -- so there are no blocks to find.
//! Each also needs its own writer, because the structure lines are consumed
//! on read and must be put back on write.

use once_cell::sync::Lazy;
use regex::Regex;

use crate::importers::block::Importer;
use crate::importers::LineImporter;
use crate::node::VnodeId;
use crate::outline::Outline;
use crate::position::Position;
use crate::util;

pub fn gen_block(im: &mut Importer, kind: LineImporter) {
    match kind {
        LineImporter::Org => org(im),
        LineImporter::Otl => otl(im),
        LineImporter::Markdown => markdown(im),
        LineImporter::Treepad => treepad(im),
    }
}

/// A stack of nodes by level, with the bodies being collected for each.
struct Builder {
    stack: Vec<VnodeId>,
    bodies: Vec<(VnodeId, Vec<String>)>,
}

impl Builder {
    fn new(root: VnodeId) -> Self {
        Self {
            stack: vec![root],
            bodies: vec![(root, Vec::new())],
        }
    }

    fn push_line(&mut self, line: String) {
        let top = *self.stack.last().unwrap();
        self.body_of(top).push(line);
    }

    fn body_of(&mut self, v: VnodeId) -> &mut Vec<String> {
        if let Some(i) = self.bodies.iter().position(|(w, _)| *w == v) {
            return &mut self.bodies[i].1;
        }
        self.bodies.push((v, Vec::new()));
        let n = self.bodies.len() - 1;
        &mut self.bodies[n].1
    }

    /// Add nodes so the stack reaches `level`, as `i.create_placeholders`.
    fn create_placeholders(&mut self, level: usize, o: &mut Outline) {
        while self.stack.len() < level {
            let parent = *self.stack.last().unwrap();
            let child = o.new_child_vnode(parent);
            o.node_mut(child).h = format!("placeholder level {}", self.stack.len());
            self.stack.push(child);
            self.body_of(child);
        }
    }

    fn add_node(&mut self, level: usize, headline: &str, o: &mut Outline) -> VnodeId {
        self.stack.truncate(level);
        self.create_placeholders(level, o);
        let parent = *self.stack.last().unwrap();
        let child = o.new_child_vnode(parent);
        o.node_mut(child).h = headline.to_string();
        self.stack.push(child);
        self.body_of(child);
        child
    }

    fn finish(self, o: &mut Outline) {
        for (v, lines) in self.bodies {
            o.node_mut(v).b = lines.concat();
        }
    }
}

// --- org ----------------------------------------------------------------

static ORG_SECTION: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(\*+)\s(.*)").unwrap());

fn org(im: &mut Importer) {
    let mut b = Builder::new(im.root.v);
    for line in im.lines.clone() {
        match ORG_SECTION.captures(&line) {
            Some(m) => {
                let level = m[1].len();
                let headline = m[2].trim_end_matches('\n').to_string();
                b.add_node(level, &headline, im.o);
            }
            None => b.push_line(line),
        }
    }
    b.finish(im.o);
}

// --- otl ----------------------------------------------------------------

static OTL_BODY: Lazy<Regex> = Lazy::new(|| Regex::new(r"^: (.*)$").unwrap());
static OTL_NODE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[ ]*(\t*)(.*)$").unwrap());

fn otl(im: &mut Importer) {
    let mut b = Builder::new(im.root.v);
    for line in im.lines.clone() {
        let line = line.trim_end_matches('\n').to_string();
        if line.trim().is_empty() {
            continue;
        }
        if let Some(m) = OTL_BODY.captures(&line) {
            b.push_line(format!("{}\n", &m[1]));
            continue;
        }
        if let Some(m) = OTL_NODE.captures(&line) {
            let level = 1 + m[1].len();
            let headline = m[2].to_string();
            b.add_node(level, &headline, im.o);
        }
    }
    b.finish(im.o);
}

// --- markdown -----------------------------------------------------------

static MD_HASH: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(#+)\s*(.+)\s*\n").unwrap());
static MD_NOHEADER: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\s*<!--\s*leo-noheader level=(\d+)\s+headline=(.*?)\s*-->\s*\n?$").unwrap()
});
static MD_UNDERLINE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(=+|-+)\n").unwrap());
static MD_PLACEHOLDER: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^placeholder level [0-9]+").unwrap());

fn is_hash(line: &str) -> Option<(usize, String)> {
    let m = MD_HASH.captures(line)?;
    let level = m[1].len();
    let name = m[2].trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some((level, name))
    }
}

fn is_underline(line: &str) -> bool {
    MD_UNDERLINE
        .captures(line)
        .map(|m| m[1].len() >= 4)
        .unwrap_or(false)
}

fn markdown(im: &mut Importer) {
    let lines = im.lines.clone();
    let mut b = Builder::new(im.root.v);
    let mut in_code = false;
    let mut skip = 0usize;
    for (i, line) in lines.iter().enumerate() {
        if skip > 0 {
            skip -= 1;
            continue;
        }
        if !in_code {
            if let Some(m) = MD_NOHEADER.captures(line) {
                let level: usize = m[1].parse().unwrap_or(1);
                let name = percent_decode(&m[2]);
                let v = b.add_node(level, &name, im.o);
                b.body_of(v).push("@noheader\n".to_string());
                continue;
            }
            // A heading underlined with '===' or '---'.
            let underlined = i + 1 < lines.len()
                && !is_underline(line)
                && !line.trim().is_empty()
                && is_underline(&lines[i + 1])
                && lines[i + 1].len() >= 4;
            if underlined {
                let level = if lines[i + 1].starts_with('=') { 1 } else { 2 };
                let name = line.trim_end_matches('\n').to_string();
                b.add_node(level, &name, im.o);
                skip = 1;
                continue;
            }
            if let Some((level, name)) = is_hash(line) {
                b.add_node(level, &name, im.o);
                continue;
            }
        }
        if i == 0 {
            // Text before any heading becomes its own node, as in Leo.
            let v = b.add_node(1, "!Declarations", im.o);
            b.body_of(v).push(line.clone());
            continue;
        }
        if line.starts_with("```") {
            in_code = !in_code;
        }
        b.push_line(line.clone());
    }
    b.finish(im.o);
}

/// Decode `%xx` escapes, as the markdown writer's `urllib.parse.quote` makes.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(v) = util::unhexlify(&s[i + 1..i + 3]) {
                out.push(v[0]);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        let c = *b as char;
        if c.is_ascii_alphanumeric() || "_.-~".contains(c) {
            out.push(c);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

// --- treepad ------------------------------------------------------------

fn treepad(im: &mut Importer) {
    let lines = im.lines.clone();
    let mut b = Builder::new(im.root.v);
    let mut i = 0usize;
    // The first line is the file's version header.
    if lines.first().map(|s| s.starts_with("<Treepad")) == Some(true) {
        i = 1;
    }
    while i < lines.len() {
        if lines[i].trim() != "dt=Text" {
            b.push_line(lines[i].clone());
            i += 1;
            continue;
        }
        // dt=Text / <node> / headline / level / body... / <end node> ...
        let headline = lines.get(i + 2).cloned().unwrap_or_default();
        let level: usize = lines
            .get(i + 3)
            .map(|s| s.trim().parse().unwrap_or(0))
            .unwrap_or(0);
        i += 4;
        let mut body = Vec::new();
        while i < lines.len() && !lines[i].starts_with("<end node>") {
            body.push(lines[i].clone());
            i += 1;
        }
        i += 1; // The <end node> line.
        let headline = headline.trim_end_matches('\n').to_string();
        let v = if level == 0 {
            im.root.v // The root record names the @auto node itself.
        } else {
            b.add_node(level, &headline, im.o)
        };
        b.body_of(v).extend(body);
    }
    b.finish(im.o);
}

// --- Writing ------------------------------------------------------------

/// Write an outline back to one of the four line-oriented formats.
pub fn write(o: &Outline, root: &Position, kind: LineImporter) -> String {
    match kind {
        LineImporter::Org => write_org(o, root),
        LineImporter::Otl => write_otl(o, root),
        LineImporter::Markdown => write_markdown(o, root),
        LineImporter::Treepad => write_treepad(o, root),
    }
}

/// The root's body, minus the `@language` and `@tabwidth` lines the importer added.
fn root_body(o: &Outline, root: &Position) -> String {
    util::split_lines(root.b(o))
        .into_iter()
        .filter(|line| !is_directive(line))
        .collect::<Vec<_>>()
        .concat()
}

/// True if the line starts with a Leo directive, as `g.isDirective`.
fn is_directive(s: &str) -> bool {
    static PAT: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*@([\w-]+)\s*").unwrap());
    let Some(m) = PAT.captures(s) else {
        return false;
    };
    let tail = &s[m.get(1).unwrap().end()..];
    if tail.starts_with('.') || tail.starts_with('(') {
        return false;
    }
    crate::importers::is_global_directive(&m[1])
}

/// One line, with exactly one newline after it.
fn put(out: &mut String, s: &str) {
    out.push_str(s.strip_suffix('\n').unwrap_or(s));
    out.push('\n');
}

fn write_org(o: &Outline, root: &Position) -> String {
    let mut out = root_body(o, root);
    let root_level = root.level();
    for p in root.subtree(o) {
        let stars = "*".repeat(p.level() - root_level);
        put(&mut out, &format!("{stars} {}", p.h(o)));
        for line in util::split_lines_no_ends(p.b(o)) {
            put(&mut out, line);
        }
    }
    out
}

fn write_otl(o: &Outline, root: &Position) -> String {
    let mut out = root_body(o, root);
    for child in root.children(o) {
        let n = child.level();
        for p in child.self_and_subtree(o) {
            let indent = "\t".repeat(p.level() - n);
            put(&mut out, &format!("{indent}{}", p.h(o)));
            for line in util::split_lines_no_ends(p.b(o)) {
                put(&mut out, &format!("{indent}: {line}"));
            }
        }
    }
    out
}

fn write_markdown(o: &Outline, root: &Position) -> String {
    let mut out = root_body(o, root);
    let root_level = root.level();
    for p in root.subtree(o) {
        let h = p.h(o);
        if MD_PLACEHOLDER.is_match(h) {
            continue;
        }
        let level = p.level() - root_level;
        let noheader = has_noheader(o, &p);
        if noheader {
            put(
                &mut out,
                &format!(
                    "<!-- leo-noheader level={level} headline={} -->",
                    percent_encode(h)
                ),
            );
        } else if h != "!Declarations" {
            put(
                &mut out,
                &format!("{} {}", "#".repeat(level), h.trim_start()),
            );
        }
        for line in util::split_lines_no_ends(p.b(o)) {
            if !is_directive(line) {
                put(&mut out, line);
            }
        }
    }
    out
}

/// True if p carries a local `@noheader` directive.
fn has_noheader(o: &Outline, p: &Position) -> bool {
    std::iter::once(p.h(o).to_string())
        .chain(util::split_lines(p.b(o)))
        .any(|line| {
            static PAT: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\s*@([\w-]+)\s*").unwrap());
            PAT.captures(&line)
                .map(|m| &m[1] == "noheader")
                .unwrap_or(false)
        })
}

fn write_treepad(o: &Outline, root: &Position) -> String {
    let mut out = String::new();
    put(&mut out, "<Treepad version 3.0>");
    let root_level = root.level();
    for p in root.self_and_subtree(o) {
        let h = if p.v == root.v { "Root" } else { p.h(o) };
        let indent = p.level() - root_level;
        put(&mut out, "dt=Text");
        put(&mut out, "<node>");
        put(&mut out, h);
        put(&mut out, &indent.to_string());
        for line in util::split_lines(p.b(o)) {
            if !is_directive(&line) {
                put(&mut out, &line);
            }
        }
        put(&mut out, "<end node> 5P9i0s8y19Z");
    }
    out
}
