//! Tangling: turning an outline into the text of its external files.
//!
//! The outline is the source of truth and the external files are regenerated
//! from it. This is the half of the `.leo` contract that lets an outline be
//! edited with no window and have the result land in the files a compiler
//! reads, so it has to match Leo byte for byte -- a sentinel written
//! differently is a file Leo can no longer read back into the same tree.

use std::collections::HashSet;

use crate::error::{Error, Result};
use crate::node::VnodeId;
use crate::outline::Outline;
use crate::position::Position;
use crate::util::{self, match_word, matches_at, skip_leading_ws_with_indent, skip_line, skip_ws};

/// The directives `putLine` dispatches on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    None,
    All,
    At,
    C,
    Code,
    Doc,
    Misc,
    Others,
    StartVerbatim,
}

/// Directives Leo recognizes in body text, without their `@`.
pub const GLOBAL_DIRECTIVES: &[&str] = &[
    "all",
    "beautify",
    "c",
    "code",
    "color",
    "colorcache",
    "comment",
    "delims",
    "doc",
    "encoding",
    "first",
    "header",
    "ignore",
    "killbeautify",
    "killcolor",
    "language",
    "last",
    "lineending",
    "markup",
    "nobeautify",
    "nocolor",
    "nocolor-node",
    "noheader",
    "nopyflakes",
    "nosearch",
    "nowrap",
    "others",
    "pagewidth",
    "path",
    "quiet",
    "section-delims",
    "silent",
    "tabwidth",
    "unit",
    "verbose",
    "wrap",
];

/// One external file being written.
pub struct AtWrite<'a> {
    o: &'a Outline,
    root: Position,
    out: String,
    indent: i32,
    /// Extra indent for a doc part nested in a leaf node's own block (#4864).
    doc_indent: i32,
    tab_width: i32,
    language: String,
    start_comment: String,
    end_comment: String,
    pub sentinels: bool,
    /// True: a section reference with no definition is written as a plain
    /// line. `@auto` files set it, because their trees are built by an
    /// importer that never creates section definition nodes.
    pub allow_undefined_refs: bool,
    section_delim1: String,
    section_delim2: String,
    encoding: String,
    pub output_newline: String,
    pub explicit_line_ending: bool,
    /// Nodes already written, so `@others` does not write one twice.
    visited: HashSet<VnodeId>,
    pub errors: Vec<String>,
}

/// Per-node state while scanning one body: doc mode and the directives seen.
#[derive(Default)]
struct Status {
    in_code: bool,
    has_at_others: bool,
    at_comment_seen: bool,
    at_delims_seen: bool,
    at_warning_given: bool,
}

impl<'a> AtWrite<'a> {
    /// Set up to write `root`'s file. Mirrors `at.initWriteIvars`.
    pub fn new(o: &'a Outline, root: &Position) -> Self {
        let language = o.get_language(root);
        let (d1, d2, d3) = o.get_delims(root);
        // Single-line comments when there is a choice: they nest.
        let (start_comment, end_comment) = if !d1.is_empty() {
            (d1, String::new())
        } else if !d2.is_empty() && !d3.is_empty() {
            (d2, d3)
        } else {
            ("#".to_string(), String::new())
        };
        let line_ending = o.get_line_ending(root);
        let explicit_line_ending = !line_ending.is_empty();
        let output_newline = if explicit_line_ending {
            line_ending
        } else {
            util::get_output_newline(&o.config.output_newline)
        };
        let mut at = Self {
            o,
            root: root.clone(),
            out: String::new(),
            indent: 0,
            doc_indent: 0,
            tab_width: o.get_tab_width(root),
            language,
            start_comment,
            end_comment,
            sentinels: true,
            allow_undefined_refs: false,
            section_delim1: "<<".to_string(),
            section_delim2: ">>".to_string(),
            encoding: o.get_encoding(root),
            output_newline,
            explicit_line_ending,
            visited: HashSet::new(),
            errors: Vec::new(),
        };
        at.scan_root_for_section_delims(root);
        at
    }

