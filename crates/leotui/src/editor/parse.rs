//! The body's key grammar.
//!
//! `count? (operator count? (motion | text-object | operator))`, parsed once
//! and handed to the editor. Scattering this across key handlers is how vim
//! clones end up with a matrix of special cases instead of a grammar.

use crossterm::event::{KeyCode, KeyModifiers};

use super::change::{InsertAt, Operator, Range, Simple};
use super::motion::{Motion, TextObject};
use crate::keys::Key;

/// What a complete key sequence asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Move(Motion, usize),
    Operate {
        operator: Operator,
        range: Range,
        count: usize,
    },
    Edit(Simple, usize),
    Insert(InsertAt, usize),
    /// `v` and `V`.
    Visual {
        linewise: bool,
    },
    /// `o` in VISUAL.
    SwapEnds,
    Repeat(usize),
    Undo(usize),
    Redo(usize),
    SearchForward,
    SearchBackward,
    FindNext(usize),
    FindPrev(usize),
    RepeatFind {
        reverse: bool,
        count: usize,
    },
    Command,
    FocusTree,
    Escape,
    /// A count is being typed, or an operator is waiting for its range.
    Pending,
    Unknown,
}

/// The keys typed so far in the body pane.
#[derive(Default)]
pub struct Parser {
    count: Option<usize>,
    operator: Option<Operator>,
    operator_count: Option<usize>,
    /// A prefix such as `g`, `z`, `i`, `a`, `f`, `r`.
    prefix: Option<char>,
    /// True while a VISUAL selection is live: `i`/`a` then mean text objects.
    pub visual: bool,
}

impl Parser {
    pub fn reset(&mut self) {
        *self = Parser {
            visual: self.visual,
            ..Default::default()
        };
    }

    pub fn is_pending(&self) -> bool {
        self.count.is_some() || self.operator.is_some() || self.prefix.is_some()
    }

    /// What has been typed, for the status line.
    pub fn describe(&self) -> String {
        let mut out = String::new();
        if let Some(n) = self.count {
            out.push_str(&n.to_string());
        }
        if let Some(op) = self.operator {
            out.push(operator_key(op));
        }
        if let Some(n) = self.operator_count {
            out.push_str(&n.to_string());
        }
        if let Some(c) = self.prefix {
            out.push(c);
        }
        out
    }

    fn total_count(&self) -> usize {
        match (self.count, self.operator_count) {
            (Some(a), Some(b)) => a * b,
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => 1,
        }
    }

    /// Feed one key. Returns the action it completes, or `Pending`.
    pub fn feed(&mut self, key: Key) -> Action {
        let ctrl = key.mods.contains(KeyModifiers::CONTROL);
        let ch = match key.code {
            KeyCode::Char(c) => Some(c),
            _ => None,
        };

        // A pending `f`, `F`, `t`, `T` or `r` takes the next key literally.
        if let Some(p) = self.prefix {
            if matches!(p, 'f' | 'F' | 't' | 'T' | 'r') {
                let Some(c) = ch else {
                    self.reset();
                    return Action::Unknown;
                };
                let count = self.total_count();
                self.reset();
                return match p {
                    'r' => Action::Edit(Simple::Replace(c), count),
                    _ => {
                        let motion = Motion::Find {
                            ch: c,
                            forward: p == 'f' || p == 't',
                            till: p == 't' || p == 'T',
                        };
                        self.finish_motion(motion, count)
                    }
                };
            }
        }

        if ctrl {
            let count = self.total_count();
            self.reset();
            let shift = key.mods.contains(KeyModifiers::SHIFT);
            return match ch {
                Some('r') => Action::Redo(count),
                // Leo binds redo to Shift-Ctrl-Z, which only a terminal
                // speaking the enhancement protocol can deliver.
                Some('z') if shift => Action::Redo(count),
                Some('z') => Action::Undo(count),
                Some('c') => Action::Escape,
                _ => Action::Unknown,
            };
        }

        match key.code {
            KeyCode::Esc => {
                let pending = self.is_pending();
                self.reset();
                return if pending {
                    Action::Pending
                } else {
                    Action::Escape
                };
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.reset();
                return Action::FocusTree;
            }
            KeyCode::Left => return self.finish_motion(Motion::Left, self.take_count()),
            KeyCode::Right => return self.finish_motion(Motion::Right, self.take_count()),
            KeyCode::Up => return self.finish_motion(Motion::Up, self.take_count()),
            KeyCode::Down => return self.finish_motion(Motion::Down, self.take_count()),
            KeyCode::Home => return self.finish_motion(Motion::LineStart, 1),
            KeyCode::End => return self.finish_motion(Motion::LineEnd, 1),
            _ => {}
        }

        let Some(c) = ch else {
            self.reset();
            return Action::Unknown;
        };

        // A count, unless `0` starts one (where it is a motion).
        if c.is_ascii_digit() && !(c == '0' && self.digits_empty()) {
            let d = c as usize - '0' as usize;
            if self.operator.is_some() {
                self.operator_count = Some(self.operator_count.unwrap_or(0) * 10 + d);
            } else {
                self.count = Some(self.count.unwrap_or(0) * 10 + d);
            }
            return Action::Pending;
        }

        // A two-key prefix.
        if let Some(p) = self.prefix.take() {
            return self.finish_prefix(p, c);
        }

        match c {
            'g' | 'z' | 'f' | 'F' | 't' | 'T' | 'r' => {
                self.prefix = Some(c);
                Action::Pending
            }
            'i' | 'a' if self.operator.is_some() || self.visual => {
                self.prefix = Some(c);
                Action::Pending
            }
            _ => self.simple_key(c),
        }
    }

