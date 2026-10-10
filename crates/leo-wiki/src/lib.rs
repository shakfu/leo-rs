//! `@wiki`: a subtree of markdown pages linked by `[[...]]`, exported to one
//! markdown file. A node headlined `@wiki NAME` is a wiki's root, and its
//! descendants are its pages. The design is in `docs/dev/wiki.md`.
//!
//! [`Wiki`] is the leolib tree kind: pages are markdown, and renaming a page
//! rewrites the links to it. With the `leoapp` feature, [`app::WikiPlugin`]
//! follows links, completes `[[`, keeps the rules and exports.

use std::collections::HashMap;
use std::ops::Range;

use leolib::ext::{Rename, TreeKind};
use leolib::{Outline, Position};

#[cfg(feature = "leoapp")]
pub mod app;

/// The headline directive.
pub const DIRECTIVE: &str = "@wiki";

/// The deepest heading markdown has; a page deeper than this is refused at
/// export.
const MAX_DEPTH: usize = 6;

/// The `@wiki` tree kind.
pub struct Wiki;

impl TreeKind for Wiki {
    fn directive(&self) -> &'static str {
        DIRECTIVE
    }
    fn language(&self) -> Option<&'static str> {
        Some("md")
    }
    fn plan_rename(
        &self,
        o: &Outline,
        root: &Position,
        p: &Position,
        headline: &str,
    ) -> Option<Result<Rename, String>> {
        plan_rename(o, root, p, headline)
    }
}

/// The name in a wiki root's headline, if `h` is one: `notes` in
/// `@wiki notes`.
pub fn wiki_name(h: &str) -> Option<&str> {
    let rest = h.trim_start().strip_prefix(DIRECTIVE)?;
    match rest.is_empty() || rest.starts_with(char::is_whitespace) {
        true => Some(rest.trim()),
        false => None,
    }
}

/// Every wiki root, in outline order.
pub fn roots(o: &Outline) -> Vec<Position> {
    o.all_positions()
        .into_iter()
        .filter(|p| wiki_name(p.h(o)).is_some())
        .collect()
}

/// The wiki root at or above p.
pub fn root_of(o: &Outline, p: &Position) -> Option<Position> {
    p.self_and_parents(o)
        .into_iter()
        .find(|q| wiki_name(q.h(o)).is_some())
}

/// The root of the wiki named `name`.
fn root_named(o: &Outline, name: &str) -> Option<Position> {
    roots(o)
        .into_iter()
        .find(|r| wiki_name(r.h(o)) == Some(name))
}

// --- Links -----------------------------------------------------------------

/// A `[[...]]` link in a page body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    /// Bytes of the body, the brackets included.
    pub range: Range<usize>,
    /// What precedes the first `:`, `other` in `[[other:Page]]`: a wiki's
    /// name if a wiki has it, else part of the path.
    pub wiki: Option<String>,
    /// The path after the wiki name; empty for `[[other:]]`.
    pub path: Vec<String>,
    /// The path read with no wiki name, `:` and all.
    pub whole: Vec<String>,
    /// The text after `|`.
    pub label: Option<String>,
}

impl Link {
    /// The wiki it names and its path there, given the wikis there are.
    fn target<'a>(&'a self, o: &Outline, here: &Position) -> (Option<Position>, &'a [String]) {
        match self.wiki.as_deref().and_then(|w| root_named(o, w)) {
            Some(r) => (Some(r), &self.path),
            None => (Some(here.clone()), &self.whole),
        }
    }

    /// The text a reader sees: the label, else the page's headline, else,
    /// for `[[other:]]`, the wiki's name. `cross` says whether the link's
    /// prefix names a wiki, so its path is what follows the colon.
    fn text(&self, cross: bool) -> String {
        let path = match cross {
            true => &self.path,
            false => &self.whole,
        };
        self.label
            .clone()
            .or_else(|| path.last().cloned())
            .or_else(|| self.wiki.clone())
            .unwrap_or_default()
    }
}

