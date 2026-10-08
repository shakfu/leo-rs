//! `@entangled <path>`: a markdown file whose named code fences are nodes.
//!
//! A leo-rs kind with no Leo counterpart; the design is in
//! `docs/dev/entangled_leo_backend.md`. The tree is `leo-markdown`'s; this
//! adds what is entangled's: which fences are named, `include=` fences
//! filled from their files, and entangled's references renamed with a
//! block. With the `leoapp` feature, [`app`] adds `:entangled-tangle` and
//! `:entangled-check`.

use once_cell::sync::Lazy;
use regex::Regex;

use leo_markdown::{get, headline_name, is_fence_node, Policy, Style, FENCE_OPEN};
use leolib::ext::{FileKind, Rename};
use leolib::{util, Outline, Position, Result};

#[cfg(feature = "leoapp")]
pub mod app;

/// The headline directive.
pub const DIRECTIVE: &str = "@entangled";

/// The `@entangled` kind, for `leolib::ext::Kinds`.
pub struct Entangled;

impl FileKind for Entangled {
    fn directive(&self) -> &'static str {
        DIRECTIVE
    }
    fn read(&self, o: &mut Outline, p: &Position) -> Result<bool> {
        read_one_at_entangled_node(o, p)
    }
    fn write(&self, o: &Outline, p: &Position) -> Result<String> {
        write_string(o, p)
    }
    fn before_save(&self, o: &mut Outline, p: &Position) {
        leo_markdown::save_cell_ids(o, p);
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
    ) -> Option<std::result::Result<Rename, String>> {
        plan_rename(o, p, headline).transpose()
    }
    fn nodes_are_documents(&self) -> bool {
        true
    }
    fn read_only(&self, o: &Outline, p: &Position) -> Option<String> {
        let target = included_from(o, p)?;
        Some(format!(
            "read-only: filled from include={target}; edit that file instead"
        ))
    }
    fn node_markdown(&self, o: &Outline, p: &Position) -> Option<String> {
        node_markdown(o, p)
    }
}

/// An `include=` fence's target, `path` or `path#anchor`: the file its code
/// comes from, which is the source, so the node is read-only.
const INCLUDE: &str = "leo-rs-entangled-include";

/// The `@entangled` file the tree at p writes.
///
/// An `include=` fence is written from its file, read now, so the markdown
/// matches the files at the moment it is saved.
pub fn write_string(o: &Outline, p: &Position) -> Result<String> {
    let dir = dir_of(&o.full_path(p));
    leo_markdown::write_tree(o, p, &|f| included(o, f, &dir))
}

/// A heading's markdown, as [`leo_markdown::node_markdown`] gives it, with
/// `include=` fences from their files.
pub fn node_markdown(o: &Outline, p: &Position) -> Option<String> {
    let root = leo_markdown::markdown_root(o, p)?;
    let dir = dir_of(&o.full_path(&root));
    leo_markdown::node_markdown_with(o, p, &|f| included(o, f, &dir))
}

/// The code of the `include=` fence whose node is f, read from its file now.
fn included(o: &Outline, f: leolib::node::VnodeId, dir: &std::path::Path) -> Option<String> {
    include_text(dir, get(o, f, INCLUDE)?).ok()
}

/// The directory of the markdown file at `path`: `include=` paths start there.
fn dir_of(path: &str) -> std::path::PathBuf {
    std::path::Path::new(path)
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default()
}

/// Mark each fence node under p whose fence says `include=`, in a `.md`
/// file: knitr and Quarto have an `include` option of their own, yes or no.
fn mark_includes(o: &mut Outline, p: &Position, style: Style) {
    if style != Style::Markdown {
        return;
    }
    for f in p.self_and_subtree(o) {
        if !is_fence_node(o, &f) {
            continue;
        }
        let Some(line) = leo_markdown::fence_line(o, &f) else {
            continue;
        };
        let Some(open) = FENCE_OPEN.captures(&line) else {
            continue;
        };
        let info = leo_markdown::fence_info(open[3].trim(), &[], style);
        if let Some(target) = info.include {
            leo_markdown::set(o, f.v, INCLUDE, &target);
        }
    }
}

/// The file an `include=` fence names, `path` or `path#anchor`, read from
/// `dir`: the whole file, or the lines between `ANCHOR: anchor` and
/// `ANCHOR_END: anchor`, without any anchor lines, as mdBook takes them.
pub fn include_text(dir: &std::path::Path, target: &str) -> std::result::Result<String, String> {
    let (file, anchor) = match target.split_once('#') {
        Some((f, a)) => (f, Some(a)),
        None => (target, None),
    };
    let path = dir.join(file);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("include={target}: {}: {e}", path.display()))?
        .replace('\r', "");
    let Some(anchor) = anchor else {
        return Ok(text);
    };
    let start =
        Regex::new(&format!(r"ANCHOR:\s*{}\b", regex::escape(anchor))).expect("an escaped anchor");
    let end = Regex::new(&format!(r"ANCHOR_END:\s*{}\b", regex::escape(anchor)))
        .expect("an escaped anchor");
    let lines = util::split_lines(&text);
    let Some(from) = lines.iter().position(|l| start.is_match(l)) else {
        return Err(format!(
            "include={target}: no `ANCHOR: {anchor}` in {}",
            path.display()
        ));
    };
    Ok(lines[from + 1..]
        .iter()
        .take_while(|l| !end.is_match(l))
        .filter(|l| !l.contains("ANCHOR:") && !l.contains("ANCHOR_END:"))
        .map(String::as_str)
        .collect())
}

