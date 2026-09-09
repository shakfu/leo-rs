//! Positions: one place a vnode appears in the outline.
//!
//! A vnode with several parents appears in several places, so "the node" is
//! never enough to say where you are. A position is a vnode, its index among
//! its parent's children, and the same pair for every ancestor. That is Leo's
//! `Position` unchanged; only the spelling differs. Leo's invalid position
//! (`p.v is None`) is `Option<Position>` here, so a traversal that runs off
//! the end cannot be used by accident.

use crate::node::VnodeId;
use crate::outline::Outline;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    pub v: VnodeId,
    pub child_index: usize,
    /// (parent vnode, that parent's own child index), outermost first.
    pub stack: Vec<(VnodeId, usize)>,
}

impl Position {
    pub fn new(v: VnodeId, child_index: usize, stack: Vec<(VnodeId, usize)>) -> Self {
        Self {
            v,
            child_index,
            stack,
        }
    }

    /// A key that is equal for equal positions. Two clones of one vnode differ.
    pub fn key(&self, o: &Outline) -> String {
        let mut parts: Vec<String> = self
            .stack
            .iter()
            .map(|(v, i)| format!("{}:{}", o.gnx(*v), i))
            .collect();
        parts.push(format!("{}:{}", o.gnx(self.v), self.child_index));
        parts.join(",")
    }

    /// The child indices from the root down to here, as `p.archivedPosition`.
    pub fn archived_position(&self) -> Vec<usize> {
        let mut out: Vec<usize> = self.stack.iter().map(|(_, i)| *i).collect();
        out.push(self.child_index);
        out
    }

    pub fn level(&self) -> usize {
        self.stack.len()
    }

    /// The vnode holding this position's siblings: the parent, or the hidden root.
    pub fn parent_vnode(&self, o: &Outline) -> VnodeId {
        match self.stack.last() {
            Some((v, _)) => *v,
            None => o.hidden_root,
        }
    }

    // --- Reading the node -------------------------------------------------