    fn scan_root_for_section_delims(&mut self, root: &Position) {
        let mut found = 0;
        for line in util::split_lines(root.b(self.o)) {
            if let Some(rest) = line.strip_prefix("@section-delims") {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if parts.len() == 2 {
                    found += 1;
                    self.section_delim1 = parts[0].to_string();
                    self.section_delim2 = parts[1].to_string();
                }
            }
        }
        if found > 1 {
            self.errors.push(format!(
                "Multiple @section-delims directives in {}",
                root.h(self.o)
            ));
            self.section_delim1 = "<<".to_string();
            self.section_delim2 = ">>".to_string();
        }
    }

    pub fn encoding(&self) -> &str {
        &self.encoding
    }

    // --- Output primitives ------------------------------------------------

    fn os(&mut self, s: &str) {
        self.out.push_str(s);
    }

    /// A newline. Deliberately '\n', not `output_newline`: line endings are
    /// converted once, when the file is written.
    fn onl(&mut self) {
        self.out.push('\n');
    }

    fn onl_sent(&mut self) {
        if self.sentinels {
            self.onl();
        }
    }

    fn put_indent(&mut self, n: i32) {
        if n > 0 {
            let w = self.tab_width;
            if w > 1 {
                let (q, r) = (n / w, n % w);
                self.os(&"\t".repeat(q as usize));
                self.os(&" ".repeat(r as usize));
            } else {
                self.os(&" ".repeat(n as usize));
            }
        }
    }

    // --- The file ---------------------------------------------------------

    /// Write root's whole file. Returns the text, with '\n' line endings.
    pub fn put_file(&mut self, root: &Position) -> String {
        self.visited.clear();
        let s = root.b(self.o).to_string();
        self.put_at_first_lines(&s);
        self.put_open_leo_sentinel("@+leo-ver=5");
        self.put_open_node_sentinel(root, false);
        self.put_body(root, None);
        // The @-leo sentinel is required to handle @last.
        self.put_sentinel("@-leo");
        self.visited.insert(root.v);
        self.put_at_last_lines(&s);
        std::mem::take(&mut self.out)
    }

    fn put_open_leo_sentinel(&mut self, s: &str) {
        if !self.sentinels {
            return;
        }
        let mut s = format!("{s}-thin");
        let encoding = self.encoding.to_lowercase();
        if encoding != "utf-8" {
            // Leo 4.2 and after: encoding fields end in ",."
            s.push_str(&format!("-encoding={encoding},."));
        }
        self.put_sentinel(&s);
    }

    fn put_at_first_lines(&mut self, s: &str) {
        let tag = "@first";
        let mut i = 0usize;
        while matches_at(s, i, tag) {
            i += tag.len();
            i = skip_ws(s, i);
            let j = i;
            i = util::skip_to_end_of_line(s, i);
            let line = s[j..i].to_string();
            self.os(&line);
            self.onl();
            i = util::skip_nl(s, i);
        }
    }

    fn put_at_last_lines(&mut self, s: &str) {
        let tag = "@last";
        let lines = util::split_lines(s);
        let n = lines.len();
        if n == 0 {
            return;
        }
        // Scan backwards over @last directives and blank lines.
        let mut j: i64 = n as i64 - 1;
        while j >= 0 {
            let line = &lines[j as usize];
            if matches_at(line, 0, tag) || line.trim().is_empty() {
                j -= 1;
            } else {
                break;
            }
        }
        for line in &lines[(j + 1) as usize..n] {
            if matches_at(line, 0, tag) {
                let i = skip_ws(line, tag.len());
                let tail = line[i..].to_string();
                self.os(&tail);
            }
        }
    }

    // --- One node's body --------------------------------------------------