/// `s` with what has a meaning in a link escaped: `\`, `/`, `|`, `]`.
pub fn escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if matches!(c, '\\' | '/' | '|' | ']') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The link's text again, with `last` as the page's headline.
fn spell(link: &Link, wiki: Option<&str>, last: Option<&str>) -> String {
    let mut path: Vec<String> = match link.wiki.is_some() && wiki.is_some() {
        true => link.path.clone(),
        false => link.whole.clone(),
    };
    if let (Some(last), Some(slot)) = (last, path.last_mut()) {
        *slot = last.to_string();
    }
    let mut out = String::from("[[");
    if let Some(w) = wiki {
        out.push_str(w);
        out.push(':');
    }
    out.push_str(&path.iter().map(|s| escape(s)).collect::<Vec<_>>().join("/"));
    if let Some(label) = &link.label {
        out.push('|');
        out.push_str(&escape(label));
    }
    out.push_str("]]");
    out
}

/// The links in `body`, outside fenced code and code spans.
pub fn links(body: &str) -> Vec<Link> {
    let mut out = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let lead = line.len() - line.trim_start_matches(' ').len();
        let rest = &line[lead..];
        let marker = rest.chars().next().filter(|c| *c == '`' || *c == '~');
        if let (true, Some(c)) = (lead <= 3, marker) {
            let n = rest.chars().take_while(|x| *x == c).count();
            if n >= 3 {
                match fence {
                    None => {
                        fence = Some((c, n));
                        continue;
                    }
                    Some((fc, fnn)) if fc == c && n >= fnn && rest[n..].trim().is_empty() => {
                        fence = None;
                        continue;
                    }
                    _ => {}
                }
            }
        }
        if fence.is_none() {
            links_in_line(line, start, &mut out);
        }
    }
    out
}

/// The links in one line, at byte `start` of the body, skipping code spans.
fn links_in_line(line: &str, start: usize, out: &mut Vec<Link>) {
    let b = line.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'`' {
            let n = b[i..].iter().take_while(|&&c| c == b'`').count();
            let close = "`".repeat(n);
            // A span ends at the same run of backticks; unclosed, they are text.
            match line[i + n..].find(&close) {
                Some(k) => i += n + k + n,
                None => i += n,
            }
            continue;
        }
        if b[i..].starts_with(b"[[") {
            if let Some(end) = link_end(line, i + 2) {
                let inner = &line[i + 2..end];
                out.push(parse(inner, start + i..start + end + 2));
                i = end + 2;
                continue;
            }
        }
        i += 1;
    }
}

/// Where the `]]` closing a link opened before `from` is, skipping escapes.
fn link_end(line: &str, from: usize) -> Option<usize> {
    let b = line.as_bytes();
    let mut i = from;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'\n' => return None,
            b']' if b.get(i + 1) == Some(&b']') => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// `[[inner]]` read: its wiki, path and label.
fn parse(inner: &str, range: Range<usize>) -> Link {
    // Split at unescaped separators, keeping each piece's own escapes.
    let split = |s: &str, sep: char| -> Vec<String> {
        let mut parts = vec![String::new()];
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    parts.last_mut().unwrap().push('\\');
                    if let Some(n) = chars.next() {
                        parts.last_mut().unwrap().push(n);
                    }
                }
                c if c == sep => parts.push(String::new()),
                c => parts.last_mut().unwrap().push(c),
            }
        }
        parts
    };
    let unescape = |s: &str| -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => out.extend(chars.next()),
                c => out.push(c),
            }
        }
        out.trim().to_string()
    };
    let path_of = |s: &str| -> Vec<String> {
        match s.trim().is_empty() {
            true => Vec::new(),
            false => split(s, '/').iter().map(|x| unescape(x)).collect(),
        }
    };
    let mut halves = split(inner, '|');
    let path_part = halves.remove(0);
    let label = (!halves.is_empty()).then(|| unescape(&halves.join("|")));
    let colon = split(&path_part, ':');
    let wiki = (colon.len() > 1 && !colon[0].contains('/')).then(|| unescape(&colon[0]));
    let path = match wiki {
        Some(_) => path_of(&colon[1..].join(":")),
        None => path_of(&path_part),
    };
    Link {
        range,
        wiki,
        path,
        whole: path_of(&path_part),
        label,
    }
}

