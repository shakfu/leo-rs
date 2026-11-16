//! Real `claude -p` runs: an `:ask`, and an `@agent` run that edits the
//! outline over MCP. Ignored, as each costs money and sends the prompt to
//! Anthropic; run them with
//! `cargo test -p leo-agent --test claude -- --ignored`.
#![cfg(unix)]

use std::time::{Duration, Instant};

use leo_agent::{AgentPlugin, Agents};
use leoapp::app::App;
use leolib::Document;

const AGENT: &str = "@agent agent=edit scope=1 sort the other children of this node's parent \
                     alphabetically by headline, keeping this node first";

#[test]
#[ignore]
fn claude_sorts_the_children_and_one_undo_restores_them() {
    let dir = tempfile::tempdir().unwrap();
    leoapp::plugins::register(vec![&AgentPlugin]);
    let mut doc = Document::new_empty(&dir.path().join("t.leo").to_string_lossy());
    let parent = doc.outline().root_position().unwrap();
    doc.set_headline(&parent, "Parent");
    for h in [AGENT, "Cherry", "Apple", "Banana"] {
        let p = doc.outline_mut_untracked().insert_as_last_child(&parent);
        doc.set_headline(&p, h);
    }
    doc.clear_undo();
    let mut app = App::new(doc);
    let o = app.outline();
    let p = o
        .all_positions()
        .into_iter()
        .find(|p| p.h(o) == AGENT)
        .unwrap();
    app.select(p);
    let log = dir.path().join("claude.log");
    let (table, warnings) = Agents::parse(&format!(
        "edit = claude -p --strict-mcp-config --mcp-config {{mcp}} --allowedTools mcp__leo \
         --debug-file {}\n",
        log.display()
    ));
    assert!(warnings.is_empty(), "{warnings:?}");
    leo_agent::set_agents(&mut app, table);

    app.run_command_line("agent-run");
    assert!(app.answer_key('y'), "{}", app.mini_label());
    let start = Instant::now();
    while leo_agent::busy(&app) {
        assert!(
            start.elapsed() < Duration::from_secs(300),
            "no reply in 5 minutes"
        );
        app.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    let children = |app: &App| {
        let o = app.outline();
        let parent = o.root_position().unwrap();
        parent
            .children(o)
            .iter()
            .map(|c| c.h(o).to_string())
            .collect::<Vec<_>>()
    };
    let o = app.outline();
    let reply = o
        .all_positions()
        .into_iter()
        .find(|p| p.h(o) == AGENT)
        .unwrap();
    let reply = reply.b(o).to_string();
    let connected = std::fs::read_to_string(&log)
        .unwrap_or_default()
        .lines()
        .filter(|l| l.contains("MCP server \"leo\""))
        .map(str::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        children(&app),
        [AGENT, "Apple", "Banana", "Cherry"],
        "message: {}\nreply: {reply}\nlog:\n{connected}",
        app.message
    );
    app.run_command_line("undo");
    assert_eq!(children(&app), [AGENT, "Cherry", "Apple", "Banana"]);
}

#[test]
#[ignore]
fn claude_answers_an_ask_with_no_agents_table() {
    let dir = tempfile::tempdir().unwrap();
    leoapp::plugins::register(vec![&AgentPlugin]);
    let mut doc = Document::new_empty(&dir.path().join("t.leo").to_string_lossy());
    let root = doc.outline().root_position().unwrap();
    doc.set_headline(&root, "Notes");
    doc.set_body(&root, "one\n");
    doc.clear_undo();
    let mut app = App::new(doc);
    app.select(root);
    app.focus = leoapp::app::Focus::Body;
    leo_agent::set_agents(&mut app, Agents::parse("").0);
    app.run_command_line("ask reply with only the word pelican, in lower case");
    assert_eq!(app.message, "ask: claude is running");
    let start = Instant::now();
    while leo_agent::busy(&app) {
        assert!(
            start.elapsed() < Duration::from_secs(300),
            "no reply in 5 minutes"
        );
        app.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    let o = app.outline();
    let b = o.root_position().unwrap().b(o).to_string();
    assert!(
        b.starts_with("one\n") && b.contains("pelican"),
        "message: {}\nbody: {b}",
        app.message
    );
}