    /// Write p's body, wrapped in sentinels. Returns true if it held @others.
    pub fn put_body(&mut self, p: &Position, from_string: Option<&str>) -> bool {
        let mut s = match from_string {
            Some(s) => s.to_string(),
            None => p.b(self.o).to_string(),
        };
        // Never expand this node again, and suppress the orphan check for it.
        self.visited.insert(p.v);
        // #1048 & #1037: regularize trailing whitespace.
        if !s.is_empty()
            && (self.sentinels || self.o.config.force_newlines_in_at_nosent_bodies)
            && !s.ends_with('\n')
        {
            s.push('\n');
        }
        let mut status = Status {
            in_code: true,
            ..Default::default()
        };
        let old_doc_indent = self.doc_indent;
        self.doc_indent = 0;
        let mut i = 0usize;
        while i < s.len() {
            let next_i = skip_line(&s, i);
            debug_assert!(next_i > i);
            let kind = self.directive_kind4(&s, i);
            self.put_line(i, kind, p, &s, &mut status);
            i = next_i;
        }
        if !status.in_code {
            self.put_end_doc_line();
            // An unclosed doc part still has its indent bump in effect.
            self.indent -= self.doc_indent;
        }
        self.doc_indent = old_doc_indent;
        status.has_at_others
    }

    fn put_line(&mut self, i: usize, kind: Kind, p: &Position, s: &str, status: &mut Status) {
        match kind {
            Kind::None => {
                if status.in_code {
                    if let Some((name, n1, n2)) = self.find_section_name(s, i) {
                        self.put_ref_line(s, i, n1, n2, &name, p);
                    } else {
                        self.put_code_line(s, i);
                    }
                } else {
                    self.put_doc_line(s, i);
                }
            }
            Kind::Doc | Kind::At => {
                if !status.in_code {
                    // Adjacent doc parts.
                    self.put_end_doc_line();
                } else {
                    self.indent += self.doc_indent;
                }
                self.put_start_doc_line(s, i, kind);
                status.in_code = false;
            }
            Kind::C | Kind::Code => {
                if !status.in_code {
                    self.put_end_doc_line();
                }
                self.put_directive(s, i, p);
                if !status.in_code {
                    self.indent -= self.doc_indent;
                }
                status.in_code = true;
            }
            Kind::All => {
                if status.in_code {
                    if *p == self.root {
                        self.put_at_all_line(s, i, p);
                    } else {
                        self.errors
                            .push(format!("@all not valid in: {}", p.h(self.o)));
                    }
                } else {
                    self.put_doc_line(s, i);
                }
            }
            Kind::Others => {
                if status.in_code {
                    if status.has_at_others {
                        self.errors
                            .push(format!("multiple @others in: {}", p.h(self.o)));
                    } else {
                        self.put_at_others_line(s, i, p);
                        status.has_at_others = true;
                    }
                } else {
                    self.put_doc_line(s, i);
                }
            }
            Kind::StartVerbatim => {
                self.errors
                    .push(format!("@verbatim is not a Leo directive: {}", p.h(self.o)));
            }
            Kind::Misc => {
                if match_word(s, i, "@comment") {
                    status.at_comment_seen = true;
                } else if match_word(s, i, "@delims") {
                    status.at_delims_seen = true;
                }
                if status.at_comment_seen && status.at_delims_seen && !status.at_warning_given {
                    status.at_warning_given = true;
                    self.errors
                        .push(format!("@comment and @delims in node {}", p.h(self.o)));
                }
                self.put_directive(s, i, p);
            }
        }
    }

