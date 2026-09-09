//! Untangling: rebuilding an outline from an external file's sentinels.
//!
//! A port of Leo's `FastAtRead`. The sentinel comments in an `@file` record
//! the whole tree -- gnx, level and headline for every node -- so the reader
//! is a line scanner with a stack, not a language parser. `@clean` files carry
//! no sentinels; see [`crate::atclean`].

use std::collections::HashMap;

use once_cell::sync::Lazy;
use regex::Regex;

use crate::node::VnodeId;
use crate::outline::Outline;
use crate::position::Position;
use crate::util;

/// Remove the leading `indent` bytes, which `@others` added, if they are all
/// whitespace. Anything else is the user's own text and stays.
fn strip_indent(line: &str, indent: usize) -> String {
    if indent == 0 || line.len() <= indent {
        return line.to_string();
    }
    match line.get(..indent) {
        Some(lead) if lead.trim().is_empty() => line[indent..].to_string(),
        _ => line.to_string(),
    }
}

/// The compiled patterns for one pair of comment delimiters.
///
/// Rebuilt whenever `@comment` or `@delims` changes the delimiters mid-file,
/// which is why this is a struct and not a set of statics.
struct Patterns {
    after: Regex,
    all: Regex,
    code: Regex,
    comment: Regex,
    delims: Regex,
    doc: Regex,
    first: Regex,
    last: Regex,
    node_start: Regex,
    others: Regex,
    ref_pat: Regex,
    section_delims: Regex,
}

impl Patterns {
    /// The `@+leo` sentinel determines the form of *all* sentinels in a file.
    /// None of these patterns may accept a space after `delim1`: doing so
    /// makes ordinary commented-out text read as sentinels.
    fn new(delim1: &str, delim2: &str) -> Self {
        let d1 = regex::escape(delim1);
        let d2 = regex::escape(delim2);
        // Python's `$` also matches before a trailing newline; Rust's does not,
        // and these patterns are matched against lines that keep their newline.
        let eol = r"\n?$";
        let compile = |s: String| Regex::new(&s).expect("bad sentinel pattern");
        Self {
            after: compile(format!(r"^\s*{d1}@afterref{d2}{eol}")),
            all: compile(format!(r"^(\s*){d1}@(\+|-)all\b(.*){d2}{eol}")),
            code: compile(format!(r"^\s*{d1}@@c(ode)?\b(.*){d2}{eol}")),
            comment: compile(format!(r"^\s*{d1}@@comment(.*){d2}")),
            delims: compile(format!(r"^\s*{d1}@delims(.*){d2}")),
            doc: compile(format!(r"^\s*{d1}@\+(at|doc)?(\s.*?)?{d2}\n")),
            first: compile(format!(r"^\s*{d1}@@first{d2}{eol}")),
            last: compile(format!(r"^\s*{d1}@@last{d2}{eol}")),
            node_start: compile(format!(
                r"^(\s*){d1}@\+node:([^:]+): \*(\d+)?(\*?) (.*){d2}{eol}"
            )),
            others: compile(format!(r"^(\s*){d1}@(\+|-)others\b(.*){d2}{eol}")),
            ref_pat: compile(format!(r"^(\s*){d1}@(\+|-)<<(.*)>>\s*{d2}{eol}")),
            section_delims: compile(format!(
                r"^\s*{d1}@@section-delims[ \t]+([^ \w\n\t]+)[ \t]+([^ \w\n\t]+)[ \t]*{d2}{eol}"
            )),
        }
    }

    /// The section-reference pattern, rebuilt for non-default section delims.
    fn set_section_delims(&mut self, d1: &str, d2: &str, delim1: &str, delim2: &str) {
        let c1 = regex::escape(delim1);
        let c2 = regex::escape(delim2);
        let s1 = regex::escape(d1);
        let s2 = regex::escape(d2);
        if let Ok(re) = Regex::new(&format!(r"^(\s*){c1}@(\+|-){s1}(.*){s2}\s*{c2}\n?$")) {
            self.ref_pat = re;
        }
    }
}

