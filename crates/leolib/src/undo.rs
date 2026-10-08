//! One undo history per outline.
//!
//! The history belongs to the document, not to whichever view made the edit:
//! two windows on one outline share one stack, and undo in either must move
//! the other. Beads record the inverse of an operation rather than a snapshot
//! of the tree, so the cost of an edit does not grow with the outline.
//!
//! Deleting a node keeps its vnode while a bead names it, which is what makes
//! undoing a delete a matter of relinking rather than rebuilding. The stack
//! holds [`DEFAULT_LIMIT`] beads; once older ones drop off, the vnodes only
//! they named are freed (`Document`).

use crate::node::{status, VnodeId};
use crate::outline::Outline;
use crate::position::Position;

/// The inverse of one edit.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Bead {
    /// A body was set.
    Body {
        /// The node edited.
        v: VnodeId,
        /// The body before the edit.
        old: String,
        /// The body after the edit.
        new: String,
    },
    /// A headline was set.
    Headline {
        /// The node edited.
        v: VnodeId,
        /// The headline before the edit.
        old: String,
        /// The headline after the edit.
        new: String,
    },
    /// A node was linked in. Undo cuts the link.
    Insert {
        /// The node `v` was linked under.
        parent: VnodeId,
        /// `v`'s child index in `parent`.
        index: usize,
        /// The node inserted.
        v: VnodeId,
    },
    /// A node was unlinked. Undo puts it back where it was.
    Delete {
        /// The node `v` was unlinked from.
        parent: VnodeId,
        /// `v`'s child index in `parent` before the delete.
        index: usize,
        /// The node deleted.
        v: VnodeId,
    },
    /// A node was moved to another parent or child index.
    Move {
        /// The node moved.
        v: VnodeId,
        /// Parent and child index before the move.
        from: (VnodeId, usize),
        /// Parent and child index after the move.
        to: (VnodeId, usize),
    },
    /// A node was marked or unmarked.
    Mark {
        /// The node toggled.
        v: VnodeId,
        /// The mark before the edit.
        was_marked: bool,
    },
    /// A parent's children were put in another order.
    Sort {
        /// The node whose children were sorted.
        parent: VnodeId,
        /// The children before the sort.
        old: Vec<VnodeId>,
        /// The children after the sort.
        new: Vec<VnodeId>,
    },
    /// Several edits that undo as one, such as a paste or a demote.
    Group {
        /// The group's operation name.
        name: String,
        /// The edits, in the order made.
        beads: Vec<Bead>,
    },
}

/// How many beads the stack keeps, as vim's default `undolevels`.
pub const DEFAULT_LIMIT: usize = 1000;

/// The undo stack, with the name of each operation for a menu or status line.
#[derive(Debug)]
pub struct Undoer {
    beads: Vec<(String, Bead)>,
    /// How many beads have been applied. Redo replays from here.
    index: usize,
    /// Beads being collected into one group, innermost last.
    open_groups: Vec<(String, Vec<Bead>)>,
    /// The most beads kept; the oldest go first.
    limit: usize,
    /// Beads dropped since the vnodes they named were last freed.
    dropped: usize,
    /// Counts every operation recorded, undone, redone or cleared.
    version: u64,
}

impl Default for Undoer {
    fn default() -> Self {
        Self {
            beads: Vec::new(),
            index: 0,
            open_groups: Vec::new(),
            limit: DEFAULT_LIMIT,
            dropped: 0,
            version: 0,
        }
    }
}

impl Undoer {
    /// An empty history keeping [`DEFAULT_LIMIT`] beads.
    pub fn new() -> Self {
        Self::default()
    }