    /// Which directive, if any, starts at s[i]. A port of `at.directiveKind4`.
    fn directive_kind4(&self, s: &str, i: usize) -> Kind {
        let b = s.as_bytes();
        let n = s.len();
        if i >= n || b[i] != b'@' {
            let j = skip_ws(s, i);
            if match_word(s, j, "@others") {
                return Kind::Others;
            }
            if match_word(s, j, "@all") {
                return Kind::All;
            }
            return Kind::None;
        }
        if i + 1 >= n || matches!(b[i + 1], b' ' | b'\t' | b'\n') {
            // A bare '@' is a doc part, except in cweb where it means nothing.
            return if self.language == "cweb" {
                Kind::None
            } else {
                Kind::At
            };
        }
        let next = s[i + 1..].chars().next().unwrap();
        if !next.is_alphabetic() {
            return Kind::None;
        }
        if self.language == "cweb" && match_word(s, i, "@c") {
            return Kind::None;
        }
        if self.language == "elixir" && match_word(s, i, "@doc ") {
            return Kind::None;
        }
        for (name, kind) in [
            ("@all", Kind::All),
            ("@c", Kind::C),
            ("@code", Kind::Code),
            ("@doc", Kind::Doc),
            ("@others", Kind::Others),
            ("@verbatim", Kind::StartVerbatim),
        ] {
            if match_word(s, i, name) {
                return kind;
            }
        }
        // A Leo directive, or a decorator that merely looks like one.
        let rest = &s[i + 1..];
        let word: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        if word.is_empty() || !GLOBAL_DIRECTIVES.contains(&word.as_str()) {
            return Kind::None;
        }
        // `@language.foo` and `@path(x)` are Python, not Leo.
        match rest[word.len()..].chars().next() {
            Some('.') | Some('(') => Kind::None,
            _ => Kind::Misc,
        }
    }

    // --- Code lines -------------------------------------------------------

    fn put_code_line(&mut self, s: &str, i: usize) {
        let j = skip_line(s, i);
        let k = skip_ws(s, i);
        let line = s[i..j].to_string();

        // #4864: remember this line's indent for a doc part that may follow.
        if !line.trim().is_empty() {
            let (_, indent) = skip_leading_ws_with_indent(s, i, self.tab_width);
            self.doc_indent = indent;
        }

        // A line that looks like a sentinel must be marked, or reading the file
        // back would treat the user's own text as Leo's.
        let looks_like_sentinel = if self.language == "python" {
            // Python sentinels *only* may carry a space between '#' and '@'.
            matches_at(s, k, "#@") || matches_at(s, k, "# @")
        } else {
            matches_at(s, k, &format!("{}@", self.start_comment))
        };
        // Only with sentinels: without them the sentinel is suppressed but its
        // indent was written anyway, so every such line in an @auto or
        // @nosent file gained its own indentation twice.
        //
        // @clean included. Leo's #2996 left @clean out, but reading an @clean
        // file compares it against this very text, written with sentinels:
        // without @verbatim, each line that only looks like a sentinel was
        // taken for one, kept, and inserted again as text, so reading the
        // file doubled it. leo-editor removed #2996 for the same reason.
        if self.sentinels && looks_like_sentinel {
            let ws_len = k - i;
            self.put_indent(ws_len as i32);
            self.put_sentinel("@verbatim");
        }

        if line.len() > 1 {
            self.put_indent(self.indent);
            if line.ends_with('\n') {
                let head = line[..line.len() - 1].to_string();
                self.os(&head);
                self.onl();
            } else {
                self.os(&line);
            }
        } else if line == "\n" {
            self.onl();
        } else if !line.is_empty() {
            self.os(&line);
        }
    }

    /// The section reference on the line at s[i], as (name, start, end).
    ///
    /// The name includes its brackets. A reference must be alone on its line
    /// apart from whitespace; anything else is code that merely contains `<<`.
    fn find_section_name(&self, s: &str, i: usize) -> Option<(String, usize, usize)> {
        let end = s[i..].find('\n').map(|k| i + k);
        let j = end.unwrap_or(s.len());
        let hay = &s[i..j];
        let n1 = hay.find(&self.section_delim1).map(|k| i + k);
        let n2 = hay.find(&self.section_delim2).map(|k| i + k);
        let (n1, n2) = (n1?, n2?);
        if n1 >= n2 {
            return None;
        }
        let n3 = n2 + self.section_delim2.len();
        let is_space = |a: usize, b: usize| -> bool {
            a >= b || s[a..b].chars().all(|c| c == ' ' || c == '\t' || c == '\n')
        };
        if is_space(i, n1) && is_space(n3, j) {
            return Some((s[n1..n3].to_string(), n1, n3));
        }
        None
    }

