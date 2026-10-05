//! An outline plus its undo history and the structural commands.
//!
//! The commands live here rather than in a front end because none of them is
//! about display: "move this node left" is a fact about the tree. A view
//! supplies the current position and decides what to select afterwards.

use crate::error::Result;
use crate::external::{ReadResult, WriteResult};
use crate::node::{status, VnodeId};
use crate::outline::Outline;
use crate::position::Position;
use crate::undo::{Bead, Undoer};
use crate::{external, leofile, util};
use once_cell::sync::Lazy;
use regex::Regex;

/// `extractDef_patterns` in `commanderEditCommands.py`: definition lines in
/// Clojure, Python, JavaScript and CoffeeScript, the name in group 1.
static EXTRACT_DEF: Lazy<Vec<Regex>> = Lazy::new(|| {
    [
        r"\((?:def|defn|defui|deftype|defrecord|defonce)\s+(\S+)",
        r"^\s*(?:def|class)\s+(\w+)",
        r"^\bvar\s+(\w+)\s*=\s*function\b",
        r"^(?:export\s)?\s*function\s+(\w+)\s*\(",
        r"\b(\w+)\s*:\s*function\s",
        r"\.(\w+)\s*=\s*function\b",
        r"(?:export\s)?\b(\w+)\s*=\s(?:=>|->)",
        r"(?:export\s)?\b(\w+)\s*=\s(?:\([^)]*\))\s*(?:=>|->)",
        r"\b(\w+)\s*:\s(?:=>|->)",
        r"\b(\w+)\s*:\s(?:\([^)]*\))\s*(?:=>|->)",
    ]
    .iter()
    .map(|s| Regex::new(s).unwrap())
    .collect()
});

/// The name a definition line defines, as `extractDef`.
fn extract_def(s: &str) -> Option<String> {
    EXTRACT_DEF
        .iter()
        .find_map(|re| re.captures(s))
        .map(|m| m[1].to_string())
}

/// Whether s holds a section name, as `extractRef`: `<<` before `>>`, or
/// `@<` before `@>`.
fn extract_ref(s: &str) -> bool {
    [("<<", ">>"), ("@<", "@>")]
        .iter()
        .any(|(a, b)| matches!((s.find(a), s.find(b)), (Some(i), Some(j)) if i < j))
}

/// Where `Document::move_node` puts a node, relative to its target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// The target's previous sibling.
    Before,
    /// The target's next sibling.
    After,
    /// The target's last child.
    Inside,
}

/// An outline and its undo history: every edit through a `Document` method
/// is undoable.
pub struct Document {
    outline: Outline,
    undoer: Undoer,
    /// What reading the external files reported when this was opened.
    pub read_report: ReadResult,
    /// An unlinked tree waiting to be pasted, and whether it was cut.
    clipboard: Option<VnodeId>,
}

impl Document {
    /// The outline, to read.
    pub fn outline(&self) -> &Outline {
        &self.outline
    }

    /// The outline, to change outside the undo history: folds, an approved
    /// overwrite, a test's setup. An edit to a node made here cannot be
    /// undone, and the history may then name nodes that have changed.
    pub fn outline_mut_untracked(&mut self) -> &mut Outline {
        &mut self.outline
    }

    /// The undo history, to read.
    pub fn undoer(&self) -> &Undoer {
        &self.undoer
    }

    /// Open a group of edits that undo as one step. Pair with `end_group`.
    pub fn begin_group(&mut self, name: &str) {
        self.undoer.begin_group(name);
    }

    /// Close the group `begin_group` opened.
    pub fn end_group(&mut self) {
        self.undoer.end_group();
        self.free_dropped();
    }

    /// Forget the undo history.
    pub fn clear_undo(&mut self) {
        self.undoer.clear();
        self.free_dropped();
    }

    /// Keep at most `limit` undo steps.
    pub fn set_undo_limit(&mut self, limit: usize) {
        self.undoer.set_limit(limit);
        self.free_dropped();
    }

    /// Record one edit, then free what the beads it pushed off named.
    fn push(&mut self, name: &str, bead: Bead) {
        self.undoer.push(name, bead);
        self.free_dropped();
    }

    /// Free the vnodes only dropped beads named, in batches: each pass walks
    /// the whole arena.
    fn free_dropped(&mut self) {
        const BATCH: usize = 64;
        if self.undoer.dropped() < BATCH {
            return;
        }
        self.undoer.take_dropped();
        let mut keep = self.undoer.referenced();
        keep.extend(self.clipboard);
        self.outline.free_unreachable(&keep);
    }

    /// Wrap `outline` with an empty undo history.
    pub fn new(outline: Outline) -> Self {
        Self {
            outline,
            undoer: Undoer::new(),
            read_report: ReadResult::default(),
            clipboard: None,
        }
    }

    /// Open a `.leo` file, reading its external files unless told not to.
    ///
    /// Folds and marks come from the sidecar state file, since the `.leo`
    /// format does not carry them.
    pub fn open(path: &str, read_external: bool) -> Result<Self> {
        let (mut outline, report) = crate::open_outline_with_report(path, read_external)?;
        crate::state::load(&mut outline);
        let mut doc = Self::new(outline);
        doc.read_report = report;
        Ok(doc)
    }

    /// A document holding a new one-node outline named `file_name`.
    pub fn new_empty(file_name: &str) -> Self {
        Self::new(crate::new_outline(file_name))
    }

    // --- Editing content --------------------------------------------------

    /// Set p's headline, as one undo step. Newlines are removed.
    pub fn set_headline(&mut self, p: &Position, s: &str) {
        let old = p.h(&self.outline).to_string();
        let new = s.replace('\n', "");
        if old == new {
            return;
        }
        self.push(
            "rename-node",
            Bead::Headline {
                v: p.v,
                old,
                new: new.clone(),
            },
        );
        self.outline.set_headline(p, &new);
    }

    /// Set p's body, as one undo step. An unchanged body records nothing.
    pub fn set_body(&mut self, p: &Position, s: &str) {
        let old = p.b(&self.outline).to_string();
        if old == s {
            return;
        }
        self.push(
            "edit-body",
            Bead::Body {
                v: p.v,
                old,
                new: s.to_string(),
            },
        );
        self.outline.set_body(p, s);
    }

    /// Leo's `reformat-paragraph`: wrap the paragraph at or around line `row`
    /// of p's body to the `@pagewidth` in effect, as one undo step. Returns
    /// the row of the next paragraph, or `None` if there is no paragraph.
    pub fn reformat_paragraph(&mut self, p: &Position, row: usize) -> Option<usize> {
        let o = &self.outline;
        let (page_width, tab_width) = (o.get_page_width(p), o.get_tab_width(p));
        let (body, next) = crate::reformat::reformat_paragraph(p.b(o), row, page_width, tab_width)?;
        self.set_body(p, &body);
        Some(next)
    }

