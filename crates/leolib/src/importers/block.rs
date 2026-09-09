//! The block importer: the algorithm behind almost all of Leo's importers.
//!
//! Comments and strings are blanked out first, producing **guide lines** that
//! have the same length and the same significant characters as the original.
//! Everything after that -- finding blocks, nesting them, computing the common
//! indentation -- reads the guide lines and edits the real ones. That is the
//! trick that keeps each language's importer down to a table of patterns.

use std::collections::HashSet;

use crate::importers::{
    lines as line_importers, python, rust_lang, EndOfBlock, FindBlocks, GuideKind, HeadlineKind,
    ImportReport, LanguageSpec, Postprocess,
};
use crate::node::VnodeId;
use crate::outline::{set_delims_from_language, Outline};
use crate::position::Position;
use crate::util;

/// One imported block, and the node it becomes.
#[derive(Debug, Clone)]
pub struct Block {
    pub kind: String,
    pub name: String,
    /// First line of the block, including any lines since the previous block.
    pub start: usize,
    /// First line of the block's body: the line after its defining line.
    pub start_body: usize,
    pub end: usize,
    pub v: Option<VnodeId>,
    pub parent_v: Option<VnodeId>,
    pub children: Vec<usize>,
}

impl Block {
    fn new(kind: &str, name: &str, start: usize, start_body: usize, end: usize) -> Self {
        Self {
            kind: kind.to_string(),
            name: name.to_string(),
            start,
            start_body,
            end,
            v: None,
            parent_v: None,
            children: Vec::new(),
        }
    }
}

/// The state of one import.
pub struct Importer<'a> {
    pub o: &'a mut Outline,
    pub spec: &'a LanguageSpec,
    pub lines: Vec<String>,
    pub guide_lines: Vec<String>,
    pub tab_width: i32,
    pub root: Position,
    pub blocks: Vec<Block>,
    at_others: HashSet<VnodeId>,
}

/// Import `contents` into the `@auto` node at `parent`.
pub fn import(
    o: &mut Outline,
    parent: &Position,
    contents: &str,
    spec: &LanguageSpec,
) -> ImportReport {
    let tab_width = o.get_tab_width(parent);
    // Leo strips every carriage return before importing, so a CRLF file
    // becomes an LF file in the outline -- and on the next write.
    let contents = contents.replace('\r', "");
    let raw = util::split_lines_at_newline(&contents);
    let regularized = !spec.allow_mixed_whitespace && !check_blanks_and_tabs(&raw, tab_width);
    let lines = if regularized {
        regularize_whitespace(&raw, tab_width)
    } else {
        raw
    };

    // A cloned @auto node would duplicate its own children (#449).
    o.detach_subtree(parent.v);
    o.node_mut(parent.v).b = String::new();

    let lines = if matches!(spec.end_of_block, EndOfBlock::Tags) {
        preprocess_xml_lines(&lines)
    } else {
        lines
    };
    // What the tree must write back: the lines as the importer sees them,
    // before generate_all_bodies edits them in place.
    let text = lines.concat();

    let mut i = Importer {
        guide_lines: Vec::new(),
        lines,
        tab_width,
        root: parent.clone(),
        blocks: Vec::new(),
        at_others: HashSet::new(),
        spec,
        o,
    };
    match spec.line_importer {
        Some(kind) => line_importers::gen_block(&mut i, kind),
        None => {
            i.guide_lines = i.make_guide_lines();
            debug_assert_eq!(i.lines.len(), i.guide_lines.len());
            i.gen_block();
        }
    }
    i.add_directives();

    let nodes = parent.self_and_subtree(i.o).len();
    let language = spec.language.to_string();
    ImportReport {
        language,
        nodes,
        regularized_whitespace: regularized,
        text,
        ..Default::default()
    }
}

