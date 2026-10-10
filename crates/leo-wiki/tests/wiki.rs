//! `@wiki`: links, resolution, rules, renaming and export.

use leo_wiki::{check, export, links, rendered, resolve, slug, wikilinks_to_unls, Wiki};
use leolib::{Document, Outline, Position};

/// An outline holding, by indentation in `tree`, `headline|body` lines;
/// `\n` in a body is written `\\n`.
fn outline(tree: &[(usize, &str, &str)]) -> Document {
    let kinds = leolib::ext::Kinds::empty().with_tree(Wiki).unwrap();
    let mut doc = Document::new_empty("");
    doc.outline_mut_untracked()
        .set_kinds(std::sync::Arc::new(kinds));
    let mut stack: Vec<Position> = Vec::new();
    for (i, (depth, h, b)) in tree.iter().enumerate() {
        let p = match (i, *depth) {
            (0, _) => doc.outline().root_position().unwrap(),
            (_, 0) => {
                let last = stack[0].clone();
                doc.outline_mut_untracked().insert_after(&last)
            }
            (_, d) => {
                let parent = stack[d - 1].clone();
                doc.outline_mut_untracked().insert_as_last_child(&parent)
            }
        };
        doc.set_headline(&p, h);
        doc.set_body(&p, b);
        stack.truncate(*depth);
        stack.push(p);
    }
    doc.clear_undo();
    doc
}

fn find(o: &Outline, h: &str) -> Position {
    o.all_positions()
        .into_iter()
        .find(|p| p.h(o) == h)
        .unwrap_or_else(|| panic!("no {h}"))
}

fn notes() -> Document {
    outline(&[
        (0, "@wiki notes", "Start at [[Intro]].\n"),
        (
            1,
            "Intro",
            "See [[Setup|how to set up]] and [[Guide/Setup]].\n",
        ),
        (1, "Guide", ""),
        (2, "Setup", "Install it.\n\n## Steps\n\nRun it.\n"),
        (
            0,
            "@wiki other",
            "Back to [[notes:Intro]] and [[notes:]].\n",
        ),
        (1, "Setup", "Elsewhere.\n"),
    ])
}

#[test]
fn links_are_read_with_their_wiki_path_and_label_outside_code() {
    let body = "a [[A\\/B|x\\|y]] b `[[code]]` [[w:P/Q]] [[Note: x]]\n```\n[[fenced]]\n```\n";
    let found = links(body);
    assert_eq!(found.len(), 3);
    assert_eq!(
        (found[0].whole.clone(), found[0].label.clone()),
        (vec!["A/B".to_string()], Some("x|y".to_string()))
    );
    assert_eq!(
        (found[1].wiki.as_deref(), found[1].path.clone()),
        (Some("w"), vec!["P".to_string(), "Q".to_string()])
    );
    assert_eq!(found[2].whole, ["Note: x"]);
    assert_eq!(&body[found[0].range.clone()], "[[A\\/B|x\\|y]]");
}

#[test]
fn a_link_resolves_by_the_end_of_its_path_in_its_own_wiki_or_another() {
    let doc = notes();
    let o = doc.outline();
    let (notes, other) = (find(o, "@wiki notes"), find(o, "@wiki other"));
    let one = |root: &Position, text: &str| -> Vec<String> {
        let link = links(text).remove(0);
        resolve(o, root, &link)
            .iter()
            .map(|p| p.h(o).to_string())
            .collect()
    };
    assert_eq!(one(&notes, "[[Intro]]"), ["Intro"]);
    assert_eq!(one(&notes, "[[Guide/Setup]]"), ["Setup"]);
    assert_eq!(one(&notes, "[[Nowhere]]"), Vec::<String>::new());
    // `Setup` is in both wikis, but a link names pages of its own.
    assert_eq!(one(&other, "[[Setup]]"), ["Setup"]);
    assert_eq!(one(&other, "[[notes:Intro]]"), ["Intro"]);
    assert_eq!(one(&other, "[[notes:]]"), ["@wiki notes"]);
    // A colon before no wiki's name is part of the headline.
    assert_eq!(one(&notes, "[[Note: x]]"), Vec::<String>::new());
}

#[test]
fn the_rules_are_reported_one_line_each() {
    let doc = outline(&[
        (0, "@wiki a:b", ""),
        (1, "@file x.py", ""),
        (1, "Page", "@language python\nx\n"),
        (1, "@wiki inner", ""),
        (0, "@wiki dup", ""),
        (0, "@wiki dup", ""),
    ]);
    let broken = check(doc.outline());
    assert!(
        broken.contains(&"wiki name `a:b` may not hold `:`, `/` or `\\`".to_string()),
        "{broken:?}"
    );
    assert!(
        broken.contains(&"page `@file x.py` of wiki a:b starts with @".to_string()),
        "{broken:?}"
    );
    assert!(
        broken
            .iter()
            .any(|b| b.starts_with("page `Page` of wiki a:b has the directive @language")),
        "{broken:?}"
    );
    assert!(
        broken
            .iter()
            .any(|b| b.starts_with("@wiki inner is inside @wiki a:b")),
        "{broken:?}"
    );
    assert!(
        broken.contains(&"two wikis are named dup".to_string()),
        "{broken:?}"
    );
    assert!(check(notes().outline()).is_empty());
}

