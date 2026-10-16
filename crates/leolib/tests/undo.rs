//! Undo and redo over random sequences of edits: undoing every step gives
//! back the starting outline, and redoing every step the final one.

use leolib::{Document, Outline, Position};

/// Every position's gnx, level, headline, body and mark, in outline order:
/// enough to see a lost clone link, a wrong parent or a stale body.
fn digest(o: &Outline) -> Vec<(String, usize, String, String, bool)> {
    o.all_positions()
        .iter()
        .map(|p| {
            (
                p.gnx(o).to_string(),
                p.level(),
                p.h(o).to_string(),
                p.b(o).to_string(),
                p.is_marked(o),
            )
        })
        .collect()
}

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n.max(1)
    }
}

/// One random edit at a random node.
fn edit(doc: &mut Document, rng: &mut Rng, step: usize) -> String {
    let all = doc.outline().all_positions();
    let p: Position = all[rng.below(all.len())].clone();
    let k = rng.below(17);
    let label = format!("op {k} at {} level {}", p.h(doc.outline()), p.level());
    match k {
        0 => doc.set_headline(&p, &format!("h{step}")),
        1 => doc.set_body(&p, &format!("body {step}\nline\n")),
        2 => {
            doc.insert_node(&p);
        }
        3 => {
            doc.insert_child(&p);
        }
        4 => {
            doc.delete_node(&p);
        }
        5 => {
            doc.clone_node(&p);
        }
        6 => {
            doc.demote(&p);
        }
        7 => {
            doc.promote(&p);
        }
        8 => {
            doc.move_up(&p);
        }
        9 => {
            doc.move_down(&p);
        }
        10 => {
            doc.move_left(&p);
        }
        11 => {
            doc.move_right(&p);
        }
        12 => doc.toggle_marked(&p),
        13 => {
            doc.clone_marked(&p);
        }
        14 => {
            doc.move_marked(&p);
        }
        15 => {
            doc.clone_find_all("h", "", false, |o, q| q.h(o).starts_with('h'));
        }
        _ => {
            doc.sort_children(&p);
        }
    }
    label
}

/// Whether every node in the tree lists each parent once per link from it.
///
/// A stale or missing parent link leaves an edit below it not dirtying its
/// `@<file>`, so the edit is never saved.
fn links_agree(o: &Outline) -> Result<(), String> {
    let mut want: std::collections::HashMap<leolib::VnodeId, Vec<leolib::VnodeId>> =
        std::collections::HashMap::new();
    let mut seen = std::collections::HashSet::new();
    let mut todo = vec![o.hidden_root];
    while let Some(u) = todo.pop() {
        if !seen.insert(u) {
            continue;
        }
        for &c in &o.node(u).children {
            want.entry(c).or_default().push(u);
            todo.push(c);
        }
    }
    for (v, mut w) in want {
        let mut got = o.node(v).parents.clone();
        w.sort();
        got.sort();
        if got != w {
            return Err(format!("{v:?} has parents {got:?}, links from {w:?}"));
        }
    }
    Ok(())
}

/// Whether some node is its own descendant, which makes the outline endless.
fn has_cycle(o: &Outline) -> bool {
    fn reaches(o: &Outline, from: leolib::VnodeId, target: leolib::VnodeId, depth: usize) -> bool {
        depth > 200
            || o.node(from)
                .children
                .iter()
                .any(|&c| c == target || reaches(o, c, target, depth + 1))
    }
    let root = o.root_position().unwrap();
    let mut vs: Vec<leolib::VnodeId> = vec![root.v];
    let mut seen = std::collections::HashSet::new();
    while let Some(v) = vs.pop() {
        if !seen.insert(v) {
            continue;
        }
        if reaches(o, v, v, 0) {
            return true;
        }
        vs.extend(o.node(v).children.iter().copied());
        vs.extend(o.node(v).parents.iter().copied());
    }
    false
}

#[test]
fn undoing_every_step_restores_the_start_and_redoing_restores_the_end() {
    for seed in 0..300u64 {
        let mut doc = Document::new_empty("");
        let root = doc.outline().root_position().unwrap();
        doc.set_headline(&root, "root");
        doc.clear_undo();
        let mut rng = Rng(seed);
        let start = digest(doc.outline());
        for step in 0..40 {
            let op = edit(&mut doc, &mut rng, step);
            assert!(
                !has_cycle(doc.outline()),
                "seed {seed} step {step}: {op} made a cycle"
            );
            if let Err(e) = links_agree(doc.outline()) {
                panic!("seed {seed} step {step}: {op}: {e}");
            }
        }
        let end = digest(doc.outline());
        while doc.undoer().can_undo() {
            doc.undo();
            if let Err(e) = links_agree(doc.outline()) {
                panic!("seed {seed}: undo: {e}");
            }
        }
        assert_eq!(digest(doc.outline()), start, "seed {seed}: undo all");
        while doc.undoer().can_redo() {
            doc.redo();
            if let Err(e) = links_agree(doc.outline()) {
                panic!("seed {seed}: redo: {e}");
            }
        }
        assert_eq!(digest(doc.outline()), end, "seed {seed}: redo all");
    }
}

/// Two top-level nodes, the second a clone of the first.
fn node_and_its_clone() -> (Document, Position) {
    let mut doc = Document::new_empty("");
    let a = doc.outline().root_position().unwrap();
    doc.set_headline(&a, "a");
    doc.clone_node(&a);
    (doc, a)
}