/// The pages a link from the wiki rooted at `here` names: the page whose
/// headline, and its parents' headlines, end the link's path, as Leo's
/// `findUnl` matches. A path naming no page names the wiki's root.
pub fn resolve(o: &Outline, here: &Position, link: &Link) -> Vec<Position> {
    let (Some(root), path) = link.target(o, here) else {
        return Vec::new();
    };
    if path.is_empty() {
        return vec![root];
    }
    let mut found = Vec::new();
    for q in root.subtree(o) {
        if q == root {
            continue;
        }
        let chain: Vec<String> = q
            .self_and_parents(o)
            .into_iter()
            .take_while(|a| a.v != root.v)
            .map(|a| a.h(o).trim().to_string())
            .collect();
        let matches =
            chain.len() >= path.len() && path.iter().rev().zip(&chain).all(|(seg, h)| seg == h);
        if matches && !found.iter().any(|f: &Position| f.v == q.v) {
            found.push(q);
        }
    }
    found
}

// --- Rules -------------------------------------------------------------------

/// Whether `line` is a Leo directive: `@language md`, `@others`.
fn is_directive(line: &str) -> bool {
    let Some(rest) = line.strip_prefix('@') else {
        return false;
    };
    let word: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    leolib::importers::is_global_directive(&word)
}

/// The rules the wiki rooted at `root` breaks, one line each: no wiki
/// inside another, no page a clone, no page headline starting with `@`, no
/// directive in a page, and a name usable as a namespace and a file name.
pub fn check_wiki(o: &Outline, root: &Position) -> Vec<String> {
    let name = wiki_name(root.h(o)).unwrap_or_default();
    let mut out = Vec::new();
    if name.is_empty() {
        out.push("an @wiki needs a name: @wiki NAME".to_string());
    } else if name
        .chars()
        .any(|c| matches!(c, ':' | '/' | '\\') || c.is_control())
    {
        out.push(format!("wiki name `{name}` may not hold `:`, `/` or `\\`"));
    }
    if let Some(outer) = root.parents(o).iter().find(|a| wiki_name(a.h(o)).is_some()) {
        out.push(format!(
            "@wiki {name} is inside {}: one wiki may not hold another",
            outer.h(o)
        ));
    }
    for q in root.subtree(o).into_iter().filter(|q| *q != *root) {
        let h = q.h(o);
        if o.node(q.v).parents.len() > 1 {
            out.push(format!("page `{h}` of wiki {name} is a clone"));
        }
        if h.trim_start().starts_with('@') && wiki_name(h).is_none() {
            out.push(format!("page `{h}` of wiki {name} starts with @"));
        }
        if let Some(d) = q.b(o).lines().find(|l| is_directive(l)) {
            out.push(format!(
                "page `{h}` of wiki {name} has the directive {}; use a fenced block",
                d.split_whitespace().next().unwrap_or(d)
            ));
        }
    }
    out
}

/// Every wiki's broken rules, and names two wikis share.
pub fn check(o: &Outline) -> Vec<String> {
    let roots = roots(o);
    let mut out: Vec<String> = Vec::new();
    let mut names: HashMap<&str, usize> = HashMap::new();
    for r in &roots {
        for v in check_wiki(o, r) {
            if !out.contains(&v) {
                out.push(v);
            }
        }
        *names
            .entry(wiki_name(r.h(o)).unwrap_or_default())
            .or_default() += 1;
    }
    let mut twice: Vec<&&str> = names
        .iter()
        .filter(|(n, k)| **k > 1 && !n.is_empty())
        .map(|(n, _)| n)
        .collect();
    twice.sort();
    out.extend(
        twice
            .into_iter()
            .map(|n| format!("two wikis are named {n}")),
    );
    out
}