    /// Put `@first` on the `#!` and coding lines p's body starts with, so a
    /// sentinel file keeps them on lines 1 and 2. Returns how many.
    ///
    /// `import_at_file` does this; a node renamed to `@file` needs it too.
    pub fn add_first_directives(&mut self, p: &Position) -> usize {
        let lines = crate::util::split_lines(p.b(&self.outline));
        let n = external::first_lines(&lines);
        if n > 0 {
            let marked: String = lines[..n].iter().map(|l| format!("@first {l}")).collect();
            self.set_body(p, &format!("{marked}{}", lines[n..].concat()));
        }
        n
    }

    /// Mark or unmark p, as one undo step.
    pub fn toggle_marked(&mut self, p: &Position) {
        let was_marked = self.outline.node(p.v).is_marked();
        self.undoer
            .push("toggle-mark", Bead::Mark { v: p.v, was_marked });
        self.outline.toggle_marked(p);
    }

    // --- Editing the tree -------------------------------------------------

    /// Insert a node, as Leo's `insert-node`: as a first child of an expanded
    /// parent, otherwise as the next sibling.
    pub fn insert_node(&mut self, p: &Position) -> Position {
        let as_child = p.has_children(&self.outline) && self.outline.is_expanded(p);
        let new = if as_child {
            self.outline.insert_as_nth_child(p, 0)
        } else {
            self.outline.insert_after(p)
        };
        let parent = new.parent_vnode(&self.outline);
        self.push(
            "insert-node",
            Bead::Insert {
                parent,
                index: new.child_index,
                v: new.v,
            },
        );
        self.outline.changed = true;
        new
    }

    /// Delete p. Returns the position to select next.
    pub fn delete_node(&mut self, p: &Position) -> Option<Position> {
        let parent = p.parent_vnode(&self.outline);
        // The node a reader would land on: the previous *visible* one, so
        // deleting a node next to a folded tree does not jump into it.
        let next = p
            .vis_back(&self.outline)
            .or_else(|| p.next(&self.outline))
            .or_else(|| p.parent(&self.outline));
        self.push(
            "delete-node",
            Bead::Delete {
                parent,
                index: p.child_index,
                v: p.v,
            },
        );
        self.outline.delete_position(p);
        self.outline.changed = true;
        next.filter(|q| self.outline.position_exists(q))
            .or_else(|| self.outline.root_position())
    }

    /// Clone p, as Leo's `clone-node`. Returns the clone, p's next sibling.
    pub fn clone_node(&mut self, p: &Position) -> Position {
        let new = self.outline.clone_node(p);
        let parent = new.parent_vnode(&self.outline);
        self.push(
            "clone-node",
            Bead::Insert {
                parent,
                index: new.child_index,
                v: new.v,
            },
        );
        self.outline.changed = true;
        new
    }

    /// Insert a node as p's first child, as Leo's `insert-child`.
    pub fn insert_child(&mut self, p: &Position) -> Position {
        let new = self.outline.insert_as_nth_child(p, 0);
        let parent = new.parent_vnode(&self.outline);
        self.push(
            "insert-child",
            Bead::Insert {
                parent,
                index: new.child_index,
                v: new.v,
            },
        );
        self.outline.expand(p);
        self.outline.changed = true;
        new
    }

    /// Insert a node before p, as Leo's `insert-node-before`.
    pub fn insert_node_before(&mut self, p: &Position) -> Position {
        let new = self.outline.insert_before(p);
        let parent = new.parent_vnode(&self.outline);
        self.push(
            "insert-node-before",
            Bead::Insert {
                parent,
                index: new.child_index,
                v: new.v,
            },
        );
        self.outline.changed = true;
        new
    }

    /// Make all of p's following siblings its children.
    ///
    /// The mirror of `promote`. Leo expands p afterwards, because the nodes
    /// would otherwise appear to have been deleted.
    pub fn demote(&mut self, p: &Position) -> bool {
        let parent_v = p.parent_vnode(&self.outline);
        let following: Vec<VnodeId> =
            self.outline.node(parent_v).children[p.child_index + 1..].to_vec();
        if following.is_empty() {
            return false;
        }
        self.undoer.begin_group("demote");
        // Record each move so undo puts the siblings back in order.
        let base = self.outline.node(p.v).children.len();
        for (i, v) in following.iter().enumerate() {
            self.push(
                "demote",
                Bead::Move {
                    v: *v,
                    from: (parent_v, p.child_index + 1),
                    to: (p.v, base + i),
                },
            );
        }
        self.end_group();
        // Moves children directly, as `Outline::promote` does.
        self.outline.invalidate_descendent_uas(p.v);
        self.outline
            .node_mut(parent_v)
            .children
            .truncate(p.child_index + 1);
        for v in &following {
            self.outline.node_mut(p.v).children.push(*v);
            if let Some(i) = self
                .outline
                .node(*v)
                .parents
                .iter()
                .position(|x| *x == parent_v)
            {
                self.outline.node_mut(*v).parents.remove(i);
            }
            self.outline.node_mut(*v).parents.push(p.v);
        }
        self.outline.expand(p);
        self.outline.set_dirty(p);
        self.outline.changed = true;
        self.outline.generation += 1;
        true
    }

    /// Make p's children its following siblings, as Leo's `promote`.
    ///
    /// False, and nothing recorded, when p has no children.
    pub fn promote(&mut self, p: &Position) -> bool {
        let parent_v = p.parent_vnode(&self.outline);
        let children = self.outline.node(p.v).children.clone();
        if children.is_empty() {
            return false;
        }
        self.undoer.begin_group("promote");
        // Each move takes p's first child, so every bead starts at index 0.
        for (i, v) in children.iter().enumerate() {
            self.push(
                "promote",
                Bead::Move {
                    v: *v,
                    from: (p.v, 0),
                    to: (parent_v, p.child_index + 1 + i),
                },
            );
        }
        self.end_group();
        self.outline.promote(p);
        self.outline.set_dirty(p);
        self.outline.changed = true;
        true
    }

    /// Copy p to the clipboard, then delete it.
    pub fn cut_node(&mut self, p: &Position) -> Option<Position> {
        self.copy_node(p);
        self.delete_node(p)
    }

    /// Clear every mark in the outline. Returns how many were cleared.
    pub fn unmark_all(&mut self) -> usize {
        let marked: Vec<Position> = self
            .outline
            .all_unique_positions()
            .into_iter()
            .filter(|p| p.is_marked(&self.outline))
            .collect();
        if marked.is_empty() {
            return 0;
        }
        self.undoer.begin_group("unmark-all");
        for p in &marked {
            self.push(
                "unmark",
                Bead::Mark {
                    v: p.v,
                    was_marked: true,
                },
            );
            self.outline.node_mut(p.v).clear_bit(status::MARKED);
        }
        self.end_group();
        self.outline.changed = true;
        marked.len()
    }