static HEADER_PATTERN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?s)^(.+)@\+leo(-ver=(\d+))?(-thin)?(-encoding=(.*)(\.))?(.*)$").unwrap()
});

/// The delimiters and the lines preceding the `@+leo` sentinel.
pub struct Header {
    pub delim1: String,
    pub delim2: String,
    pub first_lines: Vec<String>,
    pub start: usize,
}

/// Find the header line, which follows any `@first` lines.
pub fn scan_header(lines: &[String]) -> Option<Header> {
    let mut first_lines = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let probe = line.strip_suffix('\n').unwrap_or(line);
        if let Some(m) = HEADER_PATTERN.captures(probe) {
            return Some(Header {
                delim1: m.get(1).map(|x| x.as_str()).unwrap_or("").to_string(),
                delim2: m.get(8).map(|x| x.as_str()).unwrap_or("").to_string(),
                first_lines,
                start: i + 1,
            });
        }
        first_lines.push(line.clone());
    }
    None
}

/// Parse `contents` into a tree of vnodes anchored at `root`.
pub fn read_into_root(o: &mut Outline, contents: &str, path: &str, root: &Position) -> bool {
    let contents = contents.replace('\r', "");
    let lines = util::split_lines(&contents);
    let Some(header) = scan_header(&lines) else {
        return false;
    };
    // Detach the whole subtree first, so every link the scan makes is fresh.
    // Leo clears only the root's children, which leaves a re-read adding a
    // second parent link to every node below.
    o.detach_subtree(root.v);
    let mut scanner = Scanner::new(o, root, path);
    scanner.scan_lines(&header, &lines);
    true
}

struct Scanner<'a> {
    o: &'a mut Outline,
    root_v: VnodeId,
    path: String,
    pats: Patterns,
    warnings: Vec<String>,
}

impl<'a> Scanner<'a> {
    fn new(o: &'a mut Outline, root: &Position, path: &str) -> Self {
        Self {
            root_v: root.v,
            o,
            path: path.to_string(),
            pats: Patterns::new("#", ""),
            warnings: Vec::new(),
        }
    }