// --- Renaming ----------------------------------------------------------------

/// Renaming p to `headline`: a page's new headline goes into every link to
/// it, in its wiki and from others; a root's new name into every
/// `[[name:...]]`. None for a headline that does not change what links name.
pub fn plan_rename(
    o: &Outline,
    root: &Position,
    p: &Position,
    headline: &str,
) -> Option<Result<Rename, String>> {
    if p == root {
        let (old, new) = (wiki_name(root.h(o))?, wiki_name(headline)?);
        if old == new || old.is_empty() {
            return None;
        }
        if new.is_empty() || new.chars().any(|c| matches!(c, ':' | '/' | '\\')) {
            return Some(Err(format!("a wiki name has no `:`, `/` or `\\`: {new:?}")));
        }
        if root_named(o, new).is_some() {
            return Some(Err(format!(
                "a wiki named {new} is already in this outline"
            )));
        }
        let rewrite = |link: &Link| -> Option<String> {
            (link.wiki.as_deref() == Some(old)).then(|| spell(link, Some(new), None))
        };
        return Some(Ok(rename_plan(o, p, headline, old, new, rewrite)));
    }
    let (old, new) = (p.h(o).trim().to_string(), headline.trim().to_string());
    if old == new || headline.contains('\n') {
        return None;
    }
    let rewrite = |link: &Link, here: &Position| -> Option<String> {
        let targets = resolve(o, here, link);
        (targets.len() == 1 && targets[0].v == p.v).then(|| {
            let wiki = link.wiki.as_deref().filter(|w| root_named(o, w).is_some());
            spell(link, wiki, Some(&new))
        })
    };
    let mut plan = Rename {
        old: old.clone(),
        new: new.clone(),
        fences: 0,
        references: 0,
        note: None,
        edits: vec![(p.clone(), Some(headline.to_string()), None)],
    };
    for r in roots(o) {
        for q in r.self_and_subtree(o) {
            if let Some(body) = rewrite_body(q.b(o), |l| rewrite(l, &r), &mut plan.references) {
                plan.edits.push((q, None, Some(body)));
            }
        }
    }
    Some(Ok(plan))
}

/// The plan for a wiki root's rename: its headline, and every body's links
/// `rewrite` changes.
fn rename_plan(
    o: &Outline,
    root: &Position,
    headline: &str,
    old: &str,
    new: &str,
    rewrite: impl Fn(&Link) -> Option<String>,
) -> Rename {
    let mut plan = Rename {
        old: old.to_string(),
        new: new.to_string(),
        fences: 0,
        references: 0,
        note: None,
        edits: vec![(root.clone(), Some(headline.to_string()), None)],
    };
    for r in roots(o) {
        for q in r.self_and_subtree(o) {
            if let Some(body) = rewrite_body(q.b(o), &rewrite, &mut plan.references) {
                plan.edits.push((q, None, Some(body)));
            }
        }
    }
    plan
}

/// `body` with each link `rewrite` gives new text for replaced, counted in
/// `n`; None if none is.
fn rewrite_body(
    body: &str,
    rewrite: impl Fn(&Link) -> Option<String>,
    n: &mut usize,
) -> Option<String> {
    let mut out = String::new();
    let mut at = 0;
    for link in links(body) {
        if let Some(text) = rewrite(&link) {
            out.push_str(&body[at..link.range.start]);
            out.push_str(&text);
            at = link.range.end;
            *n += 1;
        }
    }
    (at > 0).then(|| out + &body[at..])
}

/// Page `p`'s `body` with each link that names one page as a markdown link
/// to it, `[text](unl:gnx://#GNX)`, for the rendered view to follow. A link
/// naming no page or several stays as typed. None if `p` is not in a wiki
/// or no link changed.
pub fn rendered(o: &Outline, p: &Position, body: &str) -> Option<String> {
    let root = root_of(o, p)?;
    let mut n = 0;
    rewrite_body(
        body,
        |link| match resolve(o, &root, link).as_slice() {
            [target] => {
                let cross = link
                    .wiki
                    .as_deref()
                    .is_some_and(|w| root_named(o, w).is_some());
                let text = link.text(cross).replace('[', "\\[").replace(']', "\\]");
                Some(format!("[{text}](unl:gnx://#{})", target.gnx(o)))
            }
            _ => None,
        },
        &mut n,
    )
}

