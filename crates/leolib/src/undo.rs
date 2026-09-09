//! One undo history per outline.
//!
//! The history belongs to the document, not to whichever view made the edit:
//! two windows on one outline share one stack, and undo in either must move
//! the other. Beads record the inverse of an operation rather than a snapshot
//! of the tree, so the cost of an edit does not grow with the outline.
//!
//! Deleting a node never frees its vnode, which is what makes undoing a
//! delete a matter of relinking rather than rebuilding.

use crate::node::{status, VnodeId};
use crate::outline::Outline;
use crate::position::Position;

/// The inverse of one edit.
#[derive(Debug, Clone)]
pub enum Bead {
    Body {
        v: VnodeId,
        old: String,
        new: String,
    },
    Headline {
        v: VnodeId,
        old: String,
        new: String,
    },
    /// A node was linked in. Undo cuts the link.
    Insert {
        parent: VnodeId,
        index: usize,
        v: VnodeId,
    },
    /// A node was unlinked. Undo puts it back where it was.
    Delete {
        parent: VnodeId,
        index: usize,
        v: VnodeId,
    },
    Move {
        v: VnodeId,
        from: (VnodeId, usize),
        to: (VnodeId, usize),
    },
    Mark {
        v: VnodeId,
        was_marked: bool,
    },
    /// Several edits that undo as one, such as a paste or a demote.
    Group {
        name: String,
        beads: Vec<Bead>,
    },
}

/// The undo stack, with the name of each operation for a menu or status line.
#[derive(Debug, Default)]
pub struct Undoer {
    beads: Vec<(String, Bead)>,
    /// How many beads have been applied. Redo replays from here.
    index: usize,
    /// Beads being collected into one group, innermost last.
    open_groups: Vec<(String, Vec<Bead>)>,
}

impl Undoer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn can_undo(&self) -> bool {
        self.index > 0
    }

    pub fn can_redo(&self) -> bool {
        self.index < self.beads.len()
    }

    pub fn undo_name(&self) -> Option<&str> {
        self.beads
            .get(self.index.checked_sub(1)?)
            .map(|b| b.0.as_str())
    }

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
    }

    pub fn clear(&mut self) {
        self.beads.clear();
        self.index = 0;
        self.open_groups.clear();
    }

    /// Undo one operation. Returns the node to select, if it still exists.
    pub fn undo(&mut self, o: &mut Outline) -> Option<Position> {
        if !self.can_undo() {
            return None;
        }
        self.index -= 1;
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
    o.node_mut(parent).children.remove(index);
    if let Some(i) = o.node(v).parents.iter().position(|x| *x == parent) {
        o.node_mut(v).parents.remove(i);
    }
    o.changed = true;
    o.generation += 1;
}

fn relink(o: &mut Outline, parent: VnodeId, index: usize, v: VnodeId) {
    let n = index.min(o.node(parent).children.len());
    o.node_mut(parent).children.insert(n, v);
    o.node_mut(v).parents.push(parent);
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