/// Put each pair of adjacent tags on its own line.
///
/// Leo's XML importer does this so a block can start at the beginning of a
/// line. It rewrites the file, which is why an `@auto` XML node writes back
/// something tidier than it read.
fn preprocess_xml_lines(lines: &[String]) -> Vec<String> {
    static TAG_NAME: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| regex::Regex::new(r"^</?([a-zA-Z]+)").unwrap());
    static ADJACENT: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
        regex::Regex::new(r"(.*?)(<[^!].*?>)\s*(<[^!].*?>)").unwrap()
    });
    let mut out = Vec::new();
    for line in lines {
        let replaced = ADJACENT.replace_all(line, |m: &regex::Captures| {
            let name = |s: &str| {
                TAG_NAME
                    .captures(s)
                    .map(|c| c[1].to_string())
                    .unwrap_or_default()
            };
            // Do not split a pair that opens and closes the same element.
            let same_element =
                name(&m[2]) == name(&m[3]) && !m[2].starts_with("</") && m[3].starts_with("</");
            let lws = util::get_leading_ws(&m[1]);
            let sep = if same_element {
                String::new()
            } else {
                format!("\n{lws}")
            };
            format!("{}{}{sep}{}", &m[1], m[2].trim_end(), &m[3])
        });
        out.extend(util::split_lines(&replaced));
    }
    out
}

/// False if the file mixes blanks and tabs, or disagrees with @tabwidth.
fn check_blanks_and_tabs(lines: &[String], tab_width: i32) -> bool {
    let mut blanks = 0usize;
    let mut tabs = 0usize;
    for s in lines {
        let lws = util::get_leading_ws(s);
        blanks += lws.matches(' ').count();
        tabs += lws.matches('\t').count();
    }
    let ok = if tab_width < 0 {
        tabs == 0
    } else if tab_width > 0 {
        blanks == 0
    } else {
        true
    };
    ok && (blanks == 0 || tabs == 0)
}

/// Convert leading tabs to blanks, or the reverse, to match @tabwidth.
///
/// This changes the text, so a file it touches is rewritten the next time the
/// `@auto` node is written. Leo does the same and prints a message; the caller
/// gets `ImportReport::regularized_whitespace` instead.
fn regularize_whitespace(lines: &[String], tab_width: i32) -> Vec<String> {
    if tab_width < 0 {
        lines
            .iter()
            .map(|line| {
                let (i, w) = util::skip_leading_ws_with_indent(line, 0, tab_width);
                format!(
                    "{}{}",
                    util::compute_leading_whitespace(w, -tab_width.abs()),
                    &line[i..]
                )
            })
            .collect()
    } else if tab_width > 0 {
        lines
            .iter()
            .map(|line| util::optimize_leading_whitespace(line, tab_width.abs()))
            .collect()
    } else {
        lines.to_vec()
    }
}