    /// Keep at most `limit` beads, at least one.
    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit.max(1);
        self.trim();
    }

    /// Drop the oldest beads beyond the limit.
    fn trim(&mut self) {
        let over = self.beads.len().saturating_sub(self.limit);
        self.beads.drain(..over);
        self.index = self.index.saturating_sub(over);
        self.dropped += over;
    }

    /// How many beads have dropped off since `take_dropped` last ran.
    pub(crate) fn dropped(&self) -> usize {
        self.dropped
    }

    pub(crate) fn take_dropped(&mut self) -> usize {
        std::mem::take(&mut self.dropped)
    }

    /// Every vnode a bead still names, to keep when freeing the rest.
    pub(crate) fn referenced(&self) -> Vec<VnodeId> {
        fn walk(bead: &Bead, out: &mut Vec<VnodeId>) {
            match bead {
                Bead::Body { v, .. } | Bead::Headline { v, .. } | Bead::Mark { v, .. } => {
                    out.push(*v)
                }
                Bead::Insert { parent, v, .. } | Bead::Delete { parent, v, .. } => {
                    out.extend([*parent, *v])
                }
                Bead::Move { v, from, to } => out.extend([*v, from.0, to.0]),
                Bead::Sort { parent, old, new } => {
                    out.push(*parent);
                    out.extend(old.iter().chain(new));
                }
                Bead::Group { beads, .. } => beads.iter().for_each(|b| walk(b, out)),
            }
        }
        let mut out = Vec::new();
        let open = self.open_groups.iter().flat_map(|(_, beads)| beads);
        for bead in self.beads.iter().map(|(_, b)| b).chain(open) {
            walk(bead, &mut out);
        }
        out
    }

    /// A number that changes whenever the history does: an operation
    /// recorded, undone, redone, or the history cleared. A caller that saw
    /// one value knows the outline has not been edited since.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// True if there is an operation to undo.
    pub fn can_undo(&self) -> bool {
        self.index > 0
    }

    /// True if there is an undone operation to redo.
    pub fn can_redo(&self) -> bool {
        self.index < self.beads.len()
    }

    /// Name of the operation [`Undoer::undo`] would reverse.
    pub fn undo_name(&self) -> Option<&str> {
        self.beads
            .get(self.index.checked_sub(1)?)
            .map(|b| b.0.as_str())
    }

    /// Name of the operation [`Undoer::redo`] would replay.
    pub fn redo_name(&self) -> Option<&str> {
        self.beads.get(self.index).map(|b| b.0.as_str())
    }

    /// Start collecting beads into one undoable operation.
    pub fn begin_group(&mut self, name: &str) {
        self.open_groups.push((name.to_string(), Vec::new()));
    }

    /// Close the innermost group. An empty group records nothing.
    pub fn end_group(&mut self) {
        let Some((name, beads)) = self.open_groups.pop() else {
            return;
        };
        if beads.is_empty() {
            return;
        }
        self.push(&name.clone(), Bead::Group { name, beads });
    }

    /// Record one edit. Anything after the current point becomes unreachable.
    pub fn push(&mut self, name: &str, bead: Bead) {
        if let Some((_, group)) = self.open_groups.last_mut() {
            group.push(bead);
            return;
        }
        self.beads.truncate(self.index);
        self.beads.push((name.to_string(), bead));
        self.index = self.beads.len();
        self.version += 1;
        self.trim();
    }

    /// Drop every bead and any open group.
    pub fn clear(&mut self) {
        self.dropped += self.beads.len();
        self.beads.clear();
        self.index = 0;
        self.open_groups.clear();
        self.version += 1;
    }

    /// Undo one operation. Returns the node to select, if it still exists.
    pub fn undo(&mut self, o: &mut Outline) -> Option<Position> {
        if !self.can_undo() {
            return None;
        }
        self.index -= 1;
        self.version += 1;
        let bead = self.beads[self.index].1.clone();
        apply(o, &bead, true)
    }

    /// Redo one operation. Returns the node to select, if it still exists.
    pub fn redo(&mut self, o: &mut Outline) -> Option<Position> {
        if !self.can_redo() {
            return None;
        }
        let bead = self.beads[self.index].1.clone();
        self.index += 1;
        self.version += 1;
        apply(o, &bead, false)
    }
}

fn apply(o: &mut Outline, bead: &Bead, undo: bool) -> Option<Position> {
    match bead {
        Bead::Body { v, old, new } => {
            o.node_mut(*v).b = if undo { old.clone() } else { new.clone() };
            touch(o, *v)
        }
        Bead::Headline { v, old, new } => {
            o.node_mut(*v).h = if undo { old.clone() } else { new.clone() };
            touch(o, *v)
        }
        Bead::Insert { parent, index, v } => {
            if undo {
                unlink(o, *parent, *index, *v);
                position_of(o, *parent)
            } else {
                relink(o, *parent, *index, *v);
                position_of(o, *v)
            }
        }
        Bead::Delete { parent, index, v } => {
            if undo {
                relink(o, *parent, *index, *v);
                position_of(o, *v)
            } else {
                unlink(o, *parent, *index, *v);
                position_of(o, *parent)
            }
        }
        Bead::Move { v, from, to } => {
            let (src, dst) = if undo { (to, from) } else { (from, to) };
            unlink(o, src.0, src.1, *v);
            relink(o, dst.0, dst.1, *v);
            position_of(o, *v)
        }
        Bead::Sort { parent, old, new } => {
            set_children(o, *parent, if undo { old } else { new });
            position_of(o, *parent)
        }
        Bead::Mark { v, was_marked } => {
            let marked = if undo { *was_marked } else { !*was_marked };
            if marked {
                o.node_mut(*v).set_bit(status::MARKED);
            } else {
                o.node_mut(*v).clear_bit(status::MARKED);
            }
            position_of(o, *v)
        }
        Bead::Group { beads, .. } => {
            // Undo runs the group backwards; redo runs it forwards.
            let mut last = None;
            if undo {
                for b in beads.iter().rev() {
                    last = apply(o, b, true).or(last);
                }
            } else {
                for b in beads.iter() {
                    last = apply(o, b, false).or(last);
                }
            }
            last
        }
    }
}