    pub fn h<'a>(&self, o: &'a Outline) -> &'a str {
        &o.node(self.v).h
    }

    pub fn b<'a>(&self, o: &'a Outline) -> &'a str {
        &o.node(self.v).b
    }

    pub fn gnx<'a>(&self, o: &'a Outline) -> &'a str {
        &o.node(self.v).gnx
    }

    // --- Navigation -------------------------------------------------------

    pub fn has_children(&self, o: &Outline) -> bool {
        !o.node(self.v).children.is_empty()
    }

    pub fn num_children(&self, o: &Outline) -> usize {
        o.node(self.v).children.len()
    }

    pub fn nth_child(&self, o: &Outline, n: usize) -> Option<Position> {
        let child = *o.node(self.v).children.get(n)?;
        let mut stack = self.stack.clone();
        stack.push((self.v, self.child_index));
        Some(Position::new(child, n, stack))
    }

    pub fn first_child(&self, o: &Outline) -> Option<Position> {
        self.nth_child(o, 0)
    }

    pub fn last_child(&self, o: &Outline) -> Option<Position> {
        let n = self.num_children(o);
        if n == 0 {
            None
        } else {
            self.nth_child(o, n - 1)
        }
    }

    pub fn parent(&self, o: &Outline) -> Option<Position> {
        let _ = o;
        let (v, child_index) = *self.stack.last()?;
        let mut stack = self.stack.clone();
        stack.pop();
        Some(Position::new(v, child_index, stack))
    }

    pub fn next(&self, o: &Outline) -> Option<Position> {
        let parent_v = self.parent_vnode(o);
        let siblings = &o.node(parent_v).children;
        let n = self.child_index + 1;
        let v = *siblings.get(n)?;
        Some(Position::new(v, n, self.stack.clone()))
    }

    pub fn back(&self, o: &Outline) -> Option<Position> {
        if self.child_index == 0 {
            return None;
        }
        let parent_v = self.parent_vnode(o);
        let n = self.child_index - 1;
        let v = *o.node(parent_v).children.get(n)?;
        Some(Position::new(v, n, self.stack.clone()))
    }

    pub fn has_next(&self, o: &Outline) -> bool {
        self.next(o).is_some()
    }

    pub fn has_back(&self) -> bool {
        self.child_index > 0
    }

    /// The last node of this node's subtree, following last children down.
    pub fn last_node(&self, o: &Outline) -> Position {
        let mut p = self.clone();
        while let Some(child) = p.last_child(o) {
            p = child;
        }
        p
    }

    /// The next node in outline order, ignoring folds.
    pub fn thread_next(&self, o: &Outline) -> Option<Position> {
        if let Some(child) = self.first_child(o) {
            return Some(child);
        }
        if let Some(next) = self.next(o) {
            return Some(next);
        }
        let mut p = self.clone();
        while let Some(parent) = p.parent(o) {
            if let Some(next) = parent.next(o) {
                return Some(next);
            }
            p = parent;
        }
        None
    }

    /// The previous node in outline order.
    pub fn thread_back(&self, o: &Outline) -> Option<Position> {
        match self.back(o) {
            Some(back) => Some(back.last_node(o)),
            None => self.parent(o),
        }
    }

    /// The first node after this node's whole subtree.
    pub fn node_after_tree(&self, o: &Outline) -> Option<Position> {
        let mut p = self.clone();
        loop {
            if let Some(next) = p.next(o) {
                return Some(next);
            }
            p = p.parent(o)?;
        }
    }

    /// The last node of this subtree that is actually on screen: descend
    /// through last children only while each is unfolded.
    pub fn last_visible_node(&self, o: &Outline) -> Position {
        let mut p = self.clone();
        while o.is_expanded(&p) {
            match p.last_child(o) {
                Some(child) => p = child,
                None => break,
            }
        }
        p
    }

    /// The previous node a reader can see, skipping folded subtrees.
    pub fn vis_back(&self, o: &Outline) -> Option<Position> {
        match self.back(o) {
            Some(back) => Some(back.last_visible_node(o)),
            None => self.parent(o),
        }
    }

    /// The next node a reader can see, skipping folded subtrees.
    pub fn vis_next(&self, o: &Outline) -> Option<Position> {
        if self.has_children(o) && o.is_expanded(self) {
            return self.first_child(o);
        }
        let mut p = self.clone();
        loop {
            if let Some(next) = p.next(o) {
                return Some(next);
            }
            p = p.parent(o)?;
        }
    }

    pub fn is_root(&self, o: &Outline) -> bool {
        let _ = o;
        self.stack.is_empty() && self.child_index == 0
    }

    pub fn is_cloned(&self, o: &Outline) -> bool {
        o.node(self.v).parents.len() > 1
    }

    /// True if `p2` lies in this node's subtree, below it.
    pub fn is_ancestor_of(&self, o: &Outline, p2: &Position) -> bool {
        let _ = o;
        if p2.stack.len() <= self.stack.len() {
            return false;
        }
        let (v, i) = p2.stack[self.stack.len()];
        v == self.v && i == self.child_index && p2.stack[..self.stack.len()] == self.stack[..]
    }

    // --- Generators -------------------------------------------------------

    pub fn children(&self, o: &Outline) -> Vec<Position> {
        (0..self.num_children(o))
            .map(|n| self.nth_child(o, n).unwrap())
            .collect()
    }

    pub fn self_and_siblings(&self, o: &Outline) -> Vec<Position> {
        let parent_v = self.parent_vnode(o);
        let n = o.node(parent_v).children.len();
        (0..n)
            .map(|i| Position::new(o.node(parent_v).children[i], i, self.stack.clone()))
            .collect()
    }

    pub fn following_siblings(&self, o: &Outline) -> Vec<Position> {
        let mut out = Vec::new();
        let mut p = self.clone();
        while let Some(next) = p.next(o) {
            out.push(next.clone());
            p = next;
        }
        out
    }

    pub fn self_and_parents(&self, o: &Outline) -> Vec<Position> {
        let mut out = vec![self.clone()];
        let mut p = self.clone();
        while let Some(parent) = p.parent(o) {
            out.push(parent.clone());
            p = parent;
        }
        out
    }

    pub fn parents(&self, o: &Outline) -> Vec<Position> {
        let mut out = self.self_and_parents(o);
        out.remove(0);
        out
    }

    pub fn self_and_subtree(&self, o: &Outline) -> Vec<Position> {
        let after = self.node_after_tree(o);
        let mut out = Vec::new();
        let mut p = Some(self.clone());
        while let Some(cur) = p {
            if Some(&cur) == after.as_ref() {
                break;
            }
            out.push(cur.clone());
            p = cur.thread_next(o);
        }
        out
    }

    pub fn subtree(&self, o: &Outline) -> Vec<Position> {
        let mut out = self.self_and_subtree(o);
        out.remove(0);
        out
    }

    // --- Predicates over the headline ------------------------------------

    pub fn any_at_file_node_name(&self, o: &Outline) -> String {
        crate::node::any_at_file_node_name(self.h(o))
    }
    pub fn is_any_at_file_node(&self, o: &Outline) -> bool {
        crate::node::is_any_at_file_node(self.h(o))
    }
    pub fn is_at_auto_node(&self, o: &Outline) -> bool {
        !crate::node::at_auto_node_name(self.h(o)).is_empty()
    }
    pub fn is_at_clean_node(&self, o: &Outline) -> bool {
        !crate::node::at_clean_node_name(self.h(o)).is_empty()
    }
    pub fn is_at_edit_node(&self, o: &Outline) -> bool {
        !crate::node::at_edit_node_name(self.h(o)).is_empty()
    }
    pub fn is_at_file_node(&self, o: &Outline) -> bool {
        !crate::node::at_file_node_name(self.h(o)).is_empty()
    }
    pub fn is_at_thin_file_node(&self, o: &Outline) -> bool {
        !crate::node::at_thin_node_name(self.h(o)).is_empty()
    }
    pub fn is_at_nosent_node(&self, o: &Outline) -> bool {
        !crate::node::at_nosent_node_name(self.h(o)).is_empty()
    }
    pub fn is_at_asis_node(&self, o: &Outline) -> bool {
        !crate::node::at_asis_node_name(self.h(o)).is_empty()
    }
    pub fn is_at_shadow_file_node(&self, o: &Outline) -> bool {
        !crate::node::at_shadow_node_name(self.h(o)).is_empty()
    }
    pub fn is_at_jupytext_node(&self, o: &Outline) -> bool {
        !crate::node::at_jupytext_node_name(self.h(o)).is_empty()
    }
    pub fn is_at_ignore_node(&self, o: &Outline) -> bool {
        crate::node::is_at_ignore_node(self.h(o), self.b(o))
    }
    pub fn is_at_others_node(&self, o: &Outline) -> bool {
        crate::node::is_special(self.b(o), "@others")
    }
    pub fn is_at_all_node(&self, o: &Outline) -> bool {
        crate::node::is_special(self.b(o), "@all")
    }
    pub fn match_headline(&self, o: &Outline, pattern: &str) -> bool {
        crate::node::match_headline(self.h(o), pattern)
    }

    pub fn is_marked(&self, o: &Outline) -> bool {
        o.node(self.v).is_marked()
    }
    pub fn is_dirty(&self, o: &Outline) -> bool {
        o.node(self.v).is_dirty()
    }
    pub fn is_visited(&self, o: &Outline) -> bool {
        o.node(self.v).is_visited()
    }

    /// True if any ancestor, or this node, is an @ignore node.
    pub fn in_at_ignore_range(&self, o: &Outline) -> bool {
        self.self_and_parents(o)
            .iter()
            .any(|p| p.is_at_ignore_node(o))
    }
}