    fn digits_empty(&self) -> bool {
        if self.operator.is_some() {
            self.operator_count.is_none()
        } else {
            self.count.is_none()
        }
    }

    fn take_count(&self) -> usize {
        self.total_count()
    }

    fn finish_prefix(&mut self, prefix: char, c: char) -> Action {
        let count = self.total_count();
        match (prefix, c) {
            ('g', 'g') => {
                let n = self.count.unwrap_or(1);
                self.reset();
                self.finish_motion(Motion::GotoLine(n), 1)
            }
            ('g', 'e') => {
                self.reset();
                self.finish_motion(Motion::WordEndBackward { big: false }, count)
            }
            ('g', 'E') => {
                self.reset();
                self.finish_motion(Motion::WordEndBackward { big: true }, count)
            }
            ('g', 'u') | ('g', 'U') | ('g', '~') => {
                self.operator = Some(match c {
                    'u' => Operator::Lowercase,
                    'U' => Operator::Uppercase,
                    _ => Operator::ToggleCase,
                });
                Action::Pending
            }
            ('i', _) | ('a', _) => {
                let around = prefix == 'a';
                let Some(object) = text_object(c, around) else {
                    self.reset();
                    return Action::Unknown;
                };
                self.finish_range(Range::Object(object), count)
            }
            _ => {
                self.reset();
                Action::Unknown
            }
        }
    }