    /// The node defining section `name`, searched for in p's subtree.
    fn find_reference(&self, name: &str, p: &Position) -> Option<Position> {
        p.subtree(self.o)
            .into_iter()
            .find(|p2| p2.match_headline(self.o, name) && !p2.is_at_ignore_node(self.o))
    }

    fn put_ref_line(&mut self, s: &str, i: usize, n1: usize, _n2: usize, name: &str, p: &Position) {
        if let Some(reference) = self.find_reference(name, p) {
            let (_, delta) = skip_leading_ws_with_indent(s, i, self.tab_width);
            self.put_lead_in_sentinel(s, i, n1);
            self.indent += delta;
            self.put_sentinel(&format!("@+{name}"));
            self.put_open_node_sentinel(&reference, false);
            self.put_body(&reference, None);
            self.put_sentinel(&format!("@-{name}"));
            self.indent -= delta;
            return;
        }
        if self.allow_undefined_refs {
            // An @auto file: nothing here promises the line is a reference.
            self.put_code_line(s, i);
            return;
        }
        // No definition. Leo refuses rather than guessing, and so does this:
        // writing the line unchanged would produce a file that does not read
        // back into the same tree.
        self.errors.push(format!(
            "undefined section: {}\n  referenced from: {}",
            util::truncate(name, 60),
            util::truncate(p.h(self.o), 60)
        ));
    }

    fn put_at_others_line(&mut self, s: &str, i: usize, p: &Position) {
        let (j, delta) = skip_leading_ws_with_indent(s, i, self.tab_width);
        let k = util::skip_to_end_of_line(s, i);
        self.put_lead_in_sentinel(s, i, j);
        self.indent += delta;
        // s[j..k] starts with '@others'. Never write leading whitespace in a sentinel.
        let tail = s[j + 1..k].trim().to_string();
        self.put_sentinel(&format!("@+{tail}"));
        for child in p.children(self.o) {
            let after = child.node_after_tree(self.o);
            let mut cur = Some(child);
            while let Some(q) = cur {
                if Some(&q) == after.as_ref() {
                    break;
                }
                if self.valid_in_at_others(&q) {
                    self.put_open_node_sentinel(&q, false);
                    if self.put_body(&q, None) {
                        cur = q.node_after_tree(self.o);
                    } else {
                        cur = q.thread_next(self.o);
                    }
                } else {
                    cur = q.node_after_tree(self.o);
                }
            }
        }
        self.put_sentinel("@-others");
        self.indent -= delta;
    }

    /// A section definition node is written where it is referenced, not here.
    fn valid_in_at_others(&mut self, p: &Position) -> bool {
        let h = p.h(self.o);
        let i = skip_ws(h, 0);
        if self.is_section_name(h, i) {
            return false;
        }
        if self.sentinels {
            // @ignore must not stop expansion when sentinels record the tree.
            return true;
        }
        if p.is_at_ignore_node(self.o) {
            self.errors
                .push(format!("did not write @ignore node {}", p.h(self.o)));
            return false;
        }
        true
    }

    fn is_section_name(&self, s: &str, mut i: usize) -> bool {
        while i < s.len() && s.as_bytes()[i] == b'.' {
            i += 1;
        }
        if !matches_at(s, i, &self.section_delim1) {
            return false;
        }
        util::find_on_line(s, i, &self.section_delim2).is_some()
    }

