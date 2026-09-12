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

pub struct Document {
    pub outline: Outline,
    pub undoer: Undoer,
    /// What reading the external files reported when this was opened.
    pub read_report: ReadResult,
    /// An unlinked tree waiting to be pasted, and whether it was cut.
    clipboard: Option<VnodeId>,
}

impl Document {
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

    pub fn new_empty(file_name: &str) -> Self {
        Self::new(crate::new_outline(file_name))
    }

    // --- Editing content --------------------------------------------------

    pub fn set_headline(&mut self, p: &Position, s: &str) {
        let old = p.h(&self.outline).to_string();
        let new = s.replace('\n', "");
        if old == new {
            return;
        }
        self.undoer.push(
            "rename-node",
            Bead::Headline {
                v: p.v,
                old,
                new: new.clone(),
            },
        );
        self.outline.set_headline(p, &new);
    }

    pub fn set_body(&mut self, p: &Position, s: &str) {
        let old = p.b(&self.outline).to_string();
        if old == s {
            return;
        }
        self.undoer.push(
            "edit-body",
            Bead::Body {
                v: p.v,
                old,
                new: s.to_string(),
            },
        );
        self.outline.set_body(p, s);
    }

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
        self.undoer.push(
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
        self.undoer.push(
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

    pub fn clone_node(&mut self, p: &Position) -> Position {
        let new = self.outline.clone_node(p);
        let parent = new.parent_vnode(&self.outline);
        self.undoer.push(
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
        self.undoer.push(
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
        self.undoer.push(
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
            self.undoer.push(
                "demote",
                Bead::Move {
                    v: *v,
                    from: (parent_v, p.child_index + 1),
                    to: (p.v, base + i),
                },
            );
        }
        self.undoer.end_group();
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
            self.undoer.push(
                "unmark",
                Bead::Mark {
                    v: p.v,
                    was_marked: true,
                },
            );
            self.outline.node_mut(p.v).clear_bit(status::MARKED);
        }
        self.undoer.end_group();
        self.outline.changed = true;
        marked.len()
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
        self.undoer.push(
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
        self.outline.node_mut(parent).children.insert(n, p.v);
        self.outline.node_mut(p.v).parents.push(parent);
        self.outline.generation += 1;
        self.outline.changed = true;
        self.undoer.push(
            "move-node",
            Bead::Move {
                v: p.v,
                from: (from_parent, from_index),
                to: (parent, n),
            },
        );
        self.outline.node_mut(p.v).set_bit(status::DIRTY);
        crate::undo::position_of(&self.outline, p.v).unwrap_or_else(|| p.clone())
    }

    // --- Undo -------------------------------------------------------------

    pub fn undo(&mut self) -> Option<Position> {
        self.undoer.undo(&mut self.outline)
    }

    pub fn redo(&mut self) -> Option<Position> {
        self.undoer.redo(&mut self.outline)
    }

    // --- Files ------------------------------------------------------------

    /// Write the `.leo` file. External files are a separate command.
    pub fn save(&mut self, path: &str) -> Result<String> {
        let written = leofile::write_leo_file(&mut self.outline, path)?;
        crate::state::save(&self.outline);
        Ok(written)
    }

    pub fn write_external_files(&mut self, dirty_only: bool) -> WriteResult {
        external::write_external_files(&mut self.outline, dirty_only)
    }

    pub fn write_files(&mut self, files: Vec<Position>) -> WriteResult {
        external::write_files(&mut self.outline, files)
    }

    pub fn read_external_files(&mut self) -> ReadResult {
        external::read_external_files(&mut self.outline)
    }

    /// Import the file at `path` as an `@file` tree, as one undoable step.
    ///
    /// The node goes after the outermost `@<file>` node at or above p, since
    /// `@<file>` nodes do not nest. Returns the node, and whether it still
    /// needs writing (see [`external::import_at_file`]).
    pub fn import_at_file(&mut self, p: &Position, path: &str) -> Result<(Position, bool)> {
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
        let was_changed = o.changed;
        let new = o.insert_after(&after);
        let dir = o.get_path(&new);
        let name = match std::path::Path::new(&abs).strip_prefix(&dir) {
            // An unsaved outline has no directory of its own to be relative to.
            Ok(rel) if !o.file_name.is_empty() => rel.to_string_lossy().to_string(),
            _ => abs.clone(),
        };
        o.set_headline(&new, &format!("@file {name}"));
        match external::import_at_file(o, &new) {
            Err(e) => {
                o.delete_position(&new);
                o.changed = was_changed;
                Err(e)
            }
            Ok(needs_write) => {
                let parent = new.parent_vnode(o);
                self.undoer.push(
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

#[cfg(test)]
mod tests {
    use super::*;

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
        d.outline.promote(&all[0]);
        assert_eq!(heads(&d), vec!["a", "b", "c"]);
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
