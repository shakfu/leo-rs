//! Search, vim's `/` and `?`, over the whole outline.
//!
//! One walk in outline order, whichever pane has focus: each node's headline,
//! then its body. `:set search=headlines` leaves bodies out. The pattern is a
//! `regex` crate regex with vim's smartcase, as `:s` uses.

use std::ops::Range;

use leolib::{Outline, Position};
use regex::{Regex, RegexBuilder};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Backward,
}

impl Direction {
    pub fn reverse(self) -> Self {
        match self {
            Direction::Forward => Direction::Backward,
            Direction::Backward => Direction::Forward,
        }
    }
}

/// What a search looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Headlines only, which `:set search=headlines` turns on.
    Headlines,
    /// Headlines and body text.
    All,
}

/// The last search, so `n` and `N` can repeat it.
#[derive(Clone)]
pub struct LastSearch {
    pub pattern: String,
    pub direction: Direction,
}

/// Where in a node a match starts. A headline sorts before its body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Place {
    /// A byte offset in the headline.
    Headline(usize),
    /// A body line, and a byte offset in it.
    Body(usize, usize),
}

/// A match: its node, and where in the node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub node: Position,
    pub place: Place,
}

/// `pattern` as a regex, with vim's smartcase.
pub fn compile(pattern: &str) -> Result<Regex, String> {
    RegexBuilder::new(pattern)
        .case_insensitive(!has_capital(pattern))
        .build()
        .map_err(|e| {
            let detail = e.to_string();
            format!("search: {}", detail.lines().last().unwrap_or("bad pattern"))
        })
}

/// Smartcase: a capital letter makes the pattern case sensitive. One after a
/// backslash is an escape, such as `\S`, not a letter.
pub fn has_capital(pattern: &str) -> bool {
    let mut escaped = false;
    for c in pattern.chars() {
        if !escaped && c.is_uppercase() {
            return true;
        }
        escaped = !escaped && c == '\\';
    }
    false
}

/// Where the matches in `text` lie, for highlighting. Empty ones are left out.
pub fn ranges(re: &Regex, text: &str) -> Vec<Range<usize>> {
    re.find_iter(text)
        .filter(|m| !m.is_empty())
        .map(|m| m.range())
        .collect()
}

/// Every match in node `p`, in order.
fn places(o: &Outline, p: &Position, re: &Regex, scope: Scope) -> Vec<Place> {
    let mut out: Vec<Place> = re
        .find_iter(p.h(o))
        .map(|m| Place::Headline(m.start()))
        .collect();
    let body = p.b(o);
    if scope == Scope::All && re.is_match(body) {
        // The body's lines as the editor splits them: a final newline ends
        // the last line rather than starting another.
        let body = body.strip_suffix('\n').unwrap_or(body);
        for (row, line) in body.split('\n').enumerate() {
            out.extend(re.find_iter(line).map(|m| Place::Body(row, m.start())));
        }
    }
    out
}

