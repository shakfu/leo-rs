//! Leo's hoist: show one node and its subtree as if it were the outline.
//!
//! `hoist`, `dehoist` and `clear-all-hoists` as `commanderOutlineCommands.py`
//! has them. Selecting a node outside the hoist dehoists until it shows, as
//! `c.selectPosition` does, so search, `gd` and `go-back` still reach it.

use super::*;

impl App {
    /// The hoisted node, if any: the outline pane's only top-level row.
    pub fn hoist_limit(&self) -> Option<&Position> {
        self.hoists.last().map(|(p, _)| p)
    }

    /// Whether `p` is the hoisted node or below it.
    pub fn in_view(&self, p: &Position) -> bool {
        self.hoist_limit()
            .is_none_or(|h| h == p || h.is_ancestor_of(self.outline(), p))
    }

    pub fn hoist(&mut self) {
        let p = self.current.clone();
        let expanded = self.outline().is_expanded(&p);
        self.hoists.push((p.clone(), expanded));
        self.doc.outline_mut_untracked().expand(&p);
        self.message = format!("hoist: {}", p.h(self.outline()));
    }

    pub fn dehoist(&mut self) {
        let Some((p, expanded)) = self.hoists.pop() else {
            self.message = "not hoisted".to_string();
            return;
        };
        if !expanded {
            self.doc.outline_mut_untracked().contract(&p);
        }
        self.message = format!("dehoist: {}", p.h(self.outline()));
    }

    pub fn clear_hoists(&mut self) {
        self.hoists.clear();
        self.message = "hoists cleared".to_string();
    }

    /// Pop hoists until `p` shows, and any whose node an edit has moved.
    pub(super) fn dehoist_to_show(&mut self, p: &Position) {
        while let Some(h) = self.hoist_limit() {
            if self.outline().position_exists(h) && self.in_view(p) {
                break;
            }
            self.hoists.pop();
        }
    }

    /// Whether a structure command on `p` must be refused, because it would
    /// move the hoisted node or, for move-left, carry `p` out of it. Leo's
    /// `canMoveOutline*` refuse the same moves.
    pub fn hoist_refuses(&mut self, p: &Position, leaves_parent: bool) -> bool {
        let Some(h) = self.hoist_limit() else {
            return false;
        };
        let refused = h == p || (leaves_parent && p.parent(self.outline()).as_ref() == Some(h));
        if refused {
            self.message = "not while hoisted: it would leave the hoisted tree".to_string();
        }
        refused
    }
}