    /// Leo's `sort-siblings`: order p's siblings by headline, ignoring case,
    /// keeping equal ones in their order. Returns p's new position, or None if
    /// they were already in order.
    pub fn sort_siblings(&mut self, p: &Position) -> Option<Position> {
        self.sort("sort-siblings", p)
    }

    /// Leo's `sort-children`: `sort_siblings` on p's children. Returns
    /// whether anything moved.
    pub fn sort_children(&mut self, p: &Position) -> bool {
        p.first_child(&self.outline)
            .and_then(|c| self.sort("sort-children", &c))
            .is_some()
    }

    fn sort(&mut self, name: &str, p: &Position) -> Option<Position> {
        let parent = p.parent_vnode(&self.outline);
        let old = self.outline.node(parent).children.clone();
        let mut new = old.clone();
        new.sort_by_cached_key(|v| self.outline.node(*v).h.to_lowercase());
        if new == old {
            return None;
        }
        let index = new.iter().position(|v| *v == p.v)?;
        crate::undo::set_children(&mut self.outline, parent, &new);
        self.push(name, Bead::Sort { parent, old, new });
        Some(Position::new(p.v, index, p.stack.clone()))
    }

    /// Leo's `extract`: move body lines `first..=last` of p into a new first
    /// child. Returns the child, or None if the lines are not in the body.
    ///
    /// The headline is a section reference on the first line, which stays in
    /// p's body; else a name a definition line defines; else the first line,
    /// which then leaves the body. The lines lose the first one's indent.
    pub fn extract(&mut self, p: &Position, first: usize, last: usize) -> Option<Position> {
        let old = p.b(&self.outline).to_string();
        let all: Vec<&str> = old.split_inclusive('\n').collect();
        if first > last || last >= all.len() {
            return None;
        }
        let tab_width = self.outline.get_tab_width(p);
        let (_, ws) = util::skip_leading_ws_with_indent(all[first], 0, tab_width);
        let lines: Vec<&str> = all[first..=last]
            .iter()
            .map(|s| util::remove_leading_whitespace(s, ws, tab_width))
            .collect();
        let head = lines[0].trim().to_string();
        // Leo looks only at the first line in these two languages.
        let language = self.outline.get_language(p).to_lowercase();
        let def_lines = if matches!(language.as_str(), "javascript" | "typescript") {
            &lines[..1]
        } else {
            &lines[..]
        };
        let (h, b, middle) = if extract_ref(&head) {
            let middle = format!("{}{}", " ".repeat(ws as usize), lines[0]);
            (head, lines[1..].concat(), middle)
        } else if let Some(name) = def_lines.iter().find_map(|l| extract_def(l.trim())) {
            (name, lines.concat(), String::new())
        } else {
            (head, lines[1..].concat(), String::new())
        };
        let new = format!(
            "{}{middle}{}",
            all[..first].concat(),
            all[last + 1..].concat()
        );
        self.undoer.begin_group("extract");
        let child = self.outline.insert_as_nth_child(p, 0);
        self.outline.set_headline(&child, &h);
        self.outline.set_body(&child, &b);
        self.push(
            "extract",
            Bead::Insert {
                parent: p.v,
                index: 0,
                v: child.v,
            },
        );
        self.set_body(p, &new);
        self.end_group();
        self.outline.expand(p);
        self.outline.changed = true;
        Some(child)
    }

    /// Leo's `clone-find-all` (`flatten` false) and `clone-find-all-flattened`.
    ///
    /// Clones each node `matches` accepts, once, under a new last top-level
    /// node headed `Found N:pattern`, sorted by headline. `@nosearch` and
    /// `@ignore` trees are skipped. Unflattened, a match's subtree is not
    /// searched. `status` names the search settings in the found node's body.
    /// Returns the found node and the count, or None if nothing matched.
    pub fn clone_find_all(
        &mut self,
        pattern: &str,
        status: &str,
        flatten: bool,
        matches: impl Fn(&Outline, &Position) -> bool,
    ) -> Option<(Position, usize)> {
        static NOSEARCH: Lazy<Regex> = Lazy::new(|| Regex::new(r"(^@|\n@)nosearch\b").unwrap());
        let o = &self.outline;
        let mut found: Vec<VnodeId> = Vec::new();
        let mut p = o.root_position();
        while let Some(cur) = p {
            // An ancestor that is @nosearch was skipped whole, so only cur
            // needs the test `g.inAtNosearch` makes of every ancestor.
            p = if cur.is_at_ignore_node(o) || NOSEARCH.is_match(cur.b(o)) {
                cur.node_after_tree(o)
            } else if matches(o, &cur) {
                if !found.contains(&cur.v) {
                    found.push(cur.v);
                }
                if flatten {
                    cur.thread_next(o)
                } else {
                    cur.node_after_tree(o)
                }
            } else {
                cur.thread_next(o)
            };
        }
        if found.is_empty() {
            return None;
        }
        let n = found.len();
        let mut last = o.root_position()?;
        while let Some(next) = last.next(o) {
            last = next;
        }
        found.sort_by_cached_key(|v| o.node(*v).h.to_lowercase());
        let organizer = self.outline.insert_after(&last);
        self.outline
            .set_headline(&organizer, &format!("Found {n}:{pattern}"));
        let flat = if flatten { "flattened, " } else { "" };
        self.outline.set_body(
            &organizer,
            &format!("@nosearch\n\n# {flat}{status}\n\n# found {n} nodes"),
        );
        for (i, v) in found.into_iter().enumerate() {
            self.outline.link_as_nth_child(&organizer, i, v);
        }
        // Undo unlinks the organizer, and its clones with it.
        self.push(
            "clone-find-all",
            Bead::Insert {
                parent: organizer.parent_vnode(&self.outline),
                index: organizer.child_index,
                v: organizer.v,
            },
        );
        self.outline.changed = true;
        Some((organizer, n))
    }

    /// Mark or unmark each of `positions`, as one undo step named `name`.
    /// Returns how many changed.
    fn set_marks(&mut self, name: &str, positions: &[Position], marked: bool) -> usize {
        let todo: Vec<VnodeId> = positions
            .iter()
            .filter(|p| p.is_marked(&self.outline) != marked)
            .map(|p| p.v)
            .collect();
        if todo.is_empty() {
            return 0;
        }
        self.undoer.begin_group(name);
        for &v in &todo {
            let was_marked = !marked;
            self.push(name, Bead::Mark { v, was_marked });
            if marked {
                self.outline.node_mut(v).set_bit(status::MARKED);
            } else {
                self.outline.node_mut(v).clear_bit(status::MARKED);
            }
        }
        self.end_group();
        self.outline.changed = true;
        todo.len()
    }

    /// Leo's `mark-subheads`: mark p's children. Returns how many changed.
    pub fn mark_subheads(&mut self, p: &Position) -> usize {
        let kids = p.children(&self.outline);
        self.set_marks("mark-subheads", &kids, true)
    }