impl Importer<'_> {
    // --- Guide lines ------------------------------------------------------

    fn make_guide_lines(&self) -> Vec<String> {
        match self.spec.guide_kind {
            GuideKind::Default => self.delete_comments_and_strings(false),
            GuideKind::JavaScript => self.delete_comments_and_strings(true),
            GuideKind::Perl => delete_regexes(&self.delete_comments_and_strings(false)),
            GuideKind::Python => python::delete_comments_and_strings(&self.lines),
            GuideKind::Rust => rust_lang::delete_comments_and_strings(&self.lines),
        }
    }

    /// Replace strings and comments with blanks, keeping every other character
    /// where it was. `handle_regex` also blanks an apparent `/.../` literal.
    fn delete_comments_and_strings(&self, handle_regex: bool) -> Vec<String> {
        let (line_comment, start_comment, end_comment) =
            set_delims_from_language(self.spec.language);
        let delims = &self.spec.string_list;
        let mut target = String::new(); // The delimiter that ends an open string.
        let mut result = Vec::with_capacity(self.lines.len());
        for line in &self.lines {
            let mut out = String::new();
            let mut skip_count = 0usize;
            for (i, ch) in line.char_indices() {
                let rest = &line[i..];
                if ch == '\n' {
                    break; // The newline is added below.
                } else if skip_count > 0 {
                    out.push(' ');
                    skip_count -= 1;
                } else if ch == '\\' {
                    // #3620: test for the escape before testing for the target.
                    out.push(' ');
                    skip_count = 1;
                } else if !target.is_empty() {
                    out.push(' ');
                    if rest.starts_with(target.as_str()) {
                        skip_count = target.chars().count().saturating_sub(1);
                        target.clear();
                    }
                } else if !line_comment.is_empty() && rest.starts_with(&line_comment) {
                    break; // The rest of the line holds nothing significant.
                } else if let Some(z) = delims.iter().find(|z| rest.starts_with(**z)) {
                    out.push(' ');
                    target = z.to_string();
                    skip_count = z.chars().count().saturating_sub(1);
                } else if !start_comment.is_empty() && rest.starts_with(&start_comment) {
                    out.push(' ');
                    target = end_comment.clone();
                    skip_count = start_comment.chars().count().saturating_sub(1);
                } else if handle_regex && ch == '/' {
                    match rest[1..].find('/') {
                        Some(j) => {
                            // An *apparent* regular expression. The count runs
                            // one character past the closing slash, as Leo's
                            // `skip_count = j - i + 1` does.
                            out.push(' ');
                            skip_count = rest[1..][..j].chars().count() + 2;
                        }
                        None => out.push(ch),
                    }
                } else {
                    out.push(ch);
                }
            }
            // Trailing whitespace cannot hold a significant character.
            let end_s = if line.ends_with('\n') { "\n" } else { "" };
            result.push(format!("{}{end_s}", out.trim_end()));
        }
        result
    }

    // --- Blocks -----------------------------------------------------------

    /// Create all descendant blocks and the nodes for them.
    fn gen_block(&mut self) {
        let parent_v = self.root.v;
        let mut todo: Vec<usize> = self.find_blocks(0, self.lines.len());
        let mut outer = Block::new("outer", "outer-block", 0, 0, self.lines.len());
        outer.v = Some(parent_v);
        for b in &todo {
            self.blocks[*b].parent_v = Some(parent_v);
            outer.children.push(*b);
        }
        let outer_index = self.blocks.len();
        self.blocks.push(outer);

        let mut result_blocks: Vec<usize> = Vec::new();
        let mut head = 0usize;
        while head < todo.len() {
            let bi = todo[head];
            head += 1;
            let parent_v = self.blocks[bi].parent_v.expect("block has no parent");
            let child_v = self.o.new_child_vnode(parent_v);
            let headline = self.compute_headline(bi);
            self.o.node_mut(child_v).h = headline;
            self.blocks[bi].v = Some(child_v);
            result_blocks.push(bi);

            let (start_body, end) = (self.blocks[bi].start_body, self.blocks[bi].end);
            let inner = self.find_blocks(start_body, end);
            for ib in inner {
                self.blocks[ib].parent_v = Some(child_v);
                self.blocks[bi].children.push(ib);
                todo.push(ib);
            }
        }

        if self.blocks[outer_index].children.is_empty() {
            // Do *not* change the headline: put everything in the body.
            let text = self.lines.concat();
            self.o.node_mut(self.root.v).b = text;
            return;
        }
        self.generate_all_bodies(outer_index);
        self.postprocess();
    }

    /// All blocks in the given range of guide lines.
    ///
    /// There must be **no gaps** between the blocks: an `@others` directive
    /// will stand for all of them, so a skipped line is a lost line.
    fn find_blocks(&mut self, i1: usize, i2: usize) -> Vec<usize> {
        match self.spec.find_blocks {
            FindBlocks::Python => python::find_blocks(self, i1, i2),
            FindBlocks::Rust => rust_lang::find_blocks(self, i1, i2),
            FindBlocks::C => self.find_blocks_c(i1, i2),
            FindBlocks::Default => self.find_blocks_default(i1, i2),
        }
    }

    fn find_blocks_default(&mut self, i1: usize, i2: usize) -> Vec<usize> {
        let min_size = self.spec.minimum_block_size;
        let (mut i, mut prev_i) = (i1, i1);
        let mut results = Vec::new();
        while i < i2 {
            let s = self.guide_lines[i].clone();
            i += 1;
            for pi in 0..self.spec.block_patterns.len() {
                let (kind, name) = {
                    let (kind, pattern) = &self.spec.block_patterns[pi];
                    match pattern.captures(&s) {
                        None => continue,
                        Some(m) => (*kind, last_group(&m)),
                    }
                };
                if self.spec.compound_statements.contains(&name.as_str()) {
                    continue;
                }
                let end = self.find_end_of_block(i, i2);
                if min_size == 0 || end.saturating_sub(prev_i) > min_size {
                    results.push(self.push_block(kind, &name, prev_i, i, end));
                    i = end;
                    prev_i = end;
                } else {
                    i = end;
                }
                break;
            }
        }
        results
    }

    /// C and its relatives.
    ///
    /// Two things the default cannot do: a definition whose `{` is on the
    /// *next* line, and rejecting a match whose line already closes a block.
    fn find_blocks_c(&mut self, i1: usize, i2: usize) -> Vec<usize> {
        static MULTI_LINE_FUNC: once_cell::sync::Lazy<regex::Regex> =
            once_cell::sync::Lazy::new(|| {
                regex::Regex::new(r"^(?:.*?\b(\w+)\s*\(.*?\)\s*(const)?)").unwrap()
            });
        let (mut i, mut prev_i) = (i1, i1);
        let mut results = Vec::new();
        while i < i2 {
            let s = self.guide_lines[i].clone();
            i += 1;
            for pi in 0..self.spec.block_patterns.len() {
                let (kind, pattern) = &self.spec.block_patterns[pi];
                let kind = *kind;
                let matched = pattern.captures(&s).map(|m| {
                    let name = m.get(1).map(|g| g.as_str()).unwrap_or("").to_string();
                    let tail_end = m.get(1).map(|g| g.end()).unwrap_or(0);
                    (name, s[tail_end.min(s.len())..].to_string())
                });
                if let Some((name, tail)) = matched {
                    // A line that also closes a block is not a definition, and
                    // a compound statement is control flow, not a block.
                    if tail.contains('}') || self.spec.compound_statements.contains(&name.as_str())
                    {
                        continue;
                    }
                    let end = self.find_end_of_block(i, i2);
                    results.push(self.push_block(kind, &name, prev_i, i + 1, end));
                    i = end;
                    prev_i = end;
                    break;
                }
                if i < i2 {
                    let Some(m2) = MULTI_LINE_FUNC.captures(&s) else {
                        continue;
                    };
                    let name = m2.get(1).map(|g| g.as_str()).unwrap_or("").to_string();
                    // The next line must open the block.
                    if !self.guide_lines[i].trim_start().starts_with('{')
                        || self.spec.compound_statements.contains(&name.as_str())
                    {
                        continue;
                    }
                    let end = self.find_end_of_block(i + 1, i2);
                    results.push(self.push_block("func", &name, prev_i, i + 1, end));
                    i = end;
                    prev_i = end;
                    break;
                }
            }
        }
        results
    }

    pub fn push_block(
        &mut self,
        kind: &str,
        name: &str,
        start: usize,
        start_body: usize,
        end: usize,
    ) -> usize {
        self.blocks
            .push(Block::new(kind, name, start, start_body, end));
        self.blocks.len() - 1
    }

    /// The index of the line following the block starting at `i - 1`.
    pub fn find_end_of_block(&self, i: usize, i2: usize) -> usize {
        match self.spec.end_of_block {
            EndOfBlock::Braces => self.end_by_braces(i, i2),
            EndOfBlock::Indent => python::find_end_of_block(self, i, i2),
            EndOfBlock::Parens => self.end_by_parens(i, i2),
            EndOfBlock::LuaEnd => self.end_by_lua_end(i, i2),
            EndOfBlock::NextBlock => self.end_at_next_block(i, i2),
            EndOfBlock::Tags => self.end_by_tags(i, i2),
        }
    }

    fn end_by_braces(&self, mut i: usize, i2: usize) -> usize {
        let mut level: i32 = if self.guide_lines[i - 1].contains('{') {
            1
        } else {
            0
        };
        while i < i2 {
            let line = &self.guide_lines[i];
            i += 1;
            for ch in line.chars() {
                if ch == '{' {
                    level += 1;
                }
                if ch == '}' {
                    level -= 1;
                    if level == 0 {
                        return i;
                    }
                }
            }
        }
        i2
    }

    fn end_by_parens(&self, mut i: usize, i2: usize) -> usize {
        // The block starts on the *previous* line, which holds the open paren.
        i = i.saturating_sub(1);
        let mut level: i32 = 0;
        while i < i2 {
            let line = &self.guide_lines[i];
            i += 1;
            for ch in line.chars() {
                if ch == '(' {
                    level += 1;
                }
                if ch == ')' {
                    level -= 1;
                    if level == 0 {
                        return i;
                    }
                }
            }
        }
        i2
    }

    fn end_by_lua_end(&self, mut i: usize, i2: usize) -> usize {
        static END_PAT: once_cell::sync::Lazy<regex::Regex> =
            once_cell::sync::Lazy::new(|| regex::Regex::new(r"^.*?\bend\b").unwrap());
        let mut level = 1; // The previous line started the function.
        while i < i2 {
            let line = &self.guide_lines[i];
            i += 1;
            if self
                .spec
                .block_patterns
                .iter()
                .any(|(_, pat)| pat.is_match(line))
            {
                level += 1;
            } else if END_PAT.is_match(line) {
                level -= 1;
                if level == 0 {
                    return i;
                }
            }
        }
        i2
    }

    fn end_at_next_block(&self, mut i: usize, i2: usize) -> usize {
        while i < i2 {
            if self
                .spec
                .block_patterns
                .iter()
                .any(|(_, pat)| pat.is_match(&self.guide_lines[i]))
            {
                return i;
            }
            i += 1;
        }
        i2
    }

    /// XML: match the opening tag of line `i - 1` with its closing tag.
    fn end_by_tags(&self, mut i: usize, i2: usize) -> usize {
        let i1 = i;
        let mut stack: Vec<String> = Vec::new();
        match self.match_tag(&self.guide_lines[i1 - 1], false) {
            Some(tag) => stack.push(tag),
            None => return i1, // No opening tag: do not create a block.
        }
        while i < i2 {
            let line = self.guide_lines[i].clone();
            i += 1;
            if let Some(tag) = self.match_tag(&line, false) {
                stack.push(tag);
            }
            if let Some(end_tag) = self.match_tag(&line, true) {
                let mut matched = false;
                while let Some(tag) = stack.pop() {
                    if tag == end_tag {
                        if stack.is_empty() {
                            return i;
                        }
                        matched = true;
                        break;
                    }
                }
                if !matched && stack.is_empty() {
                    return i1;
                }
            }
        }
        i1
    }

    fn match_tag(&self, line: &str, closing: bool) -> Option<String> {
        for (tag, _) in &self.spec.block_patterns {
            let pattern = if closing {
                format!(r"^\s*</({tag})>")
            } else {
                format!(r"^\s*<({tag})")
            };
            if let Ok(re) = regex::Regex::new(&pattern) {
                if let Some(m) = re.captures(line) {
                    return Some(m[1].to_lowercase());
                }
            }
        }
        None
    }

    pub fn compute_headline(&self, bi: usize) -> String {
        let block = &self.blocks[bi];
        match self.spec.headline {
            HeadlineKind::Rust => rust_lang::compute_headline(block),
            HeadlineKind::Tag => {
                let n = block.start.max(block.start_body.saturating_sub(1));
                let s = self.lines.get(n).map(|s| s.trim()).unwrap_or("");
                util::truncate(s, 120)
            }
            HeadlineKind::Default => {
                let name = if block.name.is_empty() {
                    format!("unnamed {}", block.kind)
                } else {
                    block.name.clone()
                };
                if block.kind.is_empty() {
                    name
                } else {
                    format!("{} {name}", block.kind)
                }
            }
        }
    }

    // --- Bodies -----------------------------------------------------------

    /// Fill in every node's body, inserting `@others` where children go.
    fn generate_all_bodies(&mut self, outer_index: usize) {
        let mut todo = vec![outer_index];
        let mut head = 0usize;
        while head < todo.len() {
            let bi = todo[head];
            head += 1;
            let v = self.blocks[bi].v.expect("block has no vnode");
            let children = self.blocks[bi].children.clone();
            let common_lws = self.compute_common_lws(&children);
            self.remove_lws_from_blocks(&children, &common_lws);
            if bi != outer_index {
                let headline = self.compute_headline(bi);
                self.o.node_mut(v).h = headline;
            }
            if children.is_empty() {
                let (start, end) = (self.blocks[bi].start, self.blocks[bi].end);
                self.o.node_mut(v).b = self.lines[start..end].concat();
            } else {
                self.handle_block_with_children(bi, &common_lws);
            }
            todo.extend(children);
        }
    }

    fn handle_block_with_children(&mut self, bi: usize, common_lws: &str) {
        let (children_start, children_end) = self.find_all_child_lines(bi);
        let v = self.blocks[bi].v.expect("block has no vnode");
        let start = self.blocks[bi].start;
        let mut body = self.lines[start..children_start].concat();
        if self.at_others.insert(v) {
            body.push_str(&format!("{common_lws}@others\n"));
        }
        let end = self.blocks[bi].end;
        body.push_str(&self.lines[children_end..end].concat());
        self.o.node_mut(v).b = body;
        // The head lines now belong to this node; @others covers the rest.
        self.blocks[bi].end = children_start;
    }

    /// The range of lines `@others` will stand for.
    fn find_all_child_lines(&self, bi: usize) -> (usize, usize) {
        let children = &self.blocks[bi].children;
        let first = &self.blocks[children[0]];
        let mut start = first.start;
        let mut end = first.end;
        for ci in children {
            start = start.min(self.blocks[*ci].start);
            end = end.max(self.blocks[*ci].end);
        }
        (start, end)
    }

    /// The indentation every line of every block shares.
    fn compute_common_lws(&self, children: &[usize]) -> String {
        if children.is_empty() {
            return String::new();
        }
        let mut min: Option<usize> = None;
        for ci in children {
            let block = &self.blocks[*ci];
            for line in &self.lines[block.start..block.end] {
                let stripped = line.trim_start();
                if !stripped.is_empty() {
                    let n = line.chars().count() - stripped.chars().count();
                    min = Some(min.map_or(n, |m: usize| m.min(n)));
                }
            }
        }
        let n = min.unwrap_or(0);
        let ws = if self.tab_width < 1 { " " } else { "\t" };
        ws.repeat(n)
    }

    fn remove_lws_from_blocks(&mut self, children: &[usize], common_lws: &str) {
        if common_lws.is_empty() {
            return;
        }
        let n = common_lws.chars().count();
        for ci in children {
            let (start, end) = (self.blocks[*ci].start, self.blocks[*ci].end);
            for line in &mut self.lines[start..end] {
                if line.trim().is_empty() {
                    continue;
                }
                let lead: String = line.chars().take(n).collect();
                if lead.trim().is_empty() && lead.chars().count() == n {
                    *line = line.chars().skip(n).collect();
                }
            }
        }
    }

    // --- Finishing --------------------------------------------------------

    fn postprocess(&mut self) {
        self.move_blank_lines();
        match self.spec.postprocess {
            Postprocess::BlankLines => {}
            Postprocess::Python => python::postprocess(self),
            Postprocess::Preamble => {
                let patterns: Vec<regex::Regex> = self
                    .spec
                    .block_patterns
                    .iter()
                    .map(|(_, p)| p.clone())
                    .collect();
                self.move_module_preamble(&patterns);
            }
            Postprocess::Rust => rust_lang::postprocess(self),
        }
    }

    /// Move a blank line from the start of a node to the end of the previous one.
    fn move_blank_lines(&mut self) {
        let root = self.root.clone();
        for p in root.subtree(self.o) {
            let Some(back) = p.back(self.o) else { continue };
            loop {
                let b = self.o.node(p.v).b.clone();
                if b.is_empty() {
                    break;
                }
                let lines = util::split_lines(&b);
                if lines.is_empty() || !lines[0].trim().is_empty() {
                    break;
                }
                self.o.node_mut(back.v).b.push('\n');
                self.o.node_mut(p.v).b = lines[1..].concat();
            }
        }
    }

    /// Move the lines before the first block from the first child to the parent.
    pub fn move_module_preamble(&mut self, patterns: &[regex::Regex]) {
        let root = self.root.clone();
        let Some(child1) = root.first_child(self.o) else {
            return;
        };
        let lines = util::split_lines(&self.o.node(child1.v).b);
        for (i, line) in lines.iter().enumerate() {
            if patterns.iter().any(|p| p.is_match(line)) {
                if i == 0 {
                    return;
                }
                let preamble: String = lines[..i].concat();
                let parent_b = self.o.node(root.v).b.clone();
                self.o.node_mut(root.v).b = format!("{preamble}{parent_b}");
                let child_b = self.o.node(child1.v).b.clone();
                self.o.node_mut(child1.v).b = child_b.replacen(&preamble, "", 1);
                return;
            }
        }
    }

    /// Add the `@language` and `@tabwidth` directives Leo puts on an @auto root.
    fn add_directives(&mut self) {
        let v = self.root.v;
        if !self.o.node(v).b.is_empty() && !self.o.node(v).b.ends_with('\n') {
            self.o.node_mut(v).b.push('\n');
        }
        let text = format!(
            "@language {}\n@tabwidth {}\n",
            self.spec.language, self.tab_width
        );
        self.o.node_mut(v).b.push_str(&text);
    }

    pub fn lws_n(&self, s: &str) -> usize {
        s.chars().count() - s.trim_start().chars().count()
    }
}