    fn scan_lines(&mut self, header: &Header, lines: &[String]) {
        let mut comment_delim1 = header.delim1.clone();
        let mut comment_delim2 = header.delim2.clone();
        self.pats = Patterns::new(&comment_delim1, &comment_delim2);

        let mut afterref = false;
        // The root of the clone tree being rescanned, if any.
        let mut clone_v: Option<VnodeId>;
        let mut doc_skip = [format!("{comment_delim1}\n"), format!("{comment_delim2}\n")];
        let mut first_i = 0usize;
        let mut in_doc = false;
        let mut is_cweb = comment_delim1 == "@q@" && comment_delim2 == "@>";
        let mut indent: usize = 0;
        let mut level_stack: Vec<(VnodeId, Option<VnodeId>)> = Vec::new();
        let mut n_last_lines = 0usize;
        let mut root_seen = false;
        let mut section_delim1 = "<<".to_string();
        let mut section_delim2 = ">>".to_string();
        let mut section_reference_seen = false;
        let mut sentinel = format!("{comment_delim1}@");
        let mut stack: Vec<(String, usize)> = Vec::new();
        let mut verbatim_line = format!("{comment_delim1}@verbatim{comment_delim2}");
        let mut verbatim = false;

        let root_v = self.root_v;
        let mut gnx = self.o.gnx(root_v).to_string();
        level_stack.push((root_v, None));
        let mut parent_v: VnodeId;

        let mut gnx2body: HashMap<String, Vec<String>> = HashMap::new();
        self.o.gnx_dict.insert(gnx.clone(), root_v);
        gnx2body.insert(gnx.clone(), header.first_lines.clone());

        let mut i = 0usize;
        let mut saw_at_leo = false;
        let body_lines = &lines[header.start.min(lines.len())..];
        for (idx, raw) in body_lines.iter().enumerate() {
            i = idx;
            let mut line = raw.clone();
            let strip_line = line.trim().to_string();

            if afterref {
                // The line after an @afterref sentinel joins the previous line.
                let body = gnx2body.entry(gnx.clone()).or_default();
                match body.last_mut() {
                    Some(last) => {
                        let joined = format!("{}{}", last.trim_end(), line);
                        *last = joined;
                    }
                    None => body.push(line.clone()),
                }
                afterref = false;
                continue;
            }
            if verbatim {
                // The previous line was a verbatim *sentinel*: take this one as is.
                line = strip_indent(&line, indent);
                gnx2body.entry(gnx.clone()).or_default().push(line);
                verbatim = false;
                continue;
            }
            if strip_line == verbatim_line {
                verbatim = true;
                continue;
            }

            // Undo the cweb hack, then strip the indentation @others added.
            if is_cweb && line.starts_with(&sentinel) {
                let head = line[..sentinel.len()].to_string();
                let tail = line[sentinel.len()..].replace("@@", "@");
                line = format!("{head}{tail}");
            }
            line = strip_indent(&line, indent);

            // Faster than a regex, and most lines are not sentinels.
            if !in_doc && !strip_line.starts_with(&sentinel) {
                gnx2body.entry(gnx.clone()).or_default().push(line);
                continue;
            }

            if let Some(m) = self.pats.others.captures(&line) {
                in_doc = false;
                if &m[2] == "+" {
                    let lead = m.get(1).unwrap().as_str();
                    let tail = m.get(3).map(|x| x.as_str()).unwrap_or("");
                    gnx2body
                        .entry(gnx.clone())
                        .or_default()
                        .push(format!("{lead}@others{tail}\n"));
                    stack.push((gnx.clone(), indent));
                    indent += m.get(1).unwrap().end();
                } else if let Some((g2, i2)) = stack.pop() {
                    gnx = g2;
                    indent = i2;
                }
                continue;
            }

            if let Some(m) = self.pats.ref_pat.captures(&line) {
                in_doc = false;
                if &m[2] == "+" {
                    // A later @section-delims directive would now be an error.
                    section_reference_seen = true;
                    let lead = m.get(1).unwrap().as_str();
                    let name = m.get(3).unwrap().as_str();
                    gnx2body
                        .entry(gnx.clone())
                        .or_default()
                        .push(format!("{lead}{section_delim1}{name}{section_delim2}\n"));
                    stack.push((gnx.clone(), indent));
                    indent += m.get(1).unwrap().end();
                } else if let Some((g2, i2)) = stack.pop() {
                    gnx = g2;
                    indent = i2;
                }
                continue;
            }

            if let Some(m) = self.pats.node_start.captures(&line) {
                in_doc = false;
                let new_gnx = m[2].to_string();
                let head = m[5].to_string();
                let level: usize = match m.get(3) {
                    Some(d) => d.as_str().parse().unwrap_or(1),
                    None => 1 + m.get(4).map(|x| x.as_str().len()).unwrap_or(0),
                };
                let existing = self.o.find_gnx(&new_gnx);

                // Case 1: the root @<file> node. Its headline stays as the
                // outline spells it; only the gnx comes from the file (#3931).
                if !root_seen {
                    root_seen = true;
                    let old_gnx = self.o.gnx(root_v).to_string();
                    if old_gnx != new_gnx {
                        gnx2body.remove(&old_gnx);
                        self.o.gnx_dict.remove(&old_gnx);
                        self.o.node_mut(root_v).gnx = new_gnx.clone();
                    }
                    self.o.gnx_dict.insert(new_gnx.clone(), root_v);
                    gnx = new_gnx;
                    gnx2body.insert(gnx.clone(), Vec::new());
                    self.o.node_mut(root_v).children.clear();
                    continue;
                }

                let (stack_parent, stack_clone) = *level_stack
                    .get(level.saturating_sub(2))
                    .or_else(|| level_stack.last())
                    .unwrap();
                parent_v = stack_parent;
                clone_v = stack_clone;

                // Case 2: inside the descendants of a clone. The last version
                // of the body and headline in the file wins.
                if let (Some(v), Some(_)) = (existing, clone_v) {
                    gnx = new_gnx;
                    gnx2body.insert(gnx.clone(), Vec::new());
                    self.o.node_mut(v).h = head;
                    level_stack.truncate(level.saturating_sub(1));
                    level_stack.push((v, clone_v));
                    self.o.node_mut(v).children.clear();
                    self.o.node_mut(parent_v).children.push(v);
                    self.o.node_mut(v).parents.push(parent_v);
                    continue;
                }

                // Case 3: a new node, or the start of a clone tree.
                let v = match existing {
                    Some(v) => {
                        clone_v = Some(v);
                        self.o.node_mut(v).children.clear();
                        v
                    }
                    None => self.o.new_vnode(Some(&new_gnx)),
                };
                gnx = new_gnx;
                gnx2body.insert(gnx.clone(), Vec::new());
                self.o.node_mut(v).h = head;
                level_stack.truncate(level.saturating_sub(1));
                level_stack.push((v, clone_v));
                self.o.node_mut(parent_v).children.push(v);
                self.o.node_mut(v).parents.push(parent_v);
                continue;
            }

            if in_doc {
                // A doc part written with block delims begins and ends with the
                // delimiter alone on a line; those two lines are not content.
                if !comment_delim2.is_empty() && doc_skip.contains(&line) {
                    continue;
                }
                if let Some(m) = self.pats.code.captures(&line) {
                    in_doc = false;
                    let text = if m.get(1).is_some() {
                        "@code\n"
                    } else {
                        "@c\n"
                    };
                    gnx2body
                        .entry(gnx.clone())
                        .or_default()
                        .push(text.to_string());
                    continue;
                }
            } else if let Some(m) = self.pats.doc.captures(&line) {
                let doc = if m.get(1).map(|x| x.as_str()) == Some("doc") {
                    "@doc"
                } else {
                    "@"
                };
                let doc2 = m.get(2).map(|x| x.as_str()).unwrap_or("");
                let text = if doc2.is_empty() {
                    format!("{doc}\n")
                } else {
                    format!("{doc}{doc2}\n")
                };
                gnx2body.entry(gnx.clone()).or_default().push(text);
                in_doc = true;
                continue;
            }

            if line.starts_with(&format!("{comment_delim1}@-leo")) {
                // The @-leo sentinel adds nothing to the text.
                i += 1;
                saw_at_leo = true;
                break;
            }

            if let Some(m) = self.pats.all.captures(&line) {
                // @all tells the *write* code not to check for undefined
                // sections. Here it is only text; the stack keeps in step.
                if &m[2] == "+" {
                    let lead = m.get(1).unwrap().as_str();
                    let tail = m.get(3).map(|x| x.as_str()).unwrap_or("");
                    gnx2body
                        .entry(gnx.clone())
                        .or_default()
                        .push(format!("{lead}@all{tail}\n"));
                    stack.push((gnx.clone(), indent));
                } else if let Some((g2, i2)) = stack.pop() {
                    gnx = g2;
                    indent = i2;
                }
                continue;
            }

            if self.pats.after.is_match(&line) {
                afterref = true;
                continue;
            }

            if self.pats.first.is_match(&line) {
                if first_i < header.first_lines.len() {
                    let text = format!("@first {}", header.first_lines[first_i]);
                    gnx2body.entry(gnx.clone()).or_default().push(text);
                    first_i += 1;
                } else {
                    self.warnings
                        .push(format!("too many @first lines: {}", self.path));
                }
                continue;
            }
            if self.pats.last.is_match(&line) {
                // The @last lines themselves follow the @-leo sentinel.
                n_last_lines += 1;
                continue;
            }

            if let Some(m) = self.pats.comment.captures(&line) {
                let delims = m[1].trim().to_string();
                // Whatever happens, keep the directive in the body.
                gnx2body
                    .entry(gnx.clone())
                    .or_default()
                    .push(format!("@comment {delims}\n"));
                let (d1, d2, d3) = util::set_delims_from_string(&delims);
                if !d1.is_empty() {
                    comment_delim1 = d1;
                    comment_delim2 = String::new();
                } else {
                    comment_delim1 = d2;
                    comment_delim2 = d3;
                }
                doc_skip = [format!("{comment_delim1}\n"), format!("{comment_delim2}\n")];
                is_cweb = comment_delim1 == "@q@" && comment_delim2 == "@>";
                sentinel = format!("{comment_delim1}@");
                verbatim_line = format!("{comment_delim1}@verbatim{comment_delim2}");
                self.pats = Patterns::new(&comment_delim1, &comment_delim2);
                continue;
            }

            if let Some(m) = self.pats.delims.captures(&line) {
                let delims = m[1].trim().to_string();
                gnx2body
                    .entry(gnx.clone())
                    .or_default()
                    .push(format!("@delims {delims}\n"));
                let mut parts = delims.split_whitespace();
                match parts.next() {
                    Some(d1) => {
                        comment_delim1 = d1.replace("__", "\n").replace('_', " ");
                        comment_delim2 = parts
                            .next()
                            .map(|d| d.replace("__", "\n").replace('_', " "))
                            .unwrap_or_default();
                    }
                    None => {
                        self.warnings
                            .push(format!("ignoring invalid @delims: {line:?}"));
                        continue;
                    }
                }
                doc_skip = [format!("{comment_delim1}\n"), format!("{comment_delim2}\n")];
                is_cweb = comment_delim1 == "@q@" && comment_delim2 == "@>";
                sentinel = format!("{comment_delim1}@");
                verbatim_line = format!("{comment_delim1}@verbatim{comment_delim2}");
                self.pats = Patterns::new(&comment_delim1, &comment_delim2);
                continue;
            }

            if let Some(m) = self.pats.section_delims.captures(&line) {
                if section_reference_seen {
                    self.warnings
                        .push("section-delims seen after a section reference".to_string());
                } else {
                    section_delim1 = m[1].to_string();
                    section_delim2 = m.get(2).map(|x| x.as_str()).unwrap_or("").to_string();
                    self.pats.set_section_delims(
                        &section_delim1,
                        &section_delim2,
                        &comment_delim1,
                        &comment_delim2,
                    );
                }
                gnx2body
                    .entry(gnx.clone())
                    .or_default()
                    .push(format!("@section-delims {} {}\n", &m[1], &m[2]));
                continue;
            }

            // @first, @last, @delims and @comment all produce @@ sentinels, so
            // this must follow every one of them.
            if line.starts_with(&format!("{comment_delim1}@@")) {
                let ii = comment_delim1.len() + 1;
                let jj = if comment_delim2.is_empty() {
                    line.trim_end_matches('\n').len()
                } else {
                    line.rfind(&comment_delim2).unwrap_or(line.len())
                };
                let text = format!("{}\n", line.get(ii..jj.max(ii)).unwrap_or(""));
                gnx2body.entry(gnx.clone()).or_default().push(text);
                continue;
            }

            if in_doc {
                if !comment_delim2.is_empty() {
                    // Inner doc lines carry no delimiters.
                    gnx2body.entry(gnx.clone()).or_default().push(line);
                    continue;
                }
                let lstripped = line.trim_start();
                let cut = comment_delim1.trim_end().len() + 1;
                let tail = lstripped.get(cut..).unwrap_or("").to_string();
                let text = if tail.trim().is_empty() {
                    "\n".to_string()
                } else {
                    tail
                };
                gnx2body.entry(gnx.clone()).or_default().push(text);
                continue;
            }

            // An apparent sentinel that matched nothing. Keep it, with a
            // warning: dropping it would silently lose the user's text (#2213).
            self.warnings.push(format!(
                "{}: inserting unexpected line: {:?}",
                util::short_file_name(&self.path),
                line.trim_end()
            ));
            gnx2body.entry(gnx.clone()).or_default().push(line);
        }

        if !saw_at_leo {
            return; // No @-leo sentinel: not a Leo file after all.
        }
        if !stack.is_empty() {
            self.warnings
                .push("scan_lines: stack should be empty".to_string());
        }

        // Trailing lines become @last directives on the node the scan ended in.
        let tail_start = header.start + i;
        if tail_start < lines.len() {
            let tail_lines = &lines[tail_start..];
            if !tail_lines.is_empty() {
                let last: Vec<String> = tail_lines
                    .iter()
                    .map(|z| format!("@last {}\n", z.trim_end()))
                    .collect();
                if n_last_lines != last.len() {
                    self.warnings.push(format!(
                        "expected {n_last_lines} trailing lines, got {}",
                        last.len()
                    ));
                }
                gnx2body.entry(gnx.clone()).or_default().extend(last);
            }
        }

        // Set every body at once: a clone's body is written by whichever
        // occurrence the file spells last, and only this pass sees that.
        for (key, body) in gnx2body {
            if let Some(v) = self.o.find_gnx(&key) {
                self.o.node_mut(v).b = body.concat();
            }
        }
        self.o.generation += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atfile_write::tangle;
    use crate::Outline;

    /// Tangle an outline, read it back, and compare the trees.
    fn round_trip(o: &mut Outline) -> Vec<(String, String, String)> {
        let root = o.root_position().unwrap();
        let text = tangle(o, &root).unwrap();
        let mut o2 = Outline::new_empty();
        let root2 = o2.root_position().unwrap();
        o2.set_headline(&root2, o.root_position().unwrap().h(o));
        assert!(read_into_root(&mut o2, &text, "test.py", &root2));
        o2.all_positions()
            .iter()
            .map(|p| {
                (
                    p.gnx(&o2).to_string(),
                    p.h(&o2).to_string(),
                    p.b(&o2).to_string(),
                )
            })
            .collect()
    }

    #[test]
    fn a_tangled_file_reads_back_into_the_same_tree() {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "@file test.py");
        o.set_body(&root, "import os\n@others\n");
        let a = o.insert_as_last_child(&root);
        o.set_headline(&a, "def f");
        o.set_body(&a, "def f():\n    @others\n");
        let a1 = o.insert_as_last_child(&a);
        o.set_headline(&a1, "body");
        o.set_body(&a1, "return 1\n");
        let b = o.insert_as_last_child(&root);
        o.set_headline(&b, "def g");
        o.set_body(&b, "def g():\n    pass\n");

        let got = round_trip(&mut o);
        let want: Vec<(String, String, String)> = o
            .all_positions()
            .iter()
            .map(|p| {
                (
                    p.gnx(&o).to_string(),
                    p.h(&o).to_string(),
                    p.b(&o).to_string(),
                )
            })
            .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn a_verbatim_line_survives_the_round_trip() {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "@file test.py");
        o.set_body(&root, "#@+node:not really\nx = 1\n");
        let got = round_trip(&mut o);
        assert_eq!(got[0].2, "#@+node:not really\nx = 1\n");
    }

