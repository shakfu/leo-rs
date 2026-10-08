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
    let k = rng.below(14);
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
        _ => {
            doc.sort_children(&p);
        }
    }
    label
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
        }
        let end = digest(doc.outline());
        while doc.undoer().can_undo() {
            doc.undo();
        }
        assert_eq!(digest(doc.outline()), start, "seed {seed}: undo all");
        while doc.undoer().can_redo() {
            doc.redo();
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