fn touch(o: &mut Outline, v: VnodeId) -> Option<Position> {
    o.changed = true;
    o.generation += 1;
    let p = position_of(o, v)?;
    o.set_dirty(&p);
    Some(p)
}

fn unlink(o: &mut Outline, parent: VnodeId, index: usize, v: VnodeId) {
    if o.node(parent).children.get(index) != Some(&v) {
        return; // The tree moved under us; do nothing rather than corrupt it.
    }
    // The tree losing the node must be written again, as must the one gaining it.
    o.set_dirty_vnode(parent);
    o.invalidate_descendent_uas(parent);
    o.node_mut(parent).children.remove(index);
    if let Some(i) = o.node(v).parents.iter().position(|x| *x == parent) {
        o.node_mut(v).parents.remove(i);
    }
    o.changed = true;
    o.generation += 1;
}

fn relink(o: &mut Outline, parent: VnodeId, index: usize, v: VnodeId) {
    let n = index.min(o.node(parent).children.len());
    o.invalidate_descendent_uas(parent);
    o.node_mut(parent).children.insert(n, v);
    o.node_mut(v).parents.push(parent);
    o.set_dirty_vnode(v);
    o.changed = true;
    o.generation += 1;
}

/// Put `parent`'s children in `order`, the same vnodes in another order.
pub(crate) fn set_children(o: &mut Outline, parent: VnodeId, order: &[VnodeId]) {
    o.invalidate_descendent_uas(parent);
    o.node_mut(parent).children = order.to_vec();
    // The order is the `@others` order, so the file must be written again.
    if parent != o.hidden_root {
        o.set_dirty_vnode(parent);
    }
    o.changed = true;
    o.generation += 1;
}

/// The first position holding `v`, or None if it is no longer in the tree.
pub fn position_of(o: &Outline, v: VnodeId) -> Option<Position> {
    if v == o.hidden_root {
        return o.root_position();
    }
    o.all_positions().into_iter().find(|p| p.v == v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Outline;

    #[test]
    fn undo_and_redo_restore_a_headline() {
        let mut o = Outline::new_empty();
        let mut u = Undoer::new();
        let root = o.root_position().unwrap();
        let old = root.h(&o).to_string();
        u.push(
            "rename",
            Bead::Headline {
                v: root.v,
                old: old.clone(),
                new: "changed".to_string(),
            },
        );
        o.set_headline(&root, "changed");
        u.undo(&mut o);
        assert_eq!(o.root_position().unwrap().h(&o), old);
        u.redo(&mut o);
        assert_eq!(o.root_position().unwrap().h(&o), "changed");
    }

    #[test]
    fn undoing_a_delete_puts_the_subtree_back() {
        let mut o = Outline::new_empty();
        let mut u = Undoer::new();
        let root = o.root_position().unwrap();
        let child = o.insert_as_last_child(&root);
        o.set_headline(&child, "child");
        let grandchild = o.insert_as_last_child(&child);
        o.set_headline(&grandchild, "grandchild");
        u.push(
            "delete-node",
            Bead::Delete {
                parent: root.v,
                index: 0,
                v: child.v,
            },
        );
        o.delete_position(&child);
        assert_eq!(o.all_positions().len(), 1);
        u.undo(&mut o);
        let heads: Vec<String> = o
            .all_positions()
            .iter()
            .map(|p| p.h(&o).to_string())
            .collect();
        assert_eq!(heads, vec!["newHeadline", "child", "grandchild"]);
    }

    #[test]
    fn a_group_undoes_as_one_operation() {
        let mut o = Outline::new_empty();
        let mut u = Undoer::new();
        let root = o.root_position().unwrap();
        u.begin_group("edit both");
        u.push(
            "",
            Bead::Headline {
                v: root.v,
                old: "newHeadline".to_string(),
                new: "a".to_string(),
            },
        );
        u.push(
            "",
            Bead::Body {
                v: root.v,
                old: String::new(),
                new: "b\n".to_string(),
            },
        );
        u.end_group();
        o.set_headline(&root, "a");
        o.set_body(&root, "b\n");
        assert_eq!(u.undo_name(), Some("edit both"));
        u.undo(&mut o);
        let root = o.root_position().unwrap();
        assert_eq!(root.h(&o), "newHeadline");
        assert_eq!(root.b(&o), "");
    }

    #[test]
    fn a_new_edit_discards_the_redo_stack() {
        let mut o = Outline::new_empty();
        let mut u = Undoer::new();
        let root = o.root_position().unwrap();
        u.push(
            "one",
            Bead::Headline {
                v: root.v,
                old: "newHeadline".to_string(),
                new: "one".to_string(),
            },
        );
        u.undo(&mut o);
        assert!(u.can_redo());
        u.push(
            "two",
            Bead::Headline {
                v: root.v,
                old: "newHeadline".to_string(),
                new: "two".to_string(),
            },
        );
        assert!(!u.can_redo());
        assert_eq!(u.undo_name(), Some("two"));
    }
}