    /// Leo's `mark-node-and-parents`.
    pub fn mark_node_and_parents(&mut self, p: &Position) -> usize {
        let line = p.self_and_parents(&self.outline);
        self.set_marks("mark-node-and-parents", &line, true)
    }

    /// Leo's `unmark-node-and-parents`.
    pub fn unmark_node_and_parents(&mut self, p: &Position) -> usize {
        let line = p.self_and_parents(&self.outline);
        self.set_marks("unmark-node-and-parents", &line, false)
    }

    /// The first marked position in outline order.
    fn first_marked(&self) -> Option<Position> {
        let o = &self.outline;
        let mut p = o.root_position();
        while let Some(cur) = p {
            if cur.is_marked(o) {
                return Some(cur);
            }
            p = cur.thread_next(o);
        }
        None
    }

    /// Each marked vnode once, in outline order, skipping those below
    /// another: they come with it.
    fn top_marked_vnodes(&self) -> Vec<VnodeId> {
        let o = &self.outline;
        let mut out: Vec<VnodeId> = Vec::new();
        let mut p = o.root_position();
        while let Some(cur) = p {
            if cur.is_marked(o) {
                if !out.contains(&cur.v) {
                    out.push(cur.v);
                }
                p = cur.node_after_tree(o);
            } else {
                p = cur.thread_next(o);
            }
        }
        out
    }

    /// Leo's `clone-marked-nodes`: a new node after p whose children are
    /// clones of the marked nodes. None if nothing is marked.
    pub fn clone_marked(&mut self, p: &Position) -> Option<Position> {
        self.gather_marked(p, "clone-marked-nodes", "Clones of marked nodes", false)
    }

    /// Leo's `copy-marked-nodes`: as `clone_marked`, with copies that keep
    /// their marks.
    pub fn copy_marked(&mut self, p: &Position) -> Option<Position> {
        self.gather_marked(p, "copy-marked-nodes", "Copies of marked nodes", true)
    }

    fn gather_marked(
        &mut self,
        p: &Position,
        name: &str,
        headline: &str,
        copy: bool,
    ) -> Option<Position> {
        let marked = self.top_marked_vnodes();
        if marked.is_empty() {
            return None;
        }
        self.undoer.begin_group(name);
        let parent = self.outline.insert_after(p);
        self.outline.set_headline(&parent, headline);
        self.push(
            name,
            Bead::Insert {
                parent: parent.parent_vnode(&self.outline),
                index: parent.child_index,
                v: parent.v,
            },
        );
        for (n, v) in marked.into_iter().enumerate() {
            let v = if copy {
                self.outline.copy_tree_with_marks(v)
            } else {
                v
            };
            self.outline.link_as_nth_child(&parent, n, v);
            self.push(
                name,
                Bead::Insert {
                    parent: parent.v,
                    index: n,
                    v,
                },
            );
        }
        self.end_group();
        self.outline.expand(&parent);
        self.outline.changed = true;
        Some(parent)
    }

    /// Leo's `move-marked-nodes`: move every marked node, with its tree, under
    /// a new node, which then goes after `current`'s node. Returns the new
    /// node and how many moved, or `None` if nothing is marked.
    ///
    /// One undo step, where Leo's cannot be undone.
    pub fn move_marked(&mut self, current: &Position) -> Option<(Position, usize)> {
        let root = self.outline.root_position()?;
        self.first_marked()?;
        let name = "move-marked-nodes";
        self.undoer.begin_group(name);
        // First at the top level, so no move shifts it.
        let parent = self.outline.insert_before(&root);
        self.outline.set_headline(&parent, "Moved marked nodes");
        self.push(
            name,
            Bead::Insert {
                parent: self.outline.hidden_root,
                index: 0,
                v: parent.v,
            },
        );
        let mut moved = 0;
        // Searching again after each move, as the positions found go stale.
        while let Some(p) = self.marked_outside(parent.v) {
            let n = self.outline.node(parent.v).children.len();
            self.move_to(&p, parent.v, n);
            moved += 1;
        }
        // After a position of `current`'s node outside the new tree, as Leo.
        let o = &self.outline;
        let after = o
            .all_positions()
            .into_iter()
            .find(|q| q.v == current.v && !q.self_and_parents(o).iter().any(|a| a.v == parent.v))
            .or_else(|| o.root_position()?.self_and_siblings(o).into_iter().last());
        let parent = match after {
            Some(q) if q.v != parent.v => {
                let to = q.parent_vnode(&self.outline);
                self.move_to(&parent, to, q.child_index + 1)
            }
            _ => parent,
        };
        self.end_group();
        self.outline.changed = true;
        Some((parent, moved))
    }

    /// The first marked position, in outline order, outside `v`'s tree.
    fn marked_outside(&self, v: VnodeId) -> Option<Position> {
        let o = &self.outline;
        let mut p = o.root_position();
        while let Some(cur) = p {
            if cur.v == v {
                p = cur.node_after_tree(o);
            } else if cur.is_marked(o) {
                return Some(cur);
            } else {
                p = cur.thread_next(o);
            }
        }
        None
    }

    /// Leo's `delete-marked-nodes`: delete every marked node, with its tree.
    /// Returns how many were deleted. The last top-level node is kept, as
    /// `delete_node` keeps it.
    ///
    /// One at a time, searching again after each: under a cloned parent one
    /// link shows at several positions, so positions found up front go stale.
    pub fn delete_marked(&mut self) -> usize {
        let mut n = 0;
        while let Some(p) = self.first_marked() {
            let o = &self.outline;
            if p.parent(o).is_none() && p.back(o).is_none() && p.next(o).is_none() {
                break;
            }
            if n == 0 {
                self.undoer.begin_group("delete-marked-nodes");
            }
            self.push(
                "delete-marked-nodes",
                Bead::Delete {
                    parent: p.parent_vnode(&self.outline),
                    index: p.child_index,
                    v: p.v,
                },
            );
            self.outline.delete_position(&p);
            n += 1;
        }
        if n > 0 {
            self.end_group();
            self.outline.changed = true;
        }
        n
    }

    /// Copy p's tree with fresh gnxs, ready to paste.
    pub fn copy_node(&mut self, p: &Position) {
        self.clipboard = Some(self.outline.copy_tree(p));
    }

    /// Paste the copied tree after p. A second paste copies it again, so the
    /// clipboard can be pasted any number of times.
    pub fn paste_node(&mut self, p: &Position) -> Option<Position> {
        let v = self.clipboard?;
        let copy = self.outline.copy_tree_of_vnode(v);
        let new = self.outline.paste_after(p, copy);
        let parent = new.parent_vnode(&self.outline);
        self.push(
            "paste-node",
            Bead::Insert {
                parent,
                index: new.child_index,
                v: new.v,
            },
        );
        self.outline.changed = true;
        Some(new)
    }

    /// Swap p with its previous sibling.
    pub fn move_up(&mut self, p: &Position) -> Option<Position> {
        let back = p.back(&self.outline)?;
        Some(self.move_to(p, back.parent_vnode(&self.outline), back.child_index))
    }

