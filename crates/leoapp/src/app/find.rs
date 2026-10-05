//! The find panel's searches: find all, replace all and clone-find-all, over
//! the outline, the selected subtree or the marked nodes.

use super::*;
use crate::search::{Find, FindScope, Found};

/// `n match` or `n matches`: `plural` only adds an s.
fn matches(n: usize) -> String {
    match n {
        1 => "1 match".to_string(),
        n => format!("{n} matches"),
    }
}

impl App {
    /// The nodes `scope` covers, in outline order.
    fn find_nodes(&self, scope: FindScope) -> Vec<Position> {
        let o = self.outline();
        match scope {
            FindScope::Outline => o.all_positions(),
            FindScope::Subtree => self.current.self_and_subtree(o),
            FindScope::Marked => o
                .all_positions()
                .into_iter()
                .filter(|p| p.is_marked(o))
                .collect(),
        }
    }

    /// Make `find` the search `n` and `N` repeat, its matches lit.
    fn remember_find(&mut self, find: &Find, re: regex::Regex) {
        self.last_search = Some(LastSearch {
            pattern: find.as_regex(),
            direction: Direction::Forward,
        });
        self.hlsearch = Some(re);
    }

    /// Every match of `find`, for the panel to list. None, with a message,
    /// if the pattern is bad.
    pub fn find_all(&mut self, find: &Find) -> Option<Vec<Found>> {
        let re = match find.compile() {
            Ok(re) => re,
            Err(e) => {
                self.message = e;
                return None;
            }
        };
        let nodes = self.find_nodes(find.scope);
        let found = crate::search::find_all(self.outline(), &nodes, find, &re);
        self.message = format!("found {}", matches(found.len()));
        self.remember_find(find, re);
        Some(found)
    }

    /// Go to a match the panel listed, if its node is still there.
    pub fn go_to_found(&mut self, found: &Found) {
        if !self.outline().position_exists(&found.node) {
            self.message = "the node is gone; find again".into();
            return;
        }
        self.land(search::Hit {
            node: found.node.clone(),
            place: found.place,
        });
    }

    /// Replace every match of `find` with `with`, as one undo step. A regex
    /// replacement may name groups, `$1`; a plain one is taken as typed.
    pub fn replace_all(&mut self, find: &Find, with: &str) {
        let re = match find.compile() {
            Ok(re) => re,
            Err(e) => return self.message = e,
        };
        let replace = |text: &str| -> (String, usize) {
            let n = re.find_iter(text).filter(|m| !m.is_empty()).count();
            let out = match find.regex {
                true => re.replace_all(text, with),
                false => re.replace_all(text, regex::NoExpand(with)),
            };
            (out.into_owned(), n)
        };
        let mut seen = std::collections::HashSet::new();
        let mut edits = Vec::new();
        let mut count = 0;
        let o = self.outline();
        for p in self.find_nodes(find.scope) {
            if !seen.insert(p.v) {
                continue;
            }
            let h = match find.headlines {
                true => replace(p.h(o)),
                false => (String::new(), 0),
            };
            let b = match find.bodies {
                true => replace(p.b(o)),
                false => (String::new(), 0),
            };
            if h.1 + b.1 > 0 {
                count += h.1 + b.1;
                edits.push((p, (h.1 > 0).then_some(h.0), (b.1 > 0).then_some(b.0)));
            }
        }
        let nodes = edits.len();
        self.doc.begin_group("replace-all");
        for (p, h, b) in edits {
            if let Some(h) = h {
                self.doc.set_headline(&p, &h);
            }
            if let Some(b) = b {
                self.doc.set_body(&p, &b);
            }
        }
        self.doc.end_group();
        self.buffer = None;
        let lines = self.body_buffer();
        self.editor.clamp(&lines);
        self.message = format!("replaced {} in {}", matches(count), plural(nodes, "node"));
        self.remember_find(find, re);
    }