    fn put_at_all_line(&mut self, s: &str, i: usize, p: &Position) {
        let (j, delta) = skip_leading_ws_with_indent(s, i, self.tab_width);
        let k = util::skip_to_end_of_line(s, i);
        self.put_lead_in_sentinel(s, i, j);
        self.indent += delta;
        let tail = s[j + 1..k].trim().to_string();
        self.put_sentinel(&format!("@+{tail}"));
        for child in p.children(self.o) {
            self.put_at_all_child(&child);
        }
        self.put_sentinel("@-all");
        self.indent -= delta;
    }

    fn put_at_all_child(&mut self, p: &Position) {
        self.put_open_node_sentinel(p, true);
        self.put_at_all_body(p);
        for child in p.children(self.o) {
            self.put_at_all_child(&child);
        }
    }

    fn put_at_all_body(&mut self, p: &Position) {
        let mut s = p.b(self.o).to_string();
        self.visited.insert(p.v);
        if self.sentinels && !s.is_empty() && !s.ends_with('\n') {
            s.push('\n');
        }
        let mut i = 0usize;
        // @all never changes doc/code status.
        while i < s.len() {
            let next_i = skip_line(&s, i);
            self.put_code_line(&s, i);
            i = next_i;
        }
    }

    // --- Doc lines --------------------------------------------------------

    fn put_blank_doc_line(&mut self) {
        if self.end_comment.is_empty() {
            self.put_indent(self.indent);
            let c = self.start_comment.clone();
            self.os(&c);
        }
        self.onl();
    }

    fn put_doc_line(&mut self, s: &str, i: usize) {
        let j = skip_line(s, i);
        let line = s[i..j].to_string();
        if line.trim().is_empty() {
            self.put_blank_doc_line();
            return;
        }
        self.put_indent(self.indent);
        if self.end_comment.is_empty() {
            let c = self.start_comment.clone();
            self.os(&c);
            self.os(" ");
        }
        self.os(&line);
        if !line.ends_with('\n') {
            self.onl();
        }
    }

    fn put_end_doc_line(&mut self) {
        if !self.end_comment.is_empty() {
            self.put_indent(self.indent);
            let c = self.end_comment.clone();
            self.os(&c);
            self.onl();
        }
    }

    fn put_start_doc_line(&mut self, s: &str, i: usize, kind: Kind) {
        let (sentinel, directive) = if kind == Kind::Doc {
            ("@+doc", "@doc")
        } else {
            ("@+at", "@")
        };
        let i = i + directive.len();
        let j = util::skip_to_end_of_line(s, i);
        let follow = s[i..j].to_string();
        self.put_sentinel(&format!("{sentinel}{follow}"));
        if !self.end_comment.is_empty() {
            self.put_indent(self.indent);
            let c = self.start_comment.clone();
            self.os(&c);
            self.onl();
        }
    }

    // --- Sentinels --------------------------------------------------------

    fn node_sentinel_text(&self, p: &Position) -> String {
        let h = self.remove_comment_delims(p);
        let gnx = p.gnx(self.o);
        let level = 1 + p.level() - self.root.level();
        if level > 2 {
            format!("{gnx}: *{level}* {h}")
        } else {
            format!("{gnx}: {} {h}", "*".repeat(level))
        }
    }

    /// Strip block comment delimiters from a headline.
    ///
    /// With no single-line delimiter, a headline containing the block delims
    /// would end the sentinel early and split the node in two on read.
    fn remove_comment_delims(&self, p: &Position) -> String {
        let h = p.h(self.o);
        if self.end_comment.is_empty() {
            return h.to_string();
        }
        h.replace(&self.start_comment, "")
            .replace(&self.end_comment, "")
    }

    fn put_open_node_sentinel(&mut self, p: &Position, in_at_all: bool) {
        if !in_at_all && p.is_at_file_node(self.o) && *p != self.root {
            self.errors
                .push(format!("@file not valid in: {}", p.h(self.o)));
            return;
        }
        let s = self.node_sentinel_text(p);
        self.put_sentinel(&format!("@+node:{s}"));
    }