    /// Swap p with its next sibling: move p to just after it.
    pub fn move_down(&mut self, p: &Position) -> Option<Position> {
        let next = p.next(&self.outline)?;
        Some(self.move_to(p, next.parent_vnode(&self.outline), next.child_index + 1))
    }

    /// Make p the next sibling of its parent.
    pub fn move_left(&mut self, p: &Position) -> Option<Position> {
        let parent = p.parent(&self.outline)?;
        let grandparent = parent.parent_vnode(&self.outline);
        Some(self.move_to(p, grandparent, parent.child_index + 1))
    }

    /// Make p the last child of its previous sibling.
    pub fn move_right(&mut self, p: &Position) -> Option<Position> {
        let back = p.back(&self.outline)?;
        let n = back.num_children(&self.outline);
        Some(self.move_to(p, back.v, n))
    }

    /// Move p before, after or into `target`, as a dropped node lands. None
    /// if the place is inside p's own tree: the outline would hold itself.
    pub fn move_node(&mut self, p: &Position, target: &Position, place: Place) -> Option<Position> {
        let (parent, index) = match place {
            Place::Before => (target.parent_vnode(&self.outline), target.child_index),
            Place::After => (target.parent_vnode(&self.outline), target.child_index + 1),
            Place::Inside => (target.v, target.num_children(&self.outline)),
        };
        if parent == p.v || self.outline.is_below(parent, p.v) {
            return None;
        }
        Some(self.move_to(p, parent, index))
    }

    /// Unlink p and relink it as `parent`'s nth child.
    ///
    /// Removing p first shifts every later index under the same parent, so the
    /// target index is corrected here rather than by each caller.
    fn move_to(&mut self, p: &Position, parent: VnodeId, index: usize) -> Position {
        let from_parent = p.parent_vnode(&self.outline);
        let from_index = p.child_index;
        let index = if parent == from_parent && index > from_index {
            index - 1
        } else {
            index
        };
        self.outline
            .node_mut(from_parent)
            .children
            .remove(from_index);
        if let Some(i) = self
            .outline
            .node(p.v)
            .parents
            .iter()
            .position(|x| *x == from_parent)
        {
            self.outline.node_mut(p.v).parents.remove(i);
        }
        let n = index.min(self.outline.node(parent).children.len());
        self.outline.invalidate_descendent_uas(from_parent);
        self.outline.invalidate_descendent_uas(parent);
        self.outline.node_mut(parent).children.insert(n, p.v);
        self.outline.node_mut(p.v).parents.push(parent);
        self.outline.generation += 1;
        self.outline.changed = true;
        self.push(
            "move-node",
            Bead::Move {
                v: p.v,
                from: (from_parent, from_index),
                to: (parent, n),
            },
        );
        // Both trees, and through the `@<file>` node above each: setting the
        // bit on the node alone left `find_files_to_write` with nothing to
        // write, so a move inside an external tree never reached its file and
        // was gone on the next read. Undo already did this (`undo::relink`).
        self.outline.set_dirty_vnode(from_parent);
        self.outline.set_dirty_vnode(p.v);
        crate::undo::position_of(&self.outline, p.v).unwrap_or_else(|| p.clone())
    }

    // --- Undo -------------------------------------------------------------

    /// Undo one step. Returns the node to select, if it still exists.
    pub fn undo(&mut self) -> Option<Position> {
        self.undoer.undo(&mut self.outline)
    }

    /// Redo one step. Returns the node to select, if it still exists.
    pub fn redo(&mut self) -> Option<Position> {
        self.undoer.redo(&mut self.outline)
    }

    // --- Files ------------------------------------------------------------

    /// Write the `.leo` file only. [`Document::save_all`] also writes the external files.
    pub fn save(&mut self, path: &str) -> Result<String> {
        let written = leofile::write_leo_file(&mut self.outline, path)?;
        crate::state::save(&self.outline);
        Ok(written)
    }

    /// Save the `.leo` file, then write every dirty external file, as
    /// [`crate::save_all`]. No file is written if the `.leo` write fails.
    pub fn save_all(&mut self, path: &str) -> crate::SaveResult {
        let leo = self.save(path);
        let files = match leo {
            Ok(_) => self.write_external_files(true),
            Err(_) => WriteResult::default(),
        };
        let dropped_descendent_uas = match leo {
            Ok(_) => std::mem::take(&mut self.outline.dropped_descendent_uas),
            Err(_) => Vec::new(),
        };
        crate::SaveResult {
            leo,
            files,
            dropped_descendent_uas,
        }
    }

    /// Write a copy of the `.leo` file to `path`, as Leo's `save-to`.
    pub fn save_to(&mut self, path: &str) -> Result<String> {
        leofile::write_leo_copy(&mut self.outline, path)
    }

    /// Read `files` from disk again, replacing their trees.
    ///
    /// Not undoable, as in Leo: the read rebuilds the trees, and the history
    /// still names the nodes it replaced. The history is cleared.
    pub fn read_files(&mut self, files: Vec<Position>) -> ReadResult {
        let result = external::read_files(&mut self.outline, files);
        self.clear_undo();
        result
    }

    /// [`Document::read_files`], reading an `@clean` file even if its mtime
    /// says it is unchanged, as Leo's `refresh-from-disk`.
    pub fn refresh_files(&mut self, files: Vec<Position>) -> ReadResult {
        let result = external::refresh_files(&mut self.outline, files);
        self.clear_undo();
        result
    }

    /// Write every `@<file>` tree, or only the dirty ones if `dirty_only`.
    pub fn write_external_files(&mut self, dirty_only: bool) -> WriteResult {
        external::write_external_files(&mut self.outline, dirty_only)
    }

    /// Write the given `@<file>` nodes. See [`external::write_files`].
    pub fn write_files(&mut self, files: Vec<Position>) -> WriteResult {
        external::write_files(&mut self.outline, files)
    }

    /// Read every `@file`, `@clean` and `@edit` tree. The history is kept.
    pub fn read_external_files(&mut self) -> ReadResult {
        external::read_external_files(&mut self.outline)
    }

    /// Import the file at `path` as an `@file` tree, as one undoable step.
    ///
    /// The node goes after the outermost `@<file>` node at or above p, since
    /// `@<file>` nodes do not nest. Returns the node, and whether it still
    /// needs writing (see [`external::import_at_file`]).
    pub fn import_at_file(&mut self, p: &Position, path: &str) -> Result<(Position, bool)> {
        let was_changed = self.outline.changed;
        let new = self.new_file_node(p, path, "@file")?;
        let o = &mut self.outline;
        match external::import_at_file(o, &new) {
            Err(e) => {
                o.delete_position(&new);
                o.changed = was_changed;
                Err(e)
            }
            Ok(needs_write) => {
                let parent = new.parent_vnode(o);
                self.push(
                    "import-at-file",
                    Bead::Insert {
                        parent,
                        index: new.child_index,
                        v: new.v,
                    },
                );
                Ok((new, needs_write))
            }
        }
    }
}