// --- Export ------------------------------------------------------------------

/// A heading's anchor as GitHub makes it: lower case, punctuation dropped,
/// spaces as hyphens.
pub fn slug(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

/// An ATX heading's level and text, if `line` is one.
fn atx(line: &str) -> Option<(usize, &str)> {
    let t = line.trim_start_matches(' ');
    if line.len() - t.len() > 3 {
        return None;
    }
    let n = t.chars().take_while(|c| *c == '#').count();
    let rest = &t[n..];
    ((1..=MAX_DEPTH).contains(&n) && (rest.is_empty() || rest.starts_with([' ', '\t', '\n'])))
        .then(|| (n, rest.trim().trim_end_matches('#').trim()))
}

/// The lines of `body` outside fenced code, as (line, in a fence).
fn fenced(body: &str) -> Vec<(&str, bool)> {
    let mut fence: Option<(char, usize)> = None;
    body.split_inclusive('\n')
        .map(|line| {
            let t = line.trim_start_matches(' ');
            let c = t.chars().next().filter(|c| *c == '`' || *c == '~');
            let n = c.map_or(0, |c| t.chars().take_while(|x| *x == c).count());
            let opens_or_closes = n >= 3 && line.len() - t.len() <= 3;
            match (fence, opens_or_closes) {
                (None, true) => {
                    fence = Some((c.unwrap(), n));
                    (line, true)
                }
                (Some((fc, fnn)), true)
                    if c == Some(fc) && n >= fnn && t[n..].trim().is_empty() =>
                {
                    fence = None;
                    (line, true)
                }
                (Some(_), _) => (line, true),
                (None, false) => (line, false),
            }
        })
        .collect()
}

/// The anchor each page of the wiki rooted at `root` has in its export,
/// by vnode, counting every heading as GitHub does for duplicates.
fn anchors(o: &Outline, root: &Position) -> HashMap<leolib::VnodeId, String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut unique = |text: &str| {
        let base = slug(text);
        let n = seen.entry(base.clone()).or_default();
        let anchor = match *n {
            0 => base,
            k => format!("{base}-{k}"),
        };
        *n += 1;
        anchor
    };
    let mut out = HashMap::new();
    for q in root.self_and_subtree(o) {
        if q != *root {
            out.insert(q.v, unique(q.h(o).trim()));
        }
        for (line, code) in fenced(q.b(o)) {
            if let (false, Some((_, text))) = (code, atx(line)) {
                unique(text);
            }
        }
    }
    out
}

/// The file `export` writes for the wiki rooted at `root`: `NAME.md` in the
/// `@path` in effect there, else beside the `.leo` file.
pub fn export_path(o: &Outline, root: &Position) -> String {
    let name = wiki_name(root.h(o)).unwrap_or_default();
    let dir = o.get_path(root);
    std::path::Path::new(&dir)
        .join(format!("{name}.md"))
        .to_string_lossy()
        .to_string()
}