/// The first match after `from`, or the last before it going backward,
/// wrapping once round the outline. The flag says whether it wrapped.
pub fn find(
    o: &Outline,
    re: &Regex,
    from: &Hit,
    direction: Direction,
    scope: Scope,
) -> Option<(Hit, bool)> {
    let all = o.all_positions();
    let n = all.len();
    let start = all.iter().position(|p| *p == from.node).unwrap_or(0);
    let hit = |i: usize, place: Place| Hit {
        node: all[i].clone(),
        place,
    };
    let here = places(o, &all[start], re, scope);
    match direction {
        Direction::Forward => {
            if let Some(&p) = here.iter().find(|p| **p > from.place) {
                return Some((hit(start, p), false));
            }
            for step in 1..n {
                let i = (start + step) % n;
                if let Some(&p) = places(o, &all[i], re, scope).first() {
                    return Some((hit(i, p), start + step >= n));
                }
            }
            here.first().map(|&p| (hit(start, p), true))
        }
        Direction::Backward => {
            if let Some(&p) = here.iter().rev().find(|p| **p < from.place) {
                return Some((hit(start, p), false));
            }
            for step in 1..n {
                let i = (start + n - step) % n;
                if let Some(&p) = places(o, &all[i], re, scope).last() {
                    return Some((hit(i, p), step > start));
                }
            }
            here.last().map(|&p| (hit(start, p), true))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Alpha, beta, Gamma, delta, with needles in two bodies.
    fn outline() -> (Outline, Vec<Position>) {
        let mut o = Outline::new_empty();
        for (i, h) in ["Alpha", "beta", "Gamma", "delta"].iter().enumerate() {
            let p = if i == 0 {
                o.root_position().unwrap()
            } else {
                let last = o.all_positions().last().unwrap().clone();
                o.insert_after(&last)
            };
            o.set_headline(&p, h);
        }
        let all = o.all_positions();
        o.set_body(&all[0], "x needle\nneedle y\n");
        o.set_body(&all[2], "needle\n");
        (o, all)
    }

    fn at(node: &Position, place: Place) -> Hit {
        Hit {
            node: node.clone(),
            place,
        }
    }

    fn next(o: &Outline, pattern: &str, from: &Hit, d: Direction, s: Scope) -> Option<(Hit, bool)> {
        find(o, &compile(pattern).unwrap(), from, d, s)
    }

    #[test]
    fn a_walk_visits_each_headline_then_its_body_and_wraps() {
        let (o, all) = outline();
        let f = Direction::Forward;
        let start = at(&all[0], Place::Headline(0));
        let first = next(&o, "needle", &start, f, Scope::All).unwrap();
        assert_eq!(first, (at(&all[0], Place::Body(0, 2)), false));
        let second = next(&o, "needle", &first.0, f, Scope::All).unwrap();
        assert_eq!(second, (at(&all[0], Place::Body(1, 0)), false));
        let third = next(&o, "needle", &second.0, f, Scope::All).unwrap();
        assert_eq!(third, (at(&all[2], Place::Body(0, 0)), false));
        let wrapped = next(&o, "needle", &third.0, f, Scope::All).unwrap();
        assert_eq!(wrapped, (at(&all[0], Place::Body(0, 2)), true));
    }

    #[test]
    fn backward_walks_the_other_way_and_wraps() {
        let (o, all) = outline();
        let b = Direction::Backward;
        let from = at(&all[0], Place::Body(1, 0));
        let hit = next(&o, "needle", &from, b, Scope::All).unwrap();
        assert_eq!(hit, (at(&all[0], Place::Body(0, 2)), false));
        let hit = next(&o, "needle", &hit.0, b, Scope::All).unwrap();
        assert_eq!(hit, (at(&all[2], Place::Body(0, 0)), true));
    }

    #[test]
    fn the_headline_scope_leaves_bodies_out() {
        let (o, all) = outline();
        let from = at(&all[0], Place::Headline(0));
        assert!(next(&o, "needle", &from, Direction::Forward, Scope::Headlines).is_none());
        let hit = next(&o, "delta", &from, Direction::Forward, Scope::Headlines).unwrap();
        assert_eq!(hit.0.node, all[3]);
    }

    #[test]
    fn case_is_smart_and_the_pattern_is_a_regex() {
        let (o, all) = outline();
        let from = at(&all[0], Place::Headline(0));
        let f = Direction::Forward;
        assert_eq!(
            next(&o, "gamma", &from, f, Scope::All).unwrap().0.node,
            all[2]
        );
        assert!(next(&o, "Beta", &from, f, Scope::All).is_none());
        assert_eq!(
            next(&o, "g.mma", &from, f, Scope::All).unwrap().0.node,
            all[2]
        );
        assert!(!has_capital(r"\Sx"));
        assert!(compile("(").is_err());
    }

    #[test]
    fn a_lone_match_wraps_round_to_itself() {
        let (o, all) = outline();
        let only = at(&all[3], Place::Headline(0));
        let hit = next(&o, "delta", &only, Direction::Forward, Scope::All).unwrap();
        assert_eq!(hit, (only, true));
    }

    #[test]
    fn ranges_leave_out_empty_matches() {
        let re = compile("x*").unwrap();
        assert_eq!(ranges(&re, "axxbx"), [1..3, 4..5]);
    }
}