impl Document {
    /// A new node headed `kind path`, not yet in the undo history, after the
    /// outermost `@<file>` node at or above p, since `@<file>` nodes do not
    /// nest. The path is relative to the outline's directory where it can
    /// be. An error if the outline already holds the file.
    fn new_file_node(&mut self, p: &Position, path: &str, kind: &str) -> Result<Position> {
        let abs = util::finalize_join(&[path]);
        let o = &self.outline;
        let existing = o
            .all_positions()
            .into_iter()
            .find(|q| q.is_any_at_file_node(o) && o.full_path(q) == abs);
        if let Some(q) = existing {
            return Err(crate::Error::Import {
                path: abs,
                detail: format!("already in the outline as {}", q.h(o)),
            });
        }
        let after = p
            .self_and_parents(o)
            .into_iter()
            .rev()
            .find(|q| q.is_any_at_file_node(o))
            .unwrap_or_else(|| p.clone());

        let o = &mut self.outline;
        let new = o.insert_after(&after);
        let dir = o.get_path(&new);
        let name = match std::path::Path::new(&abs).strip_prefix(&dir) {
            // An unsaved outline has no directory of its own to be relative to.
            Ok(rel) if !o.file_name.is_empty() => rel.to_string_lossy().to_string(),
            _ => abs.clone(),
        };
        o.set_headline(&new, &format!("{kind} {name}"));
        Ok(new)
    }