    fn simple_key(&mut self, c: char) -> Action {
        let count = self.total_count();
        // An operator waiting for a range, doubled, is linewise.
        if let Some(op) = self.operator {
            if operator_key(op) == c || (op == Operator::Change && c == 'c') {
                self.reset();
                return Action::Operate {
                    operator: op,
                    range: Range::Line,
                    count,
                };
            }
        }
        if let Some(motion) = motion_key(c) {
            return self.finish_motion(motion, count);
        }
        if let Some(op) = Operator::from_key(c) {
            if self.visual {
                self.reset();
                return Action::Operate {
                    operator: op,
                    range: Range::Visual {
                        rows: 0,
                        cols: 0,
                        kind: super::motion::Kind::Charwise,
                    },
                    count,
                };
            }
            self.operator = Some(op);
            return Action::Pending;
        }
        self.reset();
        match c {
            'i' => Action::Insert(InsertAt::Cursor, count),
            'a' => Action::Insert(InsertAt::After, count),
            'I' => Action::Insert(InsertAt::LineStart, count),
            'A' => Action::Insert(InsertAt::LineEnd, count),
            'o' if self.visual => Action::SwapEnds,
            'o' => Action::Insert(InsertAt::OpenBelow, count),
            'O' => Action::Insert(InsertAt::OpenAbove, count),
            'x' => Action::Edit(Simple::DeleteChar { before: false }, count),
            'X' => Action::Edit(Simple::DeleteChar { before: true }, count),
            'J' => Action::Edit(Simple::JoinLines, count),
            '~' => Action::Edit(Simple::ToggleCaseChar, count),
            'p' => Action::Edit(Simple::Put { before: false }, count),
            'P' => Action::Edit(Simple::Put { before: true }, count),
            's' => Action::Operate {
                operator: Operator::Change,
                range: Range::Motion(Motion::Right),
                count,
            },
            'S' => Action::Operate {
                operator: Operator::Change,
                range: Range::Line,
                count,
            },
            'D' => Action::Operate {
                operator: Operator::Delete,
                range: Range::Motion(Motion::LineEnd),
                count,
            },
            'C' => Action::Operate {
                operator: Operator::Change,
                range: Range::Motion(Motion::LineEnd),
                count,
            },
            'Y' => Action::Operate {
                operator: Operator::Yank,
                range: Range::Line,
                count,
            },
            'v' => Action::Visual { linewise: false },
            'V' => Action::Visual { linewise: true },
            '.' => Action::Repeat(count),
            'u' => Action::Undo(count),
            '/' => Action::SearchForward,
            '?' => Action::SearchBackward,
            'n' => Action::FindNext(count),
            'N' => Action::FindPrev(count),
            ';' => Action::RepeatFind {
                reverse: false,
                count,
            },
            ',' => Action::RepeatFind {
                reverse: true,
                count,
            },
            ':' => Action::Command,
            _ => Action::Unknown,
        }
    }

    /// A motion completes an operator, or moves the cursor.
    fn finish_motion(&mut self, motion: Motion, count: usize) -> Action {
        self.finish_range(Range::Motion(motion), count)
    }

    fn finish_range(&mut self, range: Range, count: usize) -> Action {
        match self.operator.take() {
            Some(operator) => {
                self.reset();
                Action::Operate {
                    operator,
                    range,
                    count,
                }
            }
            None => {
                self.reset();
                match range {
                    Range::Motion(m) => Action::Move(m, count),
                    // A text object with no operator only makes sense in
                    // VISUAL, where it extends the selection.
                    Range::Object(_) if self.visual => Action::Pending,
                    _ => Action::Unknown,
                }
            }
        }
    }
}

fn operator_key(op: Operator) -> char {
    match op {
        Operator::Delete => 'd',
        Operator::Change => 'c',
        Operator::Yank => 'y',
        Operator::Indent => '>',
        Operator::Unindent => '<',
        Operator::Lowercase | Operator::Uppercase | Operator::ToggleCase => 'g',
    }
}

fn motion_key(c: char) -> Option<Motion> {
    Some(match c {
        'h' => Motion::Left,
        'l' | ' ' => Motion::Right,
        'j' => Motion::Down,
        'k' => Motion::Up,
        'w' => Motion::WordForward { big: false },
        'W' => Motion::WordForward { big: true },
        'b' => Motion::WordBackward { big: false },
        'B' => Motion::WordBackward { big: true },
        'e' => Motion::WordEnd { big: false },
        'E' => Motion::WordEnd { big: true },
        '0' => Motion::LineStart,
        '^' => Motion::FirstNonBlank,
        '$' => Motion::LineEnd,
        'G' => Motion::FileEnd,
        '{' => Motion::ParagraphBackward,
        '}' => Motion::ParagraphForward,
        '%' => Motion::MatchingBracket,
        'H' => Motion::ScreenTop,
        'M' => Motion::ScreenMiddle,
        'L' => Motion::ScreenBottom,
        _ => return None,
    })
}