/// The name of a block: the last group the pattern captured.
///
/// TypeScript's table numbers the name group differently in each pattern, and
/// it is always the last one; every other language uses group 1, which is also
/// the last for those patterns.
fn last_group(m: &regex::Captures) -> String {
    for i in (1..m.len()).rev() {
        if let Some(g) = m.get(i) {
            return g.as_str().trim().to_string();
        }
    }
    String::new()
}

/// Perl: cut each line at the start of an apparent regular expression.
fn delete_regexes(lines: &[String]) -> Vec<String> {
    static PAT: once_cell::sync::Lazy<regex::Regex> =
        once_cell::sync::Lazy::new(|| regex::Regex::new(r"^(.*?=\s*(m|s|tr|)/)").unwrap());
    lines
        .iter()
        .map(|line| match PAT.captures(line) {
            Some(m) => line[..m.get(0).unwrap().end()].to_string(),
            None => line.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::importers::{import_string, write_string};
    use crate::{Outline, Position};

    /// Import `text` as an @auto file named `name`, and return the outline.
    fn import(name: &str, text: &str) -> (Outline, Position) {
        let mut o = Outline::new_empty();
        o.file_name = "/tmp/test.leo".to_string();
        let root = o.root_position().unwrap();
        o.set_headline(&root, &format!("@auto {name}"));
        import_string(&mut o, &root, text, name).expect("no importer");
        (o, root)
    }

    fn tree(o: &Outline, root: &Position) -> Vec<String> {
        root.self_and_subtree(o)
            .iter()
            .map(|p| format!("{}{}", "  ".repeat(p.level()), p.h(o)))
            .collect()
    }

    /// The tree must write back exactly what the importer read.
    fn assert_round_trips(o: &Outline, root: &Position, name: &str, text: &str) {
        let written = write_string(o, root, name).expect("write failed");
        assert_eq!(written, text);
    }

    #[test]
    fn python_classes_and_defs_become_nodes() {
        let text = "\
import os


class C:

    def a(self):
        pass

    def b(self):
        pass


def top():
    pass
";
        let (o, root) = import("x.py", text);
        assert_eq!(
            tree(&o, &root),
            vec![
                "@auto x.py",
                "  class C",
                "    C.a",
                "    C.b",
                "  function: top"
            ]
        );
        assert_round_trips(&o, &root, "x.py", text);
    }

    #[test]
    fn a_python_class_docstring_moves_to_the_class_node() {
        let text = "\
class C:
    \"\"\"About C.\"\"\"

    def a(self):
        pass
";
        let (o, root) = import("x.py", text);
        let class = root.first_child(&o).unwrap();
        assert!(class.b(&o).contains("About C."), "{}", class.b(&o));
        assert_round_trips(&o, &root, "x.py", text);
    }

    #[test]
    fn a_python_module_preamble_stays_on_the_root() {
        let text = "\
\"\"\"A module.\"\"\"
import os


def f():
    pass
";
        let (o, root) = import("x.py", text);
        assert!(root.b(&o).contains("A module."), "{}", root.b(&o));
        assert_round_trips(&o, &root, "x.py", text);
    }

    #[test]
    fn a_string_containing_a_brace_does_not_confuse_c() {
        // Guide lines exist for exactly this: the '{' inside the string must
        // not open a block.
        let text = "\
int main(void) {
    printf(\"{\");
    return 0;
}
";
        let (o, root) = import("x.c", text);
        assert_eq!(tree(&o, &root), vec!["@auto x.c", "  func main"]);
        assert_round_trips(&o, &root, "x.c", text);
    }

    #[test]
    fn rust_lifetimes_are_not_string_delimiters() {
        let text = "\
fn f<'a>(x: &'a str) -> &'a str {
    x
}

fn g() -> u8 {
    b'x'
}
";
        let (o, root) = import("x.rs", text);
        assert_eq!(tree(&o, &root), vec!["@auto x.rs", "  fn f", "  fn g"]);
        assert_round_trips(&o, &root, "x.rs", text);
    }

    #[test]
    fn a_rust_raw_string_hides_its_braces() {
        let text = "\
fn f() {
    let s = r#\"} fn g() {\"#;
    println!(\"{s}\");
}
";
        let (o, root) = import("x.rs", text);
        assert_eq!(tree(&o, &root), vec!["@auto x.rs", "  fn f"]);
        assert_round_trips(&o, &root, "x.rs", text);
    }

    #[test]
    fn an_ini_file_splits_at_its_sections() {
        let text = "\
[one]
a = 1

[two]
b = 2
";
        let (o, root) = import("x.ini", text);
        assert_eq!(
            tree(&o, &root),
            vec!["@auto x.ini", "  section [one]", "  section [two]"]
        );
        assert_round_trips(&o, &root, "x.ini", text);
    }

    #[test]
    fn a_file_with_no_blocks_becomes_one_node() {
        let text = "just some prose\nand more of it\n";
        let (o, root) = import("x.py", text);
        assert_eq!(tree(&o, &root), vec!["@auto x.py"]);
        assert_round_trips(&o, &root, "x.py", text);
    }

    #[test]
    fn an_unknown_extension_has_no_importer() {
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "@auto x.unknown");
        let err = import_string(&mut o, &root, "text\n", "x.unknown").unwrap_err();
        assert!(err.contains("no @auto importer"), "{err}");
    }

    #[test]
    fn an_org_file_round_trips_through_its_own_writer() {
        let text = "\
* One
body one
** Two
body two
";
        let (o, root) = import("x.org", text);
        assert_eq!(tree(&o, &root), vec!["@auto x.org", "  One", "    Two"]);
        assert_round_trips(&o, &root, "x.org", text);
    }

    #[test]
    fn an_otl_file_splits_at_its_tab_depth() {
        // No round-trip assertion: Leo's otl reader recognizes a `: ` body
        // line only at column 0, while its writer indents one, so a nested
        // body line comes back as a node. This matches Leo exactly.
        let text = "\
One
: body one
\tTwo
\t: body two
";
        let (o, root) = import("x.otl", text);
        assert_eq!(
            tree(&o, &root),
            vec!["@auto x.otl", "  One", "    Two", "    : body two"]
        );
    }

    #[test]
    fn a_markdown_file_splits_at_its_headings() {
        let text = "\
# One

body one

## Two

body two
";
        let (o, root) = import("x.md", text);
        assert_eq!(tree(&o, &root), vec!["@auto x.md", "  One", "    Two"]);
        assert_round_trips(&o, &root, "x.md", text);
    }
}