    /// i starts a line; j is where the @others or section reference begins.
    fn put_lead_in_sentinel(&mut self, s: &str, i: usize, j: usize) {
        if i == j {
            return; // The @others or reference starts the line.
        }
        let k = skip_ws(s, i);
        if j != k {
            self.put_indent(self.indent);
            let text = s[i..j].to_string();
            self.os(&text);
            self.onl_sent();
        }
    }

    fn put_sentinel(&mut self, s: &str) {
        if !self.sentinels {
            return;
        }
        self.put_indent(self.indent);
        let start = self.start_comment.clone();
        self.os(&start);
        // Blacken python sentinels.
        if self.language == "python" {
            self.os(" ");
        }
        // The cweb hack: with an opening delim ending in '@', double every
        // '@' but the first, because the delim itself introduces one.
        let s = if start.ends_with('@') {
            s.replace('@', "@@")[1..].to_string()
        } else {
            s.to_string()
        };
        self.os(&s);
        if !self.end_comment.is_empty() {
            let end = self.end_comment.clone();
            self.os(&end);
        }
        self.onl();
    }

    fn put_directive(&mut self, s: &str, i: usize, p: &Position) {
        let k = i;
        let j = util::skip_to_end_of_line(s, i);
        let directive = s[i..j].to_string();
        if match_word(s, k, "@delims") {
            self.put_delims(&directive, s, k);
        } else if match_word(s, k, "@last") || match_word(s, k, "@first") {
            // #1307, #1297: an @first or @last line becomes a bare sentinel.
            // Whatever follows the directive is written outside the sentinels,
            // by put_at_first_lines and put_at_last_lines.
            if p.is_at_clean_node(self.o) {
                let word = if match_word(s, k, "@last") {
                    "@last"
                } else {
                    "@first"
                };
                self.errors
                    .push(format!("ignoring {word} directive in {:?}", p.h(self.o)));
            } else if match_word(s, k, "@last") {
                self.put_sentinel("@@last");
            } else {
                self.put_sentinel("@@first");
            }
        } else {
            self.put_sentinel(&format!("@{directive}"));
        }
    }

    fn put_delims(&mut self, directive: &str, s: &str, k: usize) {
        // A trailing blank protects the last delim.
        self.put_sentinel(&format!("{directive} "));
        let mut i = skip_ws(s, k + "@delims".len());
        let mut j = i;
        let b = s.as_bytes();
        while i < b.len() && !util::is_ws(b[i]) && !util::is_nl(s, i) {
            i += 1;
        }
        if j < i {
            self.start_comment = s[j..i].to_string();
            i = skip_ws(s, i);
            j = i;
            while i < b.len() && !util::is_ws(b[i]) && !util::is_nl(s, i) {
                i += 1;
            }
            self.end_comment = if j < i {
                s[j..i].to_string()
            } else {
                String::new()
            };
        } else {
            self.errors.push("Bad @delims directive".to_string());
        }
    }

    /// True if the line is a Leo directive rather than content.
    ///
    /// An @edit file is its node's body with these lines removed.
    pub fn is_directive_line(&self, line: &str) -> bool {
        self.directive_kind4(line, 0) != Kind::None
    }

    /// Nodes in root's tree that no sentinel claimed: an orphan is a lost node.
    pub fn orphans(&self, root: &Position) -> Vec<String> {
        root.self_and_subtree(self.o)
            .iter()
            .filter(|p| !self.visited.contains(&p.v))
            .map(|p| p.h(self.o).to_string())
            .collect()
    }
}

/// The text of p's external file, without writing anything.
///
/// Sentinels are included only for the node kinds whose files carry them.
pub fn tangle(o: &Outline, p: &Position) -> Result<String> {
    let sentinels =
        p.is_at_file_node(o) || p.is_at_thin_file_node(o) || p.is_at_shadow_file_node(o);
    at_file_to_string(o, p, sentinels)
}