fn text_object(c: char, around: bool) -> Option<TextObject> {
    Some(match c {
        'w' => TextObject::Word { big: false, around },
        'W' => TextObject::Word { big: true, around },
        '"' | '\'' | '`' => TextObject::Quoted { delim: c, around },
        '(' | ')' | 'b' => TextObject::Bracket { open: '(', around },
        '[' | ']' => TextObject::Bracket { open: '[', around },
        '{' | '}' | 'B' => TextObject::Bracket { open: '{', around },
        '<' | '>' => TextObject::Bracket { open: '<', around },
        'p' => TextObject::Paragraph { around },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys;

    fn feed(parser: &mut Parser, spec: &str) -> Action {
        let mut last = Action::Pending;
        for key in keys::parse(spec) {
            last = parser.feed(key);
        }
        last
    }

    fn parse_keys(spec: &str) -> Action {
        let mut p = Parser::default();
        feed(&mut p, spec)
    }

    #[test]
    fn a_motion_moves() {
        assert_eq!(
            parse_keys("w"),
            Action::Move(Motion::WordForward { big: false }, 1)
        );
    }

    #[test]
    fn a_count_multiplies_through_the_operator() {
        // vim: 2d3w deletes six words.
        let action = parse_keys("2d3w");
        assert_eq!(
            action,
            Action::Operate {
                operator: Operator::Delete,
                range: Range::Motion(Motion::WordForward { big: false }),
                count: 6,
            }
        );
    }

    #[test]
    fn a_doubled_operator_is_linewise() {
        assert_eq!(
            parse_keys("dd"),
            Action::Operate {
                operator: Operator::Delete,
                range: Range::Line,
                count: 1
            }
        );
        assert_eq!(
            parse_keys("3yy"),
            Action::Operate {
                operator: Operator::Yank,
                range: Range::Line,
                count: 3
            }
        );
    }

    #[test]
    fn an_operator_takes_a_text_object() {
        assert_eq!(
            parse_keys("ciw"),
            Action::Operate {
                operator: Operator::Change,
                range: Range::Object(TextObject::Word {
                    big: false,
                    around: false
                }),
                count: 1,
            }
        );
        assert_eq!(
            parse_keys("da\""),
            Action::Operate {
                operator: Operator::Delete,
                range: Range::Object(TextObject::Quoted {
                    delim: '"',
                    around: true
                }),
                count: 1,
            }
        );
    }

    #[test]
    fn zero_is_a_motion_until_a_count_has_begun() {
        assert_eq!(parse_keys("0"), Action::Move(Motion::LineStart, 1));
        assert_eq!(parse_keys("10j"), Action::Move(Motion::Down, 10));
    }

    #[test]
    fn find_takes_the_next_key_literally() {
        let mut p = Parser::default();
        assert_eq!(p.feed(keys::parse("f")[0]), Action::Pending);
        assert_eq!(
            p.feed(keys::parse("x")[0]),
            Action::Move(
                Motion::Find {
                    ch: 'x',
                    forward: true,
                    till: false
                },
                1
            )
        );
    }

    #[test]
    fn r_takes_the_next_key_literally_even_if_it_is_a_command() {
        let mut p = Parser::default();
        p.feed(keys::parse("r")[0]);
        assert_eq!(
            p.feed(keys::parse("d")[0]),
            Action::Edit(Simple::Replace('d'), 1)
        );
    }

    #[test]
    fn gg_goes_to_a_line() {
        assert_eq!(parse_keys("gg"), Action::Move(Motion::GotoLine(1), 1));
        assert_eq!(parse_keys("5gg"), Action::Move(Motion::GotoLine(5), 1));
    }

    #[test]
    fn case_operators_take_a_motion() {
        assert_eq!(
            parse_keys("guw"),
            Action::Operate {
                operator: Operator::Lowercase,
                range: Range::Motion(Motion::WordForward { big: false }),
                count: 1,
            }
        );
    }

    #[test]
    fn shorthands_expand_to_operators() {
        assert_eq!(
            parse_keys("D"),
            Action::Operate {
                operator: Operator::Delete,
                range: Range::Motion(Motion::LineEnd),
                count: 1
            }
        );
        assert_eq!(
            parse_keys("S"),
            Action::Operate {
                operator: Operator::Change,
                range: Range::Line,
                count: 1
            }
        );
    }

    #[test]
    fn escape_abandons_a_pending_operator() {
        let mut p = Parser::default();
        p.feed(keys::parse("d")[0]);
        assert!(p.is_pending());
        assert_eq!(p.feed(keys::parse("Escape")[0]), Action::Pending);
        assert!(!p.is_pending());
        // With nothing pending, Escape leaves the pane.
        assert_eq!(p.feed(keys::parse("Escape")[0]), Action::Escape);
    }

    #[test]
    fn what_is_typed_can_be_shown() {
        let mut p = Parser::default();
        feed(&mut p, "2d3");
        assert_eq!(p.describe(), "2d3");
    }
}
