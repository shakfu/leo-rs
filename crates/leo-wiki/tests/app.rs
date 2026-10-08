//! The wiki plugin in leoapp: links followed, `[[` completed, the rules kept
//! and the wiki exported. Its own test binary, as the plugin list is one per
//! process.
#![cfg(feature = "leoapp")]

use leo_wiki::{app::WikiPlugin, Wiki};
use leoapp::app::{App, Focus, Mode};
use leoapp::keys::{KeyCode, KeyEvent, KeyModifiers};
use leolib::{Document, Position};

fn register() {
    leoapp::plugins::register_kinds(leolib::ext::Kinds::empty().with_tree(Wiki).unwrap());
    leoapp::plugins::register(vec![&WikiPlugin]);
}

/// `@wiki w` with pages Intro and Guide, in a document saved as `file`.
fn app(file: &str) -> App {
    register();
    let mut doc = Document::new_empty(file);
    let root = doc.outline().root_position().unwrap();
    doc.set_headline(&root, "@wiki w");
    doc.set_body(&root, "Read [[Intro]] first.\n");
    let intro = doc.outline_mut_untracked().insert_as_last_child(&root);
    doc.set_headline(&intro, "Intro");
    doc.set_body(&intro, "Then [[Guide]].\n");
    let guide = doc.outline_mut_untracked().insert_as_last_child(&root);
    doc.set_headline(&guide, "Guide");
    doc.clear_undo();
    App::new(doc)
}

fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn find(app: &App, h: &str) -> Position {
    let o = app.outline();
    o.all_positions().into_iter().find(|p| p.h(o) == h).unwrap()
}

#[test]
fn an_edit_that_breaks_a_rule_is_undone_and_said() {
    let mut app = app("");
    let guide = find(&app, "Guide");
    app.select(guide.clone());
    // The headline typed over: an `@file` page would write a file.
    app.open_mini(leoapp::minibuffer::MiniKind::Headline, "@file x.py".into());
    key(&mut app, KeyCode::Enter);
    assert_eq!(guide.h(app.outline()), "Guide");
    assert_eq!(
        app.message,
        "refused: page `@file x.py` of wiki w starts with @"
    );
    // A body edit adding a directive, made any way, is refused the same.
    app.doc.set_body(&guide, "@language python\n");
    app.check_rules(true);
    assert_eq!(guide.b(app.outline()), "");
    assert!(
        app.message.contains("has the directive @language"),
        "{}",
        app.message
    );
    // An edit that keeps the rules stands.
    app.doc.set_body(&guide, "Fine.\n");
    app.check_rules(true);
    assert_eq!(guide.b(app.outline()), "Fine.\n");
}

#[test]
fn a_link_under_the_cursor_is_followed_and_ctrl_o_comes_back() {
    let mut app = app("");
    let root = find(&app, "@wiki w");
    app.select(root.clone());
    app.focus = Focus::Body;
    app.editor.cursor = (0, 8);
    app.run("open-url-under-cursor", 1);
    assert_eq!(app.current.h(app.outline()), "Intro", "{}", app.message);
    app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
    assert_eq!(app.current.h(app.outline()), "@wiki w");
}

#[test]
fn double_bracket_offers_the_wikis_pages() {
    let mut app = app("");
    let intro = find(&app, "Intro");
    app.select(intro);
    app.focus = Focus::Body;
    app.editor.cursor = (0, 0);
    key(&mut app, KeyCode::Char('A'));
    assert_eq!(app.mode, Mode::Insert);
    key(&mut app, KeyCode::Char('['));
    key(&mut app, KeyCode::Char('['));
    key(&mut app, KeyCode::Char('G'));
    let menu = app.completion.as_ref().expect("a menu after [[");
    let shown: Vec<&str> = (0..menu.shown.len())
        .filter_map(|i| menu.shown_item(i))
        .map(|c| c.label.as_str())
        .collect();
    assert_eq!(shown, ["Guide"]);
    app.accept_completion(0);
    assert_eq!(app.body_buffer()[0], "Then [[Guide]].[[Guide");
}

#[test]
fn export_wiki_writes_the_file_and_the_rules_are_known_at_open() {
    let dir = tempfile::tempdir().unwrap();
    let leo = dir.path().join("o.leo").to_string_lossy().to_string();
    let mut app = app(&leo);
    let intro = find(&app, "Intro");
    app.select(intro);
    app.run("export-wiki", 1);
    let text = std::fs::read_to_string(dir.path().join("w.md")).unwrap();
    assert_eq!(
        text,
        "Read [Intro](#intro) first.\n\n# Intro\n\nThen [Guide](#guide).\n\n# Guide\n\n"
    );
    assert!(app.message.starts_with("exported "), "{}", app.message);
    assert!(app.broken_rules().is_empty());
}