#[test]
fn every_page_is_markdown_whatever_its_parents_say() {
    let doc = outline(&[
        (0, "@file x.py", "@language python\n"),
        (1, "@wiki w", ""),
        (2, "Page", ""),
    ]);
    let o = doc.outline();
    assert_eq!(o.language_at(&find(o, "Page")).as_deref(), Some("md"));
    assert_eq!(o.language_at(&find(o, "@wiki w")).as_deref(), Some("md"));
}

#[test]
fn renaming_a_page_rewrites_every_link_to_it_as_one_undo() {
    let mut doc = notes();
    let setup = find(doc.outline(), "Setup");
    let plan = doc.rename_block(&setup, "Install").unwrap().unwrap();
    assert_eq!(plan.references, 2);
    let o = doc.outline();
    assert_eq!(
        find(o, "Intro").b(o),
        "See [[Install|how to set up]] and [[Guide/Install]].\n"
    );
    // The other wiki's own `Setup` is not this page.
    assert_eq!(
        find(o, "@wiki other").b(o),
        "Back to [[notes:Intro]] and [[notes:]].\n"
    );
    doc.undo();
    let o = doc.outline();
    assert_eq!(
        find(o, "Intro").b(o),
        "See [[Setup|how to set up]] and [[Guide/Setup]].\n"
    );
}

#[test]
fn renaming_a_wiki_rewrites_the_links_naming_it() {
    let mut doc = notes();
    let root = find(doc.outline(), "@wiki notes");
    doc.rename_block(&root, "@wiki docs").unwrap().unwrap();
    let o = doc.outline();
    assert_eq!(
        find(o, "@wiki other").b(o),
        "Back to [[docs:Intro]] and [[docs:]].\n"
    );
    let taken = find(o, "@wiki other");
    assert!(doc.rename_block(&taken, "@wiki docs").is_err());
}

#[test]
fn export_writes_headings_by_depth_and_links_as_anchors() {
    let doc = notes();
    let o = doc.outline();
    let text = export(o, &find(o, "@wiki notes")).unwrap();
    assert_eq!(
        text,
        "Start at [Intro](#intro).\n\n\
         # Intro\n\nSee [how to set up](#setup) and [Setup](#setup).\n\n\
         # Guide\n\n\
         ## Setup\n\nInstall it.\n\n#### Steps\n\nRun it.\n"
    );
    let other = export(o, &find(o, "@wiki other")).unwrap();
    assert!(
        other.starts_with("Back to [Intro](notes.md#intro) and [notes](notes.md).\n"),
        "{other}"
    );
}

#[test]
fn export_refuses_a_broken_link_and_a_page_deeper_than_six() {
    let mut tree = vec![(0, "@wiki deep", "[[Gone]]\n")];
    let names = ["1", "2", "3", "4", "5", "6", "7"];
    for (d, n) in names.iter().enumerate() {
        tree.push((d + 1, n, ""));
    }
    let doc = outline(&tree);
    let o = doc.outline();
    let problems = export(o, &find(o, "@wiki deep")).unwrap_err();
    assert!(
        problems.contains(&"page `@wiki deep`: [[Gone]] names no page".to_string()),
        "{problems:?}"
    );
    assert!(
        problems.contains(&"page `7` is 7 levels deep; markdown has 6".to_string()),
        "{problems:?}"
    );
}

#[test]
fn duplicate_headings_get_numbered_anchors_as_github_gives_them() {
    assert_eq!(slug("Hello, World!"), "hello-world");
    let doc = outline(&[
        (0, "@wiki w", "[[A/X]] [[B/X]]\n"),
        (1, "A", ""),
        (2, "X", ""),
        (1, "B", ""),
        (2, "X", ""),
    ]);
    let o = doc.outline();
    let text = export(o, &find(o, "@wiki w")).unwrap();
    assert!(text.starts_with("[X](#x) [X](#x-1)\n"), "{text}");
}

#[test]
fn links_become_leo_unls_for_an_outline_shared_with_leo() {
    let doc = notes();
    let o = doc.outline();
    let edits = wikilinks_to_unls(o, &find(o, "@wiki notes")).unwrap();
    let intro = find(o, "Intro");
    let gnx = intro.gnx(o);
    let root_edit = &edits
        .iter()
        .find(|(p, _)| p.h(o) == "@wiki notes")
        .unwrap()
        .1;
    assert_eq!(root_edit, &format!("Start at `unl:gnx://#{gnx}`.\n"));
}

#[test]
fn the_rendered_view_shows_each_link_to_one_page_as_a_link_to_its_node() {
    let doc = notes();
    let o = doc.outline();
    let intro = find(o, "Intro");
    let setup = find(o, "Setup").gnx(o).to_string();
    let other = find(o, "@wiki other");
    let body = "[[Setup|a [b\\]]] [[other:]] [[Nowhere]] `[[Setup]]`\n";
    assert_eq!(
        rendered(o, &intro, body).unwrap(),
        format!(
            "[a \\[b\\]](unl:gnx://#{setup}) [other](unl:gnx://#{}) [[Nowhere]] `[[Setup]]`\n",
            other.gnx(o)
        )
    );
    // Outside a wiki, or with no link to change, the body stays as it is.
    assert_eq!(rendered(o, &intro, "No links.\n"), None);
}