/// Read the `@entangled` file at p into its tree, then fill its `include=`
/// fences from their files.
pub fn read_one_at_entangled_node(o: &mut Outline, p: &Position) -> Result<bool> {
    let path = o.full_path(p);
    // The file as read: `include=` fences are filled after its check.
    let style = Style::of(&path);
    let mut notes = leo_markdown::read(o, p, Policy::Named, style)?;
    mark_includes(o, p, style);
    let (refilled, missing) = fill_includes(o, p, &dir_of(&path));
    notes.extend(missing);
    if refilled > 0 {
        notes.push(format!(
            "{refilled} include= fence{} took new text from {}; saving writes it to the markdown",
            if refilled == 1 { "" } else { "s" },
            if refilled == 1 {
                "its file"
            } else {
                "their files"
            },
        ));
        o.keep_unsaved_after_read(p);
    }
    for note in notes {
        o.add_import_warning(p, note);
    }
    o.remember_read_path(p, &path);
    o.clear_dirty_in_tree(p);
    Ok(true)
}

/// Fill each `include=` fence node under p from its file. Returns how many
/// changed, and what could not be read.
fn fill_includes(o: &mut Outline, p: &Position, dir: &std::path::Path) -> (usize, Vec<String>) {
    let mut changed = 0;
    let mut missing = Vec::new();
    for q in p.self_and_subtree(o) {
        let Some(target) = get(o, q.v, INCLUDE).map(str::to_string) else {
            continue;
        };
        match include_text(dir, &target) {
            Ok(text) if text != o.node(q.v).b => {
                o.node_mut(q.v).b = text;
                changed += 1;
            }
            Ok(_) => {}
            Err(why) => missing.push(why),
        }
    }
    (changed, missing)
}

/// The target of p's `include=` fence, if p is one: such a node is
/// read-only, since its file is the source.
pub fn included_from<'a>(o: &'a Outline, p: &Position) -> Option<&'a str> {
    get(o, p.v, INCLUDE)
}

static REF_LINE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^([ \t]*<<)([^<>]+)(>>[ \t]*\n?)$").unwrap());

/// Plan renaming the block whose fence node p is to the name in
/// `new_headline`, as [`leo_markdown::plan_rename`] does, and entangled's
/// references to it, `<<name>>` and `<<doc.md#name>>`, in the code of every
/// `@entangled` document in the outline.
pub fn plan_rename(
    o: &Outline,
    p: &Position,
    new_headline: &str,
) -> std::result::Result<Option<Rename>, String> {
    let Some(mut plan) = leo_markdown::plan_rename(o, p, new_headline)? else {
        return Ok(None);
    };
    let root = leo_markdown::markdown_root(o, p).expect("a fence node");
    let (old, new) = (plan.old.clone(), plan.new.clone());
    let edits = &mut plan.edits;
    let entangled = is_entangled(root.h(o));
    let doc = leolib::node::find_at_file_name(root.h(o), &[DIRECTIVE]);
    let mut references = 0;
    for r in o
        .all_positions()
        .into_iter()
        .filter(|q| entangled && is_entangled(q.h(o)))
    {
        let here = r.v == root.v;
        let has_own = !here
            && r.self_and_subtree(o)
                .iter()
                .any(|q| is_fence_node(o, q) && headline_name(q.h(o)) == Some(old.as_str()));
        for f in r
            .self_and_subtree(o)
            .into_iter()
            .filter(|q| is_fence_node(o, q))
        {
            let start = edits.iter().position(|(q, _, _)| q.v == f.v);
            let current = start
                .and_then(|k| edits[k].2.clone())
                .unwrap_or_else(|| f.b(o).to_string());
            let mut body = String::new();
            let mut n = 0;
            for line in util::split_lines(&current) {
                match REF_LINE.captures(&line) {
                    Some(m) => {
                        let target = m[2].trim();
                        let qualified = format!("{doc}#{old}");
                        let renamed = if target == qualified {
                            Some(format!("{doc}#{new}"))
                        } else if target == old && (here || !has_own) {
                            Some(new.clone())
                        } else {
                            None
                        };
                        match renamed {
                            Some(t) => {
                                body.push_str(&format!("{}{t}{}", &m[1], &m[3]));
                                n += 1;
                            }
                            None => body.push_str(&line),
                        }
                    }
                    None => body.push_str(&line),
                }
            }
            if n > 0 {
                references += n;
                match start {
                    Some(k) => edits[k].2 = Some(body),
                    None => edits.push((f, None, Some(body))),
                }
            }
        }
    }
    plan.references = references;
    // Another document's references can be checked only by entangled.
    plan.note = entangled.then(|| {
        format!(
            "{}, {}; documents outside the outline are not checked (:entangled-check)",
            plural(plan.fences, "fence"),
            plural(references, "reference"),
        )
    });
    Ok(Some(plan))
}

/// Whether headline `h` is an `@entangled` node's.
pub fn is_entangled(h: &str) -> bool {
    !leolib::node::find_at_file_name(h, &[DIRECTIVE]).is_empty()
}

/// `n thing` or `n things`.
fn plural(n: usize, thing: &str) -> String {
    match n {
        1 => format!("1 {thing}"),
        _ => format!("{n} {thing}s"),
    }
}
