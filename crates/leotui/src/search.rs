//! Incremental search, over headlines in the tree and text in the body.
//!
//! Focus decides what is searched, as it decides everything else: `/` in the
//! outline walks headlines, `/` in the body walks that node's text.

use leolib::{Outline, Position};

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

/// What a tree search looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Headlines,
    /// Headlines and body text, which `:set search=all` turns on.
    All,
}

/// The last search, so `n` and `N` can repeat it.
#[derive(Clone)]
pub struct LastSearch {
    pub pattern: String,
    pub direction: Direction,
}

/// vim's smartcase: an all-lowercase pattern ignores case, one with any
/// capital does not. It is what a user means without having to say so.
fn matches(haystack: &str, pattern: &str) -> bool {
    if pattern.chars().any(|c| c.is_uppercase()) {
        haystack.contains(pattern)
    } else {
        haystack.to_lowercase().contains(&pattern.to_lowercase())
    }
}

/// The first node matching `pattern`, starting after `from` and wrapping once.
pub fn find_node(
    o: &Outline,
    from: &Position,
    pattern: &str,
    direction: Direction,
    scope: Scope,
) -> Option<Position> {
    if pattern.is_empty() {
        return None;
    }
    let all = o.all_positions();
    let start = all.iter().position(|p| p == from)?;
    let n = all.len();
    for step in 1..=n {
        let i = match direction {
            Direction::Forward => (start + step) % n,
            Direction::Backward => (start + n - step % n) % n,
        };
        let p = &all[i];
        let hit = matches(p.h(o), pattern) || (scope == Scope::All && matches(p.b(o), pattern));
        if hit {
            return Some(p.clone());
        }
    }
    None
}

/// The next occurrence of `pattern` in `lines`, as (row, column) in characters.
pub fn find_in_lines(
    lines: &[String],
    from: (usize, usize),
    pattern: &str,
    direction: Direction,
) -> Option<(usize, usize)> {
    if pattern.is_empty() || lines.is_empty() {
        return None;
    }
    let n = lines.len();
    let (row, col) = from;
    // The starting line is searched twice: after the cursor first, then from
    // its start when the search wraps all the way round.
    for step in 0..=n {
        let r = match direction {
            Direction::Forward => (row + step) % n,
            Direction::Backward => (row + n - step % n) % n,
        };
        let line = &lines[r];
        let hit = if step == 0 {
            match direction {
                Direction::Forward => {
                    let start = char_index(line, col + 1);
                    find_at(&line[start..], pattern).map(|i| char_count(&line[..start + i]))
                }
                Direction::Backward => {
                    let end = char_index(line, col);
                    rfind_at(&line[..end], pattern).map(|i| char_count(&line[..i]))
                }
            }
        } else {
            match direction {
                Direction::Forward => find_at(line, pattern).map(|i| char_count(&line[..i])),
                Direction::Backward => rfind_at(line, pattern).map(|i| char_count(&line[..i])),
            }
        };
        if let Some(c) = hit {
            return Some((r, c));
        }
    }
    None
}

fn find_at(haystack: &str, pattern: &str) -> Option<usize> {
    if pattern.chars().any(|c| c.is_uppercase()) {
        haystack.find(pattern)
    } else {
        haystack.to_lowercase().find(&pattern.to_lowercase())
    }
}

fn rfind_at(haystack: &str, pattern: &str) -> Option<usize> {
    if pattern.chars().any(|c| c.is_uppercase()) {
        haystack.rfind(pattern)
    } else {
        haystack.to_lowercase().rfind(&pattern.to_lowercase())
    }
}

fn char_index(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map(|(i, _)| i).unwrap_or(s.len())
}

fn char_count(s: &str) -> usize {
    s.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use leolib::Outline;

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
        (o, all)
    }

    #[test]
    fn a_lowercase_pattern_ignores_case() {
        let (o, all) = outline();
        let hit = find_node(&o, &all[0], "gamma", Direction::Forward, Scope::Headlines);
        assert_eq!(hit.unwrap().h(&o), "Gamma");
    }

    #[test]
    fn a_pattern_with_a_capital_is_case_sensitive() {
        let (o, all) = outline();
        assert!(find_node(&o, &all[0], "Beta", Direction::Forward, Scope::Headlines).is_none());
        let hit = find_node(&o, &all[0], "Gamma", Direction::Forward, Scope::Headlines);
        assert_eq!(hit.unwrap().h(&o), "Gamma");
    }

    #[test]
    fn search_wraps_round_the_outline() {
        let (o, all) = outline();
        let hit = find_node(&o, &all[3], "alpha", Direction::Forward, Scope::Headlines);
        assert_eq!(hit.unwrap().h(&o), "Alpha");
    }

    #[test]
    fn backward_search_walks_the_other_way() {
        let (o, all) = outline();
        let hit = find_node(&o, &all[3], "a", Direction::Backward, Scope::Headlines);
        assert_eq!(hit.unwrap().h(&o), "Gamma");
    }

    #[test]
    fn body_text_is_searched_only_with_the_wider_scope() {
        let (mut o, all) = outline();
        o.set_body(&all[2], "a needle in here\n");
        assert!(find_node(&o, &all[0], "needle", Direction::Forward, Scope::Headlines).is_none());
        let hit = find_node(&o, &all[0], "needle", Direction::Forward, Scope::All);
        assert_eq!(hit.unwrap().h(&o), "Gamma");
    }

    #[test]
    fn body_search_finds_the_next_occurrence_and_wraps() {
        let lines: Vec<String> = ["one two", "three two", "four"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            find_in_lines(&lines, (0, 0), "two", Direction::Forward),
            Some((0, 4))
        );
        assert_eq!(
            find_in_lines(&lines, (0, 4), "two", Direction::Forward),
            Some((1, 6))
        );
        // Wraps back to the first.
        assert_eq!(
            find_in_lines(&lines, (1, 6), "two", Direction::Forward),
            Some((0, 4))
        );
        assert_eq!(
            find_in_lines(&lines, (1, 6), "two", Direction::Backward),
            Some((0, 4))
        );
    }
}