    /// Import the file at `path` as an `@auto` tree: Leo's importer splits
    /// it, and writing it back adds no sentinels. As any read, it clears the
    /// undo history. Returns the node and what the read reported.
    pub fn import_auto(&mut self, p: &Position, path: &str) -> Result<(Position, ReadResult)> {
        let new = self.new_file_node(p, path, "@auto")?;
        self.outline.expand(&new);
        let result = self.read_files(vec![new.clone()]);
        Ok((new, result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_imports_as_an_auto_tree_once() {
        let dir = tempfile::tempdir().unwrap();
        let py = dir.path().join("x.py");
        std::fs::write(&py, "def f():\n    return 1\n\ndef g():\n    return 2\n").unwrap();
        let leo = dir.path().join("x.leo");
        let mut d = Document::new_empty(leo.to_str().unwrap());
        let root = d.outline.root_position().unwrap();
        let (node, result) = d.import_auto(&root, py.to_str().unwrap()).unwrap();
        let o = &d.outline;
        assert!(result.errors.is_empty());
        assert_eq!(node.h(o), "@auto x.py");
        let children: Vec<&str> = node.children(o).iter().map(|c| c.h(o)).collect();
        assert_eq!(children, ["function: f", "function: g"]);
        let again = d.import_auto(&root, py.to_str().unwrap());
        assert!(matches!(again, Err(crate::Error::Import { .. })));
    }

    fn abc() -> (Document, Vec<Position>) {
        let mut d = Document::new_empty("");
        let root = d.outline.root_position().unwrap();
        d.set_headline(&root, "a");
        let b = d.outline.insert_after(&root);
        d.set_headline(&b, "b");
        let c = d.outline.insert_after(&b);
        d.set_headline(&c, "c");
        d.undoer.clear();
        let all = d.outline.all_positions();
        (d, all)
    }

    fn heads(d: &Document) -> Vec<String> {
        d.outline
            .all_positions()
            .iter()
            .map(|p| format!("{}{}", "  ".repeat(p.level()), p.h(&d.outline)))
            .collect()
    }

    /// a, b with child b1, c; `marked` names the nodes to mark.
    fn marked(marked: &[&str]) -> Document {
        let (mut d, all) = abc();
        let b1 = d.outline.insert_as_nth_child(&all[1], 0);
        d.set_headline(&b1, "b1");
        for p in d.outline.all_positions() {
            if marked.contains(&p.h(&d.outline)) {
                d.outline.toggle_marked(&p);
            }
        }
        d.undoer.clear();
        d
    }

    fn marks(d: &Document) -> Vec<String> {
        d.outline
            .all_positions()
            .iter()
            .filter(|p| p.is_marked(&d.outline))
            .map(|p| p.h(&d.outline).to_string())
            .collect()
    }

    #[test]
    fn clone_marked_gathers_each_top_marked_node_once_and_undoes_as_one() {
        // b1 comes with b, so it is not gathered on its own.
        let mut d = marked(&["b", "b1", "c"]);
        let a = d.outline.root_position().unwrap();
        let parent = d.clone_marked(&a).unwrap();
        assert_eq!(
            heads(&d),
            vec![
                "a",
                "Clones of marked nodes",
                "  b",
                "    b1",
                "  c",
                "b",
                "  b1",
                "c"
            ]
        );
        assert!(parent
            .first_child(&d.outline)
            .unwrap()
            .is_cloned(&d.outline));
        d.undo();
        assert_eq!(heads(&d), vec!["a", "b", "  b1", "c"]);
        d.redo();
        assert_eq!(heads(&d).len(), 8);
    }

    #[test]
    fn copy_marked_makes_new_nodes_that_keep_their_marks() {
        let mut d = marked(&["b"]);
        let a = d.outline.root_position().unwrap();
        let parent = d.copy_marked(&a).unwrap();
        let copy = parent.first_child(&d.outline).unwrap();
        assert!(!copy.is_cloned(&d.outline));
        assert_eq!(marks(&d), vec!["b", "b"]);
        assert_eq!(heads(&d)[1..4], ["Copies of marked nodes", "  b", "    b1"]);
    }

    #[test]
    fn nothing_marked_gathers_nothing() {
        let mut d = marked(&[]);
        let a = d.outline.root_position().unwrap();
        assert!(d.clone_marked(&a).is_none());
        assert!(!d.undoer.can_undo());
    }

    #[test]
    fn a_long_session_frees_what_undo_can_no_longer_reach() {
        let (mut d, all) = abc();
        // Three beads a round, so nine keep three whole rounds.
        d.set_undo_limit(9);
        for _ in 0..500 {
            let p = d.insert_node(&all[0]);
            d.set_body(&p, &"x".repeat(1000));
            d.delete_node(&p);
        }
        assert!(d.outline.node_count() < 100, "{}", d.outline.node_count());
        // The steps still kept undo.
        for _ in 0..9 {
            assert!(d.undoer.can_undo());
            d.undo();
        }
        assert!(!d.undoer.can_undo());
        assert_eq!(heads(&d), vec!["a", "b", "c"]);
        // A freed slot is reused with a gnx of its own.
        let p = d.insert_node(&all[0]);
        assert_eq!(d.outline.find_gnx(p.gnx(&d.outline)), Some(p.v));
    }

    #[test]
    fn move_marked_gathers_the_marked_trees_after_the_selection_and_undoes_as_one() {
        let mut d = marked(&["a", "b1", "c"]);
        let before = heads(&d);
        let b = d.outline.all_positions()[1].clone();
        let (parent, n) = d.move_marked(&b).unwrap();
        assert_eq!(n, 3);
        assert_eq!(parent.h(&d.outline), "Moved marked nodes");
        assert_eq!(
            heads(&d),
            vec!["b", "Moved marked nodes", "  a", "  b1", "  c"]
        );
        d.undo();
        assert_eq!(heads(&d), before);
        d.redo();
        assert_eq!(heads(&d)[1], "Moved marked nodes");
    }

    #[test]
    fn move_marked_from_a_marked_node_goes_last() {
        let mut d = marked(&["a"]);
        let a = d.outline.root_position().unwrap();
        d.move_marked(&a).unwrap();
        assert_eq!(
            heads(&d),
            vec!["b", "  b1", "c", "Moved marked nodes", "  a"]
        );
        assert!(marked(&[]).move_marked(&a).is_none());
    }

    #[test]
    fn delete_marked_takes_every_marked_tree_and_undoes_as_one() {
        let mut d = marked(&["a", "b1", "c"]);
        assert_eq!(d.delete_marked(), 3);
        assert_eq!(heads(&d), vec!["b"]);
        d.undo();
        assert_eq!(heads(&d), vec!["a", "b", "  b1", "c"]);
    }

    #[test]
    fn delete_marked_survives_a_marked_child_of_a_cloned_parent() {
        // b is cloned, so b1's one link shows at two positions.
        let mut d = marked(&["b1"]);
        let b = d.outline.all_positions()[1].clone();
        d.outline.clone_node(&b);
        assert_eq!(d.delete_marked(), 1);
        assert_eq!(heads(&d), vec!["a", "b", "b", "c"]);
        d.undo();
        assert_eq!(heads(&d), vec!["a", "b", "  b1", "b", "  b1", "c"]);
    }

    #[test]
    fn delete_marked_keeps_the_last_top_level_node() {
        let mut d = marked(&["a", "b", "c"]);
        assert_eq!(d.delete_marked(), 2);
        assert_eq!(heads(&d), vec!["c"]);
    }

    #[test]
    fn mark_commands_change_only_what_needs_it_as_one_undo() {
        let mut d = marked(&["b"]);
        let b1 = d.outline.all_positions()[2].clone();
        assert_eq!(d.mark_node_and_parents(&b1), 1);
        assert_eq!(marks(&d), vec!["b", "b1"]);
        assert_eq!(d.unmark_node_and_parents(&b1), 2);
        assert!(marks(&d).is_empty());
        d.undo();
        assert_eq!(marks(&d), vec!["b", "b1"]);
        let b = d.outline.all_positions()[1].clone();
        d.unmark_all();
        assert_eq!(d.mark_subheads(&b), 1);
        assert_eq!(marks(&d), vec!["b1"]);
    }

    #[test]
    fn sort_siblings_ignores_case_keeps_ties_in_order_and_undoes() {
        let (mut d, all) = abc();
        d.set_headline(&all[0], "b");
        d.set_headline(&all[1], "C");
        d.set_headline(&all[2], "B");
        d.undoer.clear();
        let first_b = all[0].v;
        let new = d.sort_siblings(&all[1]).unwrap();
        assert_eq!(heads(&d), vec!["b", "B", "C"]);
        assert_eq!(d.outline.all_positions()[0].v, first_b);
        assert_eq!(new.h(&d.outline), "C");
        assert!(d.outline.position_exists(&new));
        assert!(d.sort_siblings(&new).is_none());
        d.undo();
        assert_eq!(heads(&d), vec!["b", "C", "B"]);
        d.redo();
        assert_eq!(heads(&d), vec!["b", "B", "C"]);
    }

    #[test]
    fn sort_children_sorts_below_and_dirties_the_file() {
        let (mut d, all) = abc();
        d.set_headline(&all[0], "@file x.py");
        // Each is inserted first, so the children read z, y.
        for h in ["y", "z"] {
            let k = d.outline.insert_as_nth_child(&all[0], 0);
            d.set_headline(&k, h);
        }
        d.outline.node_mut(all[0].v).clear_bit(status::DIRTY);
        assert!(d.sort_children(&all[0]));
        assert_eq!(heads(&d)[1..3], ["  y", "  z"]);
        assert!(all[0].is_dirty(&d.outline));
        assert_eq!(d.undoer.undo_name(), Some("sort-children"));
    }

    fn body_of(d: &Document, p: &Position) -> String {
        p.b(&d.outline).to_string()
    }

    #[test]
    fn extract_a_section_keeps_the_reference_and_undoes_as_one() {
        // Leo's test_leoUndo.test_extract_test.
        let (mut d, all) = abc();
        let before = "before\n    << section >>\n    sec line 1\n        sec line 2 indented\nsec line 3\nafter\n";
        d.set_body(&all[0], before);
        d.undoer.clear();
        let child = d.extract(&all[0], 1, 4).unwrap();
        assert_eq!(body_of(&d, &all[0]), "before\n    << section >>\nafter\n");
        assert_eq!(child.h(&d.outline), "<< section >>");
        assert_eq!(
            body_of(&d, &child),
            "sec line 1\n    sec line 2 indented\nsec line 3\n"
        );
        d.undo();
        assert_eq!(body_of(&d, &all[0]), before);
        assert_eq!(heads(&d), vec!["a", "b", "c"]);
        d.redo();
        assert_eq!(body_of(&d, &all[0]), "before\n    << section >>\nafter\n");
        d.undo();
        assert_eq!(body_of(&d, &all[0]), before);
    }

    #[test]
    fn extract_names_a_definition_and_keeps_every_line() {
        let (mut d, all) = abc();
        d.set_body(
            &all[0],
            "x = 1\n    # helper\n    def spam(a):\n        return a\n",
        );
        let child = d.extract(&all[0], 1, 3).unwrap();
        assert_eq!(child.h(&d.outline), "spam");
        assert_eq!(
            body_of(&d, &child),
            "# helper\ndef spam(a):\n    return a\n"
        );
        assert_eq!(body_of(&d, &all[0]), "x = 1\n");
    }

    #[test]
    fn extract_otherwise_heads_the_child_with_the_first_line() {
        let (mut d, all) = abc();
        d.set_body(&all[0], "title\nline\n");
        let child = d.extract(&all[0], 0, 1).unwrap();
        assert_eq!(child.h(&d.outline), "title");
        assert_eq!(body_of(&d, &child), "line\n");
        assert_eq!(body_of(&d, &all[0]), "");
        assert!(d.extract(&all[0], 0, 0).is_none());
    }

    fn has_b(o: &Outline, p: &Position) -> bool {
        p.h(o).contains('b')
    }

    #[test]
    fn clone_find_all_skips_a_match_subtree_unless_flattened() {
        let mut d = marked(&[]);
        let (found, n) = d.clone_find_all("b", "Head", false, has_b).unwrap();
        assert_eq!(n, 1);
        assert_eq!(found.h(&d.outline), "Found 1:b");
        assert!(found.b(&d.outline).starts_with("@nosearch\n"));
        assert_eq!(
            heads(&d),
            vec!["a", "b", "  b1", "c", "Found 1:b", "  b", "    b1"]
        );
        d.undo();
        assert_eq!(heads(&d), vec!["a", "b", "  b1", "c"]);
        d.redo();
        assert_eq!(heads(&d).len(), 7);
        d.undo();
        let (_, n) = d.clone_find_all("b", "Head", true, has_b).unwrap();
        assert_eq!(n, 2);
        assert_eq!(heads(&d)[4..], ["Found 2:b", "  b", "    b1", "  b1"]);
    }

    #[test]
    fn clone_find_all_skips_nosearch_trees_and_its_own_results() {
        let mut d = marked(&[]);
        let b = d.outline.all_positions()[1].clone();
        d.set_body(&b, "text\n@nosearch\n");
        let c = d.outline.all_positions()[3].clone();
        d.set_headline(&c, "cb");
        let (_, n) = d.clone_find_all("b", "Head", false, has_b).unwrap();
        assert_eq!(n, 1);
        // The first organizer is @nosearch, so its clones are not found again.
        let (_, n) = d.clone_find_all("b", "Head", false, has_b).unwrap();
        assert_eq!(n, 1);
        assert!(d
            .clone_find_all("zzz", "Head", false, |_, _| false)
            .is_none());
    }

    #[test]
    fn move_node_lands_before_after_or_inside_and_undoes() {
        let (mut d, all) = abc();
        let c = d.move_node(&all[2], &all[0], Place::Before).unwrap();
        assert_eq!(heads(&d), vec!["c", "a", "b"]);
        let all = d.outline.all_positions();
        d.move_node(&c, &all[2], Place::After);
        assert_eq!(heads(&d), vec!["a", "b", "c"]);
        let all = d.outline.all_positions();
        d.move_node(&all[0], &all[1], Place::Inside);
        assert_eq!(heads(&d), vec!["b", "  a", "c"]);
        d.undo();
        assert_eq!(heads(&d), vec!["a", "b", "c"]);
    }

    #[test]
    fn move_node_refuses_a_place_inside_the_moved_tree() {
        let (mut d, all) = abc();
        let b = d.move_right(&all[1]).unwrap();
        let a = d.outline.all_positions()[0].clone();
        assert!(d.move_node(&a, &a, Place::Inside).is_none());
        assert!(d.move_node(&a, &b, Place::Inside).is_none());
        assert!(d.move_node(&a, &b, Place::After).is_none());
        assert_eq!(heads(&d), vec!["a", "  b", "c"]);
    }

    #[test]
    fn move_down_swaps_with_the_next_sibling() {
        let (mut d, all) = abc();
        d.move_down(&all[0]);
        assert_eq!(heads(&d), vec!["b", "a", "c"]);
    }

    #[test]
    fn move_up_swaps_with_the_previous_sibling() {
        let (mut d, all) = abc();
        d.move_up(&all[2]);
        assert_eq!(heads(&d), vec!["a", "c", "b"]);
    }

    #[test]
    fn move_right_makes_a_child_of_the_previous_sibling() {
        let (mut d, all) = abc();
        d.move_right(&all[1]);
        assert_eq!(heads(&d), vec!["a", "  b", "c"]);
    }

    #[test]
    fn move_left_makes_a_sibling_of_the_parent() {
        let (mut d, all) = abc();
        let b = d.move_right(&all[1]).unwrap();
        d.move_left(&b);
        assert_eq!(heads(&d), vec!["a", "b", "c"]);
    }

    #[test]
    fn every_move_undoes() {
        let (mut d, all) = abc();
        let before = heads(&d);
        let b = d.move_right(&all[1]).unwrap();
        d.move_down(&all[0]);
        d.move_left(&b);
        while d.undoer.can_undo() {
            d.undo();
        }
        assert_eq!(heads(&d), before);
    }

    #[test]
    fn paste_makes_an_independent_copy() {
        let (mut d, all) = abc();
        d.copy_node(&all[0]);
        let pasted = d.paste_node(&all[2]).unwrap();
        assert_eq!(heads(&d), vec!["a", "b", "c", "a"]);
        assert_ne!(pasted.gnx(&d.outline), all[0].gnx(&d.outline));
        d.set_headline(&pasted, "copy");
        assert_eq!(all[0].h(&d.outline), "a");
    }

    #[test]
    fn demote_makes_following_siblings_children() {
        let (mut d, all) = abc();
        assert!(d.demote(&all[0]));
        assert_eq!(heads(&d), vec!["a", "  b", "  c"]);
        d.undo();
        assert_eq!(heads(&d), vec!["a", "b", "c"]);
    }

    #[test]
    fn demote_with_no_following_siblings_does_nothing() {
        let (mut d, all) = abc();
        assert!(!d.demote(&all[2]));
        assert_eq!(heads(&d), vec!["a", "b", "c"]);
    }

    #[test]
    fn promote_is_the_inverse_of_demote() {
        let (mut d, all) = abc();
        d.demote(&all[0]);
        assert!(d.promote(&all[0]));
        assert_eq!(heads(&d), vec!["a", "b", "c"]);
    }

    #[test]
    fn promote_undoes_redoes_and_marks_the_outline_changed() {
        let (mut d, all) = abc();
        d.demote(&all[0]);
        d.undoer.clear();
        d.outline.changed = false;
        assert!(d.promote(&all[0]));
        assert!(d.outline.changed);
        assert_eq!(heads(&d), vec!["a", "b", "c"]);
        d.undo();
        assert_eq!(heads(&d), vec!["a", "  b", "  c"]);
        d.redo();
        assert_eq!(heads(&d), vec!["a", "b", "c"]);
        assert!(!d.promote(&all[2]));
    }

    #[test]
    fn insert_child_goes_first_and_unfolds_the_parent() {
        let (mut d, all) = abc();
        let child = d.insert_child(&all[0]);
        d.set_headline(&child, "child");
        assert_eq!(heads(&d), vec!["a", "  child", "b", "c"]);
        assert!(d.outline.is_expanded(&all[0]));
    }

    #[test]
    fn unmark_all_clears_every_mark_as_one_undo() {
        let (mut d, all) = abc();
        d.toggle_marked(&all[0]);
        d.toggle_marked(&all[2]);
        assert_eq!(d.unmark_all(), 2);
        assert!(!all[0].is_marked(&d.outline));
        d.undo();
        assert!(all[0].is_marked(&d.outline));
        assert!(all[2].is_marked(&d.outline));
    }

    #[test]
    fn deleting_the_last_node_still_leaves_a_selection() {
        let (mut d, all) = abc();
        let next = d.delete_node(&all[2]).unwrap();
        assert_eq!(next.h(&d.outline), "b");
    }
}
