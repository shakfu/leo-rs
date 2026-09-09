//! Changes: the unit of editing, of undo, and of `.`.
//!
//! A change is a value rather than a closure, so `.` replays it and a test can
//! assert on it. One change is one `Document::set_body`, which is one undo
//! bead: typing 200 characters and pressing Escape is a single `u`.

use super::motion::{Kind, Motion, Pos, TextObject};

/// What an operator does to the range a motion or object gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    Delete,
    Change,
    Yank,
    Indent,
    Unindent,
    Lowercase,
    Uppercase,
    ToggleCase,
}

impl Operator {
    pub fn from_key(c: char) -> Option<Self> {
        Some(match c {
            'd' => Operator::Delete,
            'c' => Operator::Change,
            'y' => Operator::Yank,
            '>' => Operator::Indent,
            '<' => Operator::Unindent,
            _ => return None,
        })
    }

    /// True if the operator leaves the editor in INSERT.
    pub fn enters_insert(self) -> bool {
        self == Operator::Change
    }
}

/// What an operator is applied to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    Motion(Motion),
    Object(TextObject),
    /// A doubled operator: `dd`, `cc`, `>>`.
    Line,
    /// The selection a VISUAL mode had, replayed as a shape.
    Visual {
        rows: usize,
        cols: usize,
        kind: Kind,
    },
}

/// Edits that are not operator-plus-range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Simple {
    DeleteChar { before: bool },
    Replace(char),
    JoinLines,
    ToggleCaseChar,
    Put { before: bool },
}

/// Where an INSERT session began, so `.` can repeat it in the same place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertAt {
    Cursor,
    After,
    LineStart,
    LineEnd,
    OpenBelow,
    OpenAbove,
}

/// One replayable change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Operator {
        count: usize,
        operator: Operator,
        range: Range,
    },
    Simple {
        count: usize,
        edit: Simple,
    },
    Insert {
        at: InsertAt,
        text: String,
        count: usize,
    },
    /// `cw`, `S`, `C`: an operator that deletes, then the text typed.
    OperatorInsert {
        count: usize,
        operator: Operator,
        range: Range,
        text: String,
    },
}

/// What `y` and `d` put in the register, and what `p` takes out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Register {
    pub text: String,
    pub linewise: bool,
}

/// Order two positions.
pub fn ordered(a: Pos, b: Pos) -> (Pos, Pos) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operators_come_from_their_keys() {
        assert_eq!(Operator::from_key('d'), Some(Operator::Delete));
        assert_eq!(Operator::from_key('>'), Some(Operator::Indent));
        assert_eq!(Operator::from_key('q'), None);
    }

    #[test]
    fn only_change_enters_insert() {
        assert!(Operator::Change.enters_insert());
        assert!(!Operator::Delete.enters_insert());
    }

    #[test]
    fn a_change_is_a_value_so_it_can_be_stored_and_compared() {
        let a = Change::Simple {
            count: 1,
            edit: Simple::DeleteChar { before: false },
        };
        assert_eq!(a.clone(), a);
    }
}
