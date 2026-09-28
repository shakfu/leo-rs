//! Leo's node history: every selected node, for `go-back` and `go-forward`.
//!
//! A port of `leoHistory.NodeHistory` after #3800: selecting a node moves it
//! to just after the pointer, so the list holds each vnode once. Two
//! departures, both in `docs/dev/porting-notes.md`: the pointer counts only
//! the removed beads at or before it, and a step skips stale beads rather
//! than stopping on the first.

use leolib::{Outline, Position};

#[derive(Default)]
pub struct NodeHistory {
    beads: Vec<Position>,
    /// The bead the selection is on. Meaningless while `beads` is empty.
    pointer: usize,
}

impl NodeHistory {
    /// Record `p` as selected. Stepping through the history lands on the
    /// bead already at the pointer, which this leaves where it is.
    pub fn update(&mut self, p: &Position) {
        if self.beads.get(self.pointer) == Some(p) {
            return;
        }
        let before = self.beads[..self.beads.len().min(self.pointer + 1)]
            .iter()
            .filter(|b| b.v == p.v)
            .count();
        self.beads.retain(|b| b.v != p.v);
        let at = if self.beads.is_empty() {
            0
        } else {
            self.pointer + 1 - before
        };
        self.beads.insert(at, p.clone());
        self.pointer = at;
    }

    /// The bead `step` away (-1 back, +1 forward) that still exists, dropping
    /// stale ones on the way. The pointer moves to it.
    pub fn step(&mut self, outline: &Outline, step: isize) -> Option<Position> {
        loop {
            let next = self.pointer.checked_add_signed(step)?;
            let p = self.beads.get(next)?.clone();
            if outline.position_is_linked(&p) {
                self.pointer = next;
                return Some(p);
            }
            self.beads.remove(next);
            if step < 0 {
                self.pointer -= 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use leolib::Document;

    /// Four sibling nodes a, b, c, d.
    fn outline() -> (Document, Vec<Position>) {
        let mut doc = Document::new_empty("");
        let mut p = doc.outline.root_position().unwrap();
        let mut all = vec![p.clone()];
        for _ in 0..3 {
            p = doc.outline.insert_after(&p);
            all.push(p.clone());
        }
        (doc, all)
    }

    #[test]
    fn back_and_forward_walk_the_selections() {
        let (doc, n) = outline();
        let mut h = NodeHistory::default();
        for p in &n[..3] {
            h.update(p);
        }
        assert_eq!(h.step(&doc.outline, -1), Some(n[1].clone()));
        assert_eq!(h.step(&doc.outline, -1), Some(n[0].clone()));
        assert_eq!(h.step(&doc.outline, -1), None);
        assert_eq!(h.step(&doc.outline, 1), Some(n[1].clone()));
        assert_eq!(h.step(&doc.outline, 1), Some(n[2].clone()));
        assert_eq!(h.step(&doc.outline, 1), None);
    }

    #[test]
    fn a_node_is_kept_once_and_lands_after_the_pointer() {
        // #3800: after going back to b, selecting d puts d after b.
        let (doc, n) = outline();
        let mut h = NodeHistory::default();
        for p in &n[..3] {
            h.update(p);
        }
        h.step(&doc.outline, -1);
        h.update(&n[3]);
        assert_eq!(
            h.beads,
            vec![n[0].clone(), n[1].clone(), n[3].clone(), n[2].clone()]
        );
        // Selecting a again moves it rather than adding a second bead.
        h.update(&n[0]);
        assert_eq!(h.beads.len(), 4);
        assert_eq!(h.step(&doc.outline, -1), Some(n[3].clone()));
    }

    #[test]
    fn a_later_bead_does_not_move_the_pointer_back() {
        // Leo subtracts every removed bead, so c would land before b here.
        let (doc, n) = outline();
        let mut h = NodeHistory::default();
        for p in &n[..3] {
            h.update(p);
        }
        h.step(&doc.outline, -1);
        h.update(&n[2]);
        assert_eq!(h.beads, vec![n[0].clone(), n[1].clone(), n[2].clone()]);
        assert_eq!(h.step(&doc.outline, -1), Some(n[1].clone()));
    }

    #[test]
    fn a_deleted_node_is_skipped() {
        let (mut doc, n) = outline();
        let mut h = NodeHistory::default();
        for p in &n[..3] {
            h.update(p);
        }
        doc.delete_node(&n[1]);
        assert_eq!(h.step(&doc.outline, -1), Some(n[0].clone()));
        // b's bead is dropped. c's is stale too, since c moved up, but
        // nothing has stepped onto it yet.
        assert_eq!(h.beads, vec![n[0].clone(), n[2].clone()]);
    }
}