/// The markdown the wiki rooted at `root` exports: the root's body, then
/// each page under a heading of its depth, its own headings moved down by
/// as much, and each link as a markdown link. Refused, with every reason,
/// for a broken rule, a link that names no page or several, or a heading
/// deeper than six.
pub fn export(o: &Outline, root: &Position) -> Result<String, Vec<String>> {
    let mut problems = check_wiki(o, root);
    let name = wiki_name(root.h(o)).unwrap_or_default().to_string();
    let own = anchors(o, root);
    let mut others: HashMap<String, HashMap<leolib::VnodeId, String>> = HashMap::new();
    let mut out = String::new();
    for q in root.self_and_subtree(o) {
        let depth = q.level() - root.level();
        let h = q.h(o).trim().to_string();
        if depth > 0 {
            if depth > MAX_DEPTH {
                problems.push(format!("page `{h}` is {depth} levels deep; markdown has 6"));
            }
            if !out.is_empty() && !out.ends_with("\n\n") {
                out.push('\n');
            }
            out.push_str(&format!("{} {h}\n\n", "#".repeat(depth.min(MAX_DEPTH))));
        }
        let body = q.b(o);
        let links = links(body);
        let mut at = 0;
        let mut text = String::new();
        for link in &links {
            text.push_str(&body[at..link.range.start]);
            at = link.range.end;
            let spelled = &body[link.range.clone()];
            let targets = resolve(o, root, link);
            let target = match targets.len() {
                1 => &targets[0],
                0 => {
                    problems.push(format!("page `{h}`: {spelled} names no page"));
                    continue;
                }
                n => {
                    problems.push(format!(
                        "page `{h}`: {spelled} names {n} pages; add its parent, [[Parent/Page]]"
                    ));
                    continue;
                }
            };
            let wiki = root_of(o, target).expect("a page is in a wiki");
            let other = wiki_name(wiki.h(o)).unwrap_or_default().to_string();
            let anchor = match other == name {
                true => own.get(&target.v).cloned(),
                false => others
                    .entry(other.clone())
                    .or_insert_with(|| anchors(o, &wiki))
                    .get(&target.v)
                    .cloned(),
            };
            let href = match (other == name, anchor) {
                (true, Some(a)) => format!("#{a}"),
                (true, None) => "#".to_string(),
                (false, Some(a)) => format!("{other}.md#{a}"),
                (false, None) => format!("{other}.md"),
            };
            let cross = link
                .wiki
                .as_deref()
                .is_some_and(|w| root_named(o, w).is_some());
            text.push_str(&format!("[{}]({href})", link.text(cross)));
        }
        text.push_str(&body[at..]);
        // The body's own headings move down by the page's depth; directives
        // are the root's only, and not markdown.
        for (line, code) in fenced(&text) {
            match (code, atx(line)) {
                (false, Some((level, _))) if depth > 0 => {
                    if level + depth > MAX_DEPTH {
                        problems.push(format!(
                            "a heading in page `{h}` is {} deep; markdown has 6",
                            level + depth
                        ));
                    }
                    out.push_str(&"#".repeat(depth));
                    out.push_str(line);
                }
                (false, _) if depth == 0 && is_directive(line) => {}
                _ => out.push_str(line),
            }
        }
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
    }
    match problems.is_empty() {
        true => Ok(out),
        false => Err(problems),
    }
}

/// The edits that rewrite every link of the wiki rooted at `root` as a Leo
/// link, `` `unl:gnx://#GNX` ``, for an outline shared with Leo, which has no
/// `[[...]]`. Labels are dropped: Leo's links have none. Refused, with every
/// reason, for a link that names no page or several.
pub fn wikilinks_to_unls(
    o: &Outline,
    root: &Position,
) -> Result<Vec<(Position, String)>, Vec<String>> {
    let mut problems = Vec::new();
    let mut edits = Vec::new();
    for q in root.self_and_subtree(o) {
        let mut n = 0;
        let body = rewrite_body(
            q.b(o),
            |link| {
                let targets = resolve(o, root, link);
                (targets.len() == 1).then(|| format!("`unl:gnx://#{}`", targets[0].gnx(o)))
            },
            &mut n,
        );
        for link in links(q.b(o)) {
            let k = resolve(o, root, &link).len();
            if k != 1 {
                problems.push(format!(
                    "page `{}`: {} names {k} pages",
                    q.h(o),
                    &q.b(o)[link.range.clone()]
                ));
            }
        }
        if let Some(body) = body {
            edits.push((q, body));
        }
    }
    match problems.is_empty() {
        true => Ok(edits),
        false => Err(problems),
    }
}