    /// Leo's `clone-find-all` with the panel's settings: the matching nodes
    /// in scope, cloned under a new `Found` node.
    pub fn clone_find(&mut self, find: &Find, flatten: bool) {
        let re = match find.compile() {
            Ok(re) => re,
            Err(e) => return self.message = e,
        };
        let scope: std::collections::HashSet<leolib::VnodeId> = match find.scope {
            FindScope::Outline => Default::default(),
            other => self.find_nodes(other).iter().map(|p| p.v).collect(),
        };
        let mut status = vec![if find.regex { "Regex" } else { "Plain" }];
        if find.ignore_case {
            status.insert(0, "Ignore Case");
        }
        if find.whole_word {
            status.push("Whole Word");
        }
        if find.headlines {
            status.push("Head");
        }
        if find.bodies {
            status.push("Body");
        }
        let everywhere = find.scope == FindScope::Outline;
        let matches = |o: &Outline, p: &Position| {
            (everywhere || scope.contains(&p.v)) && find.matches(&re, o, p)
        };
        let result = self
            .doc
            .clone_find_all(&find.pattern, &status.join(", "), flatten, matches);
        match result {
            Some((found, n)) => {
                self.focus = Focus::Tree;
                self.select(found);
                self.message = format!("found {n} for {}", find.pattern);
            }
            None => self.message = format!("found 0 for {}", find.pattern),
        }
        self.remember_find(find, re);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::Place;

    /// one (body "cat\ncat dog"), two (body "dog"), with child three
    /// (headline "cat three").
    fn app() -> App {
        let mut doc = Document::new_empty("");
        let one = doc.outline().root_position().unwrap();
        doc.set_headline(&one, "one");
        doc.set_body(&one, "cat\ncat dog\n");
        let two = doc.outline_mut_untracked().insert_after(&one);
        doc.set_headline(&two, "two");
        doc.set_body(&two, "dog\n");
        let three = doc.outline_mut_untracked().insert_as_last_child(&two);
        doc.set_headline(&three, "cat three");
        doc.clear_undo();
        App::new(doc)
    }

    fn cat() -> Find {
        Find {
            pattern: "cat".into(),
            ..Find::default()
        }
    }

    #[test]
    fn find_all_lists_matches_in_scope_and_goes_to_one() {
        let mut app = app();
        let found = app.find_all(&cat()).unwrap();
        assert_eq!(found.len(), 3);
        assert_eq!(app.message, "found 3 matches");
        assert_eq!(found[2].place, Place::Headline(0));
        app.go_to_found(&found[1]);
        assert_eq!(app.focus, Focus::Body);
        assert_eq!(app.editor.cursor, (1, 0));
        // `n` goes on with the panel's search.
        assert!(app.last_search.is_some() && app.hlsearch.is_some());

        let two = app.row_position(1).unwrap();
        app.select(two);
        let subtree = Find {
            scope: FindScope::Subtree,
            ..cat()
        };
        assert_eq!(app.find_all(&subtree).unwrap().len(), 1);
        let marked = Find {
            scope: FindScope::Marked,
            ..cat()
        };
        assert!(app.find_all(&marked).unwrap().is_empty());
    }

    #[test]
    fn replace_all_is_one_undo_step() {
        let mut app = app();
        let whole = Find {
            whole_word: true,
            ..cat()
        };
        app.replace_all(&whole, "$cow");
        assert_eq!(app.message, "replaced 3 matches in 2 nodes");
        let o = app.outline();
        let all = o.all_positions();
        assert_eq!(all[0].b(o), "$cow\n$cow dog\n");
        assert_eq!(all[2].h(o), "$cow three");
        app.doc.undo();
        let o = app.outline();
        assert_eq!(o.all_positions()[0].b(o), "cat\ncat dog\n");
        assert_eq!(o.all_positions()[2].h(o), "cat three");

        let groups = Find {
            pattern: "(c)at".into(),
            regex: true,
            headlines: false,
            ..Find::default()
        };
        app.replace_all(&groups, "${1}ow");
        assert_eq!(app.message, "replaced 2 matches in 1 node");
        assert_eq!(
            app.outline().all_positions()[0].b(app.outline()),
            "cow\ncow dog\n"
        );
    }

    #[test]
    fn clone_find_keeps_to_the_scope() {
        let mut app = app();
        let two = app.row_position(1).unwrap();
        app.select(two);
        app.clone_find(
            &Find {
                scope: FindScope::Subtree,
                ..cat()
            },
            false,
        );
        assert_eq!(app.message, "found 1 for cat");
        let o = app.outline();
        let found = app.current.clone();
        assert!(found.h(o).starts_with("Found 1:cat"));
        assert_eq!(found.first_child(o).unwrap().h(o), "cat three");
    }
}