#[cfg(test)]
mod tests {
    use crate::Outline;

    fn sample() -> (Outline, Vec<String>) {
        // root
        //   a
        //     a1
        //   b
        // root2
        let mut o = Outline::new_empty();
        let root = o.root_position().unwrap();
        o.set_headline(&root, "root");
        let a = o.insert_as_last_child(&root);
        o.set_headline(&a, "a");
        let a1 = o.insert_as_last_child(&a);
        o.set_headline(&a1, "a1");
        let b = o.insert_as_last_child(&root);
        o.set_headline(&b, "b");
        let root2 = o.insert_after(&root);
        o.set_headline(&root2, "root2");
        let order: Vec<String> = o
            .all_positions()
            .iter()
            .map(|p| p.h(&o).to_string())
            .collect();
        (o, order)
    }

    #[test]
    fn thread_next_visits_the_outline_in_order() {
        let (_o, order) = sample();
        assert_eq!(order, vec!["root", "a", "a1", "b", "root2"]);
    }

    #[test]
    fn node_after_tree_skips_descendants() {
        let (o, _) = sample();
        let root = o.root_position().unwrap();
        let after = root.node_after_tree(&o).unwrap();
        assert_eq!(after.h(&o), "root2");
    }

    #[test]
    fn thread_back_is_the_inverse_of_thread_next() {
        let (o, _) = sample();
        let all = o.all_positions();
        for w in all.windows(2) {
            assert_eq!(w[1].thread_back(&o).as_ref(), Some(&w[0]));
        }
    }

    #[test]
    fn ancestry_is_by_position_not_by_vnode() {
        let (o, _) = sample();
        let root = o.root_position().unwrap();
        let a1 = o.all_positions()[2].clone();
        assert!(root.is_ancestor_of(&o, &a1));
        assert!(!a1.is_ancestor_of(&o, &root));
    }
}