/// Write p's file to a string. `sentinels` is false for @clean and @nosent.
pub fn at_file_to_string(o: &Outline, p: &Position, sentinels: bool) -> Result<String> {
    write_to_string(o, p, sentinels, false)
}

/// Write p's file to a string, saying whether undefined section references
/// are an error. Only an `@auto` file allows them.
pub fn write_to_string(
    o: &Outline,
    p: &Position,
    sentinels: bool,
    allow_undefined_refs: bool,
) -> Result<String> {
    let mut at = AtWrite::new(o, p);
    at.sentinels = sentinels;
    at.allow_undefined_refs = allow_undefined_refs;
    let contents = at.put_file(p);
    if at.errors.is_empty() {
        Ok(contents)
    } else {
        Err(Error::Write {
            detail: at.errors.join("\n"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Outline;

    fn python_outline() -> (Outline, Position) {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "@file test.py");
        o.set_body(&root, "@others\n");
        (o, root)
    }

    #[test]
    fn a_file_gets_leo_and_node_sentinels() {
        let (mut o, root) = python_outline();
        let child = o.insert_as_last_child(&root);
        o.set_headline(&child, "child");
        o.set_body(&child, "print(1)\n");
        let s = tangle(&o, &root).unwrap();
        assert!(s.starts_with("# @+leo-ver=5-thin\n"), "{s}");
        assert!(s.contains("# @+others\n"), "{s}");
        assert!(s.contains("print(1)\n"));
        assert!(s.trim_end().ends_with("# @-leo"), "{s}");
    }

    #[test]
    fn at_others_indents_its_children() {
        let (mut o, root) = python_outline();
        o.set_body(&root, "def f():\n    @others\n");
        let child = o.insert_as_last_child(&root);
        o.set_headline(&child, "body");
        o.set_body(&child, "return 1\n");
        let s = tangle(&o, &root).unwrap();
        assert!(s.contains("    return 1\n"), "{s}");
        assert!(s.contains("    # @+others\n"), "{s}");
    }

    #[test]
    fn a_line_that_looks_like_a_sentinel_is_marked_verbatim() {
        let (mut o, root) = python_outline();
        o.set_body(&root, "#@+node:fake\n");
        let s = tangle(&o, &root).unwrap();
        assert!(s.contains("# @verbatim\n#@+node:fake\n"), "{s}");
    }

    #[test]
    fn an_undefined_section_reference_is_an_error() {
        // Writing the line unchanged would produce a file that no longer reads
        // back into this tree, so refusing is the only safe answer.
        let (mut o, root) = python_outline();
        o.set_body(&root, "<< missing >>\n");
        let err = tangle(&o, &root).unwrap_err();
        assert!(matches!(err, Error::Write { .. }), "{err}");
        assert!(err.to_string().contains("undefined section"), "{err}");
    }

    #[test]
    fn a_section_reference_writes_the_definition_in_place() {
        let (mut o, root) = python_outline();
        o.set_body(&root, "x = 1\n<< helper >>\n");
        let child = o.insert_as_last_child(&root);
        o.set_headline(&child, "<< helper >>");
        o.set_body(&child, "y = 2\n");
        let s = tangle(&o, &root).unwrap();
        assert!(s.contains("# @+<< helper >>\n"), "{s}");
        assert!(s.contains("y = 2\n"), "{s}");
        assert!(s.contains("# @-<< helper >>\n"), "{s}");
    }

    #[test]
    fn at_clean_writes_no_sentinels() {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "@clean test.py");
        o.set_body(&root, "@others\n");
        let child = o.insert_as_last_child(&root);
        o.set_headline(&child, "child");
        o.set_body(&child, "print(1)\n");
        let s = at_file_to_string(&o, &root, false).unwrap();
        assert_eq!(s, "print(1)\n");
    }
}
