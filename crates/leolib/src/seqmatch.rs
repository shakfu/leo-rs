//! A port of Python's `difflib.SequenceMatcher`, restricted to what the
//! `@clean` update algorithm uses.
//!
//! Not any diff: the `@clean` algorithm interleaves sentinels between the
//! opcodes this produces, so a different set of opcodes puts sentinels in
//! different places and rebuilds a different outline. Matching Python's
//! choices, including its "autojunk" heuristic, is the whole point.

use std::collections::HashMap;
use std::hash::Hash;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    Equal,
    Replace,
    Delete,
    Insert,
}

/// `(tag, a_start, a_end, b_start, b_end)`, as `get_opcodes` returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Opcode {
    pub tag: Tag,
    pub ai: usize,
    pub aj: usize,
    pub bi: usize,
    pub bj: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Match {
    i: usize,
    j: usize,
    size: usize,
}

pub struct SequenceMatcher<'a, T: Hash + Eq> {
    a: &'a [T],
    b: &'a [T],
    b2j: HashMap<&'a T, Vec<usize>>,
}

impl<'a, T: Hash + Eq> SequenceMatcher<'a, T> {
    pub fn new(a: &'a [T], b: &'a [T]) -> Self {
        let mut b2j: HashMap<&T, Vec<usize>> = HashMap::new();
        for (i, elt) in b.iter().enumerate() {
            b2j.entry(elt).or_default().push(i);
        }
        // Autojunk: in a long sequence, an element appearing in more than 1%
        // of it is never used to seed a match. Without this the matcher is
        // quadratic on files full of blank lines -- and Python's opcodes
        // would differ, which is what actually matters here.
        let n = b.len();
        if n >= 200 {
            let ntest = n / 100 + 1;
            b2j.retain(|_, idxs| idxs.len() <= ntest);
        }
        Self { a, b, b2j }
    }

    fn find_longest_match(&self, alo: usize, ahi: usize, blo: usize, bhi: usize) -> Match {
        let (mut besti, mut bestj, mut bestsize) = (alo, blo, 0usize);
        let mut j2len: HashMap<usize, usize> = HashMap::new();
        for i in alo..ahi {
            let mut newj2len: HashMap<usize, usize> = HashMap::new();
            if let Some(indices) = self.b2j.get(&self.a[i]) {
                for &j in indices {
                    if j < blo {
                        continue;
                    }
                    if j >= bhi {
                        break;
                    }
                    let k = j
                        .checked_sub(1)
                        .and_then(|jm| j2len.get(&jm).copied())
                        .unwrap_or(0)
                        + 1;
                    newj2len.insert(j, k);
                    if k > bestsize {
                        besti = i + 1 - k;
                        bestj = j + 1 - k;
                        bestsize = k;
                    }
                }
            }
            j2len = newj2len;
        }
        // Extend the match over elements the index dropped as too popular.
        while besti > alo && bestj > blo && self.a[besti - 1] == self.b[bestj - 1] {
            besti -= 1;
            bestj -= 1;
            bestsize += 1;
        }
        while besti + bestsize < ahi
            && bestj + bestsize < bhi
            && self.a[besti + bestsize] == self.b[bestj + bestsize]
        {
            bestsize += 1;
        }
        Match {
            i: besti,
            j: bestj,
            size: bestsize,
        }
    }

    fn matching_blocks(&self) -> Vec<Match> {
        let (la, lb) = (self.a.len(), self.b.len());
        let mut queue = vec![(0usize, la, 0usize, lb)];
        let mut blocks = Vec::new();
        while let Some((alo, ahi, blo, bhi)) = queue.pop() {
            let m = self.find_longest_match(alo, ahi, blo, bhi);
            if m.size > 0 {
                blocks.push(m);
                if alo < m.i && blo < m.j {
                    queue.push((alo, m.i, blo, m.j));
                }
                if m.i + m.size < ahi && m.j + m.size < bhi {
                    queue.push((m.i + m.size, ahi, m.j + m.size, bhi));
                }
            }
        }
        blocks.sort_by_key(|m| (m.i, m.j, m.size));
        // Collapse adjacent blocks.
        let (mut i1, mut j1, mut k1) = (0usize, 0usize, 0usize);
        let mut out = Vec::new();
        for m in blocks {
            if i1 + k1 == m.i && j1 + k1 == m.j {
                k1 += m.size;
            } else {
                if k1 > 0 {
                    out.push(Match {
                        i: i1,
                        j: j1,
                        size: k1,
                    });
                }
                i1 = m.i;
                j1 = m.j;
                k1 = m.size;
            }
        }
        if k1 > 0 {
            out.push(Match {
                i: i1,
                j: j1,
                size: k1,
            });
        }
        out.push(Match {
            i: la,
            j: lb,
            size: 0,
        });
        out
    }

    pub fn opcodes(&self) -> Vec<Opcode> {
        let (mut i, mut j) = (0usize, 0usize);
        let mut out = Vec::new();
        for m in self.matching_blocks() {
            let tag = if i < m.i && j < m.j {
                Some(Tag::Replace)
            } else if i < m.i {
                Some(Tag::Delete)
            } else if j < m.j {
                Some(Tag::Insert)
            } else {
                None
            };
            if let Some(tag) = tag {
                out.push(Opcode {
                    tag,
                    ai: i,
                    aj: m.i,
                    bi: j,
                    bj: m.j,
                });
            }
            i = m.i + m.size;
            j = m.j + m.size;
            if m.size > 0 {
                out.push(Opcode {
                    tag: Tag::Equal,
                    ai: m.i,
                    aj: i,
                    bi: m.j,
                    bj: j,
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(a: &[&str], b: &[&str]) -> Vec<(Tag, usize, usize, usize, usize)> {
        let a: Vec<String> = a.iter().map(|s| s.to_string()).collect();
        let b: Vec<String> = b.iter().map(|s| s.to_string()).collect();
        SequenceMatcher::new(&a, &b)
            .opcodes()
            .into_iter()
            .map(|o| (o.tag, o.ai, o.aj, o.bi, o.bj))
            .collect()
    }

    #[test]
    fn identical_sequences_are_one_equal_block() {
        assert_eq!(
            tags(&["a", "b"], &["a", "b"]),
            vec![(Tag::Equal, 0, 2, 0, 2)]
        );
    }

    #[test]
    fn opcodes_match_pythons_for_a_mixed_edit() {
        // Checked against difflib: qabxcd -> abycdf
        assert_eq!(
            tags(
                &["q", "a", "b", "x", "c", "d"],
                &["a", "b", "y", "c", "d", "f"]
            ),
            vec![
                (Tag::Delete, 0, 1, 0, 0),
                (Tag::Equal, 1, 3, 0, 2),
                (Tag::Replace, 3, 4, 2, 3),
                (Tag::Equal, 4, 6, 3, 5),
                (Tag::Insert, 6, 6, 5, 6),
            ]
        );
    }

    #[test]
    fn an_empty_source_is_one_insert() {
        assert_eq!(tags(&[], &["a"]), vec![(Tag::Insert, 0, 0, 0, 1)]);
    }
}