#[test]
fn a_node_cannot_be_demoted_or_moved_into_its_own_clone() {
    let (mut doc, a) = node_and_its_clone();
    assert!(!doc.demote(&a));
    let clone = a.next(doc.outline()).unwrap();
    assert!(doc.move_right(&clone).is_none());
    assert!(!has_cycle(doc.outline()));
    assert_eq!(doc.outline().all_positions().len(), 2);
}

#[test]
fn the_only_top_level_node_is_not_deleted() {
    let mut doc = Document::new_empty("");
    let a = doc.outline().root_position().unwrap();
    let child = doc.insert_child(&a);
    doc.set_headline(&child, "kept");
    assert!(doc.delete_node(&a).is_none());
    assert_eq!(doc.outline().all_positions().len(), 2);
}

/// Top-level nodes headed `names`, and their positions.
fn tops(names: &[&str]) -> (Document, Vec<Position>) {
    let mut doc = Document::new_empty("");
    let mut p = doc.outline().root_position().unwrap();
    doc.set_headline(&p, names[0]);
    for name in &names[1..] {
        p = doc.insert_node(&p);
        doc.set_headline(&p, name);
    }
    doc.clear_undo();
    let all = doc.outline().all_positions();
    (doc, all)
}

fn heads(doc: &Document) -> Vec<String> {
    let o = doc.outline();
    o.all_positions()
        .iter()
        .map(|p| format!("{}{}", " ".repeat(p.level()), p.h(o)))
        .collect()
}

#[test]
fn several_nodes_move_together_in_order_and_undo_as_one() {
    let (mut doc, ps) = tops(&["a", "b", "c", "d"]);
    let before = heads(&doc);
    let moved = doc
        .move_nodes(
            &[ps[2].clone(), ps[0].clone()],
            &ps[3],
            leolib::Place::Inside,
        )
        .unwrap();
    assert_eq!(moved.len(), 2);
    assert_eq!(heads(&doc), ["b", "d", " a", " c"]);
    doc.undo();
    assert_eq!(heads(&doc), before);
    // Into one of the nodes moved: refused, nothing moved.
    let ps = doc.outline().all_positions();
    assert!(doc
        .move_nodes(
            &[ps[0].clone(), ps[1].clone()],
            &ps[1],
            leolib::Place::Inside
        )
        .is_none());
    assert_eq!(heads(&doc), before);
}

#[test]
fn several_nodes_delete_as_one_and_the_last_top_node_stays() {
    let (mut doc, ps) = tops(&["a", "b", "c"]);
    let (n, _) = doc.delete_nodes(&[ps[0].clone(), ps[2].clone()]);
    assert_eq!(n, 2);
    assert_eq!(heads(&doc), ["b"]);
    doc.undo();
    assert_eq!(heads(&doc), ["a", "b", "c"]);
    let ps = doc.outline().all_positions();
    let (n, _) = doc.delete_nodes(&ps);
    assert_eq!(n, 2);
    assert_eq!(doc.outline().all_positions().len(), 1);
}

/// Undoing a delete restored the node but not its children's parent links,
/// so an edit below it never dirtied the `@file` and was not saved.
#[test]
fn an_edit_below_an_undeleted_node_dirties_its_file() {
    let mut doc = Document::new_empty("");
    let root = doc.outline().root_position().unwrap();
    doc.set_headline(&root, "@file f.py");
    let a = doc.insert_child(&root);
    let g = doc.insert_child(&a);
    doc.set_headline(&g, "g");
    doc.delete_node(&a);
    doc.undo();
    links_agree(doc.outline()).unwrap();
    let root = doc.outline().root_position().unwrap();
    doc.outline_mut_untracked().clear_dirty_in_tree(&root);
    let g = doc
        .outline()
        .all_positions()
        .into_iter()
        .find(|p| p.h(doc.outline()) == "g")
        .unwrap();
    doc.set_body(&g, "x = 1\n");
    assert!(doc
        .outline()
        .root_position()
        .unwrap()
        .is_dirty(doc.outline()));
}

/// The new node went inside the marked tree, then the marked tree under it.
#[test]
fn clone_marked_from_inside_a_marked_tree_makes_no_cycle() {
    let mut doc = Document::new_empty("");
    let a = doc.outline().root_position().unwrap();
    doc.set_headline(&a, "a");
    let b = doc.insert_child(&a);
    doc.toggle_marked(&a);
    assert!(doc.clone_marked(&b).is_some());
    assert!(!has_cycle(doc.outline()));
    links_agree(doc.outline()).unwrap();
}

/// Through a clone, the place after `current` had a parent inside the new tree.
#[test]
fn move_marked_after_a_clone_inside_the_moved_tree_makes_no_cycle() {
    let mut doc = Document::new_empty("");
    let m = doc.outline().root_position().unwrap();
    doc.set_headline(&m, "m");
    let x = doc.insert_child(&m);
    doc.set_headline(&x, "x");
    let y = doc.insert_child(&x);
    doc.set_headline(&y, "y");
    doc.toggle_marked(&m);
    let x = doc
        .outline()
        .all_positions()
        .into_iter()
        .find(|p| p.h(doc.outline()) == "x")
        .unwrap();
    let xc = doc.clone_node(&x);
    doc.move_node(&xc, &m, leolib::Place::After).unwrap();
    let y_under_top_x = doc
        .outline()
        .all_positions()
        .into_iter()
        .find(|p| p.h(doc.outline()) == "y" && p.level() == 1)
        .unwrap();
    assert!(doc.move_marked(&y_under_top_x).is_some());
    assert!(!has_cycle(doc.outline()));
    links_agree(doc.outline()).unwrap();
}