    #[test]
    fn a_doc_part_survives_the_round_trip() {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "@file test.py");
        o.set_body(&root, "@\nSome prose.\n\nMore prose.\n@c\nx = 1\n");
        let got = round_trip(&mut o);
        assert_eq!(got[0].2, "@\nSome prose.\n\nMore prose.\n@c\nx = 1\n");
    }

    #[test]
    fn at_first_and_at_last_lines_survive() {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "@file test.py");
        o.set_body(
            &root,
            "@first #!/usr/bin/env python3\nx = 1\n@last # tail\n",
        );
        let text = tangle(&o, &root).unwrap();
        assert!(text.starts_with("#!/usr/bin/env python3\n"), "{text}");
        assert!(text.ends_with("# tail\n"), "{text}");
        let got = round_trip(&mut o);
        assert_eq!(
            got[0].2,
            "@first #!/usr/bin/env python3\nx = 1\n@last # tail\n"
        );
    }

    #[test]
    fn a_clone_reads_back_as_one_vnode() {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "@file test.py");
        o.set_body(&root, "@others\n");
        let a = o.insert_as_last_child(&root);
        o.set_headline(&a, "shared");
        o.set_body(&a, "shared = 1\n");
        o.clone_node(&a);
        let text = tangle(&o, &root).unwrap();
        let mut o2 = Outline::new_empty();
        let root2 = o2.root_position().unwrap();
        o2.set_headline(&root2, "@file test.py");
        assert!(read_into_root(&mut o2, &text, "test.py", &root2));
        assert_eq!(o2.all_positions().len(), 3);
        assert_eq!(o2.all_unique_positions().len(), 2);
    }
}
