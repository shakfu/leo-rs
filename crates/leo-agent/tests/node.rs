//! `:agent-run` on `@agent` headline nodes, with shell commands standing in
//! for the agent. Its own test binary, as the plugin list is one per process.
#![cfg(unix)]

use std::time::{Duration, Instant};

use leo_agent::{AgentPlugin, Agents};
use leoapp::app::{App, Mode};
use leoapp::keys::{KeyCode, KeyEvent, KeyModifiers};
use leolib::{Document, Position};

/// `Parent` > `HEADLINE` with `body`, and `Sibling`; the agent node selected.
fn app(headline: &str, body: &str, agents: &str) -> App {
    leoapp::plugins::register(vec![&AgentPlugin]);
    let dir = std::env::temp_dir().join(format!("leo-agent-node-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut doc = Document::new_empty(&dir.join("t.leo").to_string_lossy());
    let root = doc.outline().root_position().unwrap();
    doc.set_headline(&root, "Parent");
    doc.set_body(&root, "parent text\n");
    let node = doc.outline_mut_untracked().insert_as_last_child(&root);
    doc.set_headline(&node, headline);
    doc.set_body(&node, body);
    let sibling = doc.outline_mut_untracked().insert_as_last_child(&root);
    doc.set_headline(&sibling, "Sibling");
    doc.set_body(&sibling, "sibling text\n");
    doc.clear_undo();
    let mut app = App::new(doc);
    let p = find(&app, headline);
    app.select(p);
    let (table, warnings) = Agents::parse(agents);
    assert!(warnings.is_empty(), "{warnings:?}");
    leo_agent::set_agents(&mut app, table);
    app
}

/// An executable shell script running `body`.
fn script(name: &str, body: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("leo-agent-node-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path.to_string_lossy().to_string()
}

fn find(app: &App, h: &str) -> Position {
    let o = app.outline();
    o.all_positions()
        .into_iter()
        .find(|p| p.h(o) == h)
        .unwrap_or_else(|| panic!("no node {h}"))
}

fn body(app: &App, h: &str) -> String {
    find(app, h).b(app.outline()).to_string()
}

fn key(app: &mut App, c: char) {
    app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
}

/// Poll until the run is over and its reply written.
fn finish(app: &mut App) {
    let start = Instant::now();
    while leo_agent::busy(app) {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the run never finished"
        );
        app.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Run the selected node, answering y if it asks.
fn run(app: &mut App) {
    app.run_command_line("agent-run");
    if app.mode == Mode::Confirm {
        key(app, 'y');
    }
    finish(app);
}

/// `body` with each separator's time replaced by `TIME`.
fn untimed(body: &str) -> String {
    body.lines()
        .map(|l| match l.strip_prefix("-- ") {
            Some(rest) if rest.len() > 18 => format!("-- TIME{}", &rest[17..]),
            _ => l.to_string(),
        })
        .map(|l| l + "\n")
        .collect()
}

const ECHO: &str = "default = echo\necho = cat\n";

#[test]
fn the_first_run_asks_then_replies_append_with_a_separator() {
    let mut app = app("@agent hi", "", ECHO);
    app.run_command_line("agent-run");
    assert_eq!(app.mode, Mode::Confirm);
    assert_eq!(app.mini_label(), "agent: send 1 KB to echo? (y/n) ");
    key(&mut app, 'y');
    finish(&mut app);
    assert_eq!(untimed(&body(&app, "@agent hi")), "-- TIME echo --\nhi\n");
    assert_eq!(app.message, "agent: the reply is appended to `@agent hi`");

    // The outline is trusted now: a prompt-only run does not ask again.
    app.run_command_line("agent-run");
    assert_ne!(app.mode, Mode::Confirm);
    finish(&mut app);
    assert_eq!(
        untimed(&body(&app, "@agent hi")),
        "-- TIME echo --\nhi\n\n-- TIME echo --\nhi\n"
    );
    // One undo step takes out one reply.
    app.doc.undo();
    assert_eq!(untimed(&body(&app, "@agent hi")), "-- TIME echo --\nhi\n");
}

#[test]
fn a_separator_carries_the_utc_minute() {
    let mut app = app("@agent hi", "", ECHO);
    run(&mut app);
    let b = body(&app, "@agent hi");
    let line = b.lines().next().unwrap();
    // `-- 2026-11-16 14:03Z echo --`
    let time = &line[3..20];
    assert!(time.ends_with('Z') && time.as_bytes()[10] == b' ', "{line}");
}

#[test]
fn outline_text_always_asks_and_no_runs_nothing() {
    let mut app = app("@agent scope=1 hi", "", ECHO);
    run(&mut app);
    app.run_command_line("agent-run");
    assert_eq!(app.mode, Mode::Confirm);
    assert!(
        app.mini_label().contains("with 3 nodes of this outline"),
        "{}",
        app.mini_label()
    );
    key(&mut app, 'n');
    assert_eq!(app.message, "not run");
    assert!(!leo_agent::busy(&app));
}

#[test]
fn the_nodes_own_replies_are_not_sent() {
    let mut app = app("@agent scope=1 reply=child hi", "an earlier reply\n", ECHO);
    run(&mut app);
    let parent = find(&app, "@agent scope=1 reply=child hi");
    let o = app.outline();
    let child = parent.children(o).last().unwrap().clone();
    let sent = child.b(o).to_string();
    assert!(sent.contains("\n* Parent\n  parent text\n"), "{sent}");
    assert!(
        sent.contains("\n  * @agent scope=1 reply=child hi\n  * Sibling\n"),
        "{sent}"
    );
    assert!(!sent.contains("an earlier reply"), "{sent}");
}

#[test]
fn reply_child_adds_a_headlined_child_as_one_undo_step() {
    let mut app = app(
        "@agent reply=child list the top five sort algorithms, with costs and space",
        "",
        ECHO,
    );
    run(&mut app);
    let p = find(
        &app,
        "@agent reply=child list the top five sort algorithms, with costs and space",
    );
    assert_eq!(p.b(app.outline()), "");
    let o = app.outline();
    let children = p.children(o);
    assert_eq!(children.len(), 1);
    let h = children[0].h(o);
    assert!(
        h.ends_with("Z list the top five sort algorithms, with costs and space"),
        "{h}"
    );
    assert!(!h.starts_with('@'));
    assert!(untimed(children[0].b(o)).starts_with("-- TIME echo --\nlist the top five"));
    app.doc.undo();
    let p = find(
        &app,
        "@agent reply=child list the top five sort algorithms, with costs and space",
    );
    assert!(p.children(app.outline()).is_empty());
}

#[test]
fn reply_replace_overwrites_its_own_reply_but_not_an_edit() {
    let mut app = app("@agent reply=replace hi", "notes\n", ECHO);
    run(&mut app);
    assert_eq!(
        untimed(&body(&app, "@agent reply=replace hi")),
        "-- TIME echo --\nhi\n"
    );
    run(&mut app);
    assert_eq!(
        untimed(&body(&app, "@agent reply=replace hi")),
        "-- TIME echo --\nhi\n"
    );
    let p = find(&app, "@agent reply=replace hi");
    app.doc.set_body(&p, "my edit\n");
    app.run_command_line("agent-run");
    assert!(
        app.message.contains("body changed since the last reply"),
        "{}",
        app.message
    );
    assert_eq!(body(&app, "@agent reply=replace hi"), "my edit\n");
}

#[test]
fn a_body_edited_during_a_replace_run_gets_the_reply_in_the_register() {
    let slow = script("slow", "sleep 0.3; cat");
    let mut app = app(
        "@agent reply=replace hi",
        "",
        &format!("default = slow\nslow = {slow}\n"),
    );
    app.run_command_line("agent-run");
    key(&mut app, 'y');
    let p = find(&app, "@agent reply=replace hi");
    app.doc.set_body(&p, "typed meanwhile\n");
    finish(&mut app);
    assert_eq!(body(&app, "@agent reply=replace hi"), "typed meanwhile\n");
    assert_eq!(app.editor.register.text, "hi\n");
    assert!(
        app.message.contains("`p` puts the reply"),
        "{}",
        app.message
    );
}

#[test]
fn a_node_deleted_during_the_run_gets_the_reply_in_the_register() {
    let slow = script("slow-del", "sleep 0.3; cat");
    let mut app = app("@agent hi", "", &format!("default = slow\nslow = {slow}\n"));
    app.run_command_line("agent-run");
    key(&mut app, 'y');
    let p = find(&app, "@agent hi");
    app.doc.delete_node(&p);
    finish(&mut app);
    assert_eq!(app.editor.register.text, "hi\n");
    assert!(app.message.contains("was deleted"), "{}", app.message);
}

#[test]
fn a_node_that_is_not_an_agent_or_a_bad_headline_refuses() {
    let mut app = app("Top 5", "", ECHO);
    app.run_command_line("agent-run");
    assert!(
        app.message.contains("is not an @agent node"),
        "{}",
        app.message
    );

    let mut app = self::app("@agent reply=insert hi", "", ECHO);
    app.run_command_line("agent-run");
    assert!(
        app.message.contains("reply is append, replace or child"),
        "{}",
        app.message
    );
    let mut app = self::app("@agent scope=1", "", ECHO);
    app.run_command_line("agent-run");
    assert!(app.message.contains("no prompt"), "{}", app.message);
}

#[test]
fn activating_an_agent_node_runs_it_and_others_fall_through() {
    let mut app = app("@agent hi", "", ECHO);
    let p = find(&app, "@agent hi");
    assert!(leoapp::plugins::activate(&mut app, &p));
    assert_eq!(app.mode, Mode::Confirm);
    key(&mut app, 'y');
    finish(&mut app);
    assert_eq!(untimed(&body(&app, "@agent hi")), "-- TIME echo --\nhi\n");

    let sibling = find(&app, "Sibling");
    app.select(sibling.clone());
    assert!(!leoapp::plugins::activate(&mut app, &sibling));
    assert!(!leo_agent::busy(&app));
}

#[test]
fn a_block_keeps_its_prose_and_prompt_and_appends_or_replaces_after_the_end() {
    let block = "Notes.\n@agent\nsay hi\n@agent-end\n";
    let mut app = app("Sorting", block, ECHO);
    run(&mut app);
    assert_eq!(
        untimed(&body(&app, "Sorting")),
        format!("{block}\n-- TIME echo --\nsay hi\n")
    );

    let block = "Notes.\n@agent reply=replace\nsay hi\n@agent-end\n";
    let mut app = self::app("Sorting", &format!("{block}stale reply\n"), ECHO);
    run(&mut app);
    run(&mut app);
    assert_eq!(
        untimed(&body(&app, "Sorting")),
        format!("{block}-- TIME echo --\nsay hi\n")
    );
}

#[test]
fn the_line_form_takes_the_headline_as_the_prompt() {
    let mut app = app(
        "Top 5 sort algorithms",
        "@agent prompt=headline reply=child\n",
        ECHO,
    );
    run(&mut app);
    let p = find(&app, "Top 5 sort algorithms");
    let o = app.outline();
    let child = &p.children(o)[0];
    assert!(
        child.h(o).ends_with("Z Top 5 sort algorithms"),
        "{}",
        child.h(o)
    );
    assert!(
        child.b(o).ends_with(" echo --\nTop 5 sort algorithms\n"),
        "{}",
        child.b(o)
    );
}

#[test]
fn enter_on_the_agent_line_runs_it_and_the_line_is_marked() {
    let mut app = app("Sorting", "Notes.\n@agent\nsay hi\n@agent-end\n", ECHO);
    let p = find(&app, "Sorting");
    let lines: Vec<String> = body(&app, "Sorting").lines().map(str::to_string).collect();
    let marked = leoapp::plugins::links(app.outline(), &p, &lines);
    assert!(marked.contains(&(1, 0..6)), "{marked:?}");

    app.focus = leoapp::app::Focus::Body;
    app.editor.cursor = (0, 0);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(!leo_agent::busy(&app) && app.mode != Mode::Confirm);

    app.editor.cursor = (1, 0);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.mode, Mode::Confirm);
    key(&mut app, 'y');
    finish(&mut app);
    assert!(body(&app, "Sorting").ends_with(" echo --\nsay hi\n"));
}

#[test]
fn a_body_forms_own_body_is_not_sent() {
    let mut app = app(
        "Sorting",
        "Private notes.\n@agent scope=1 reply=child\nsummarise\n@agent-end\n",
        ECHO,
    );
    run(&mut app);
    let p = find(&app, "Sorting");
    let o = app.outline();
    let sent = p.children(o)[0].b(o).to_string();
    assert!(sent.contains("\n  * Sorting\n  * Sibling\n"), "{sent}");
    assert!(!sent.contains("Private notes"), "{sent}");
}
