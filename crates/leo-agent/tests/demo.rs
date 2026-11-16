//! `demo/agent`, run with its own `agents.toml` and stand-in agent, so the
//! exercises its outline describes hold. Its own test binary, as the plugin
//! list is one per process.
#![cfg(unix)]

use std::path::Path;
use std::time::{Duration, Instant};

use leo_agent::{AgentPlugin, Agents};
use leoapp::app::{App, Focus};
use leolib::{Document, Position};

/// `demo/agent` copied to a temporary directory, opened, with the demo's
/// table. None outside the workspace: the published crate has no `demo/`.
fn demo() -> Option<(tempfile::TempDir, App)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo/agent");
    if !src.is_dir() {
        return None;
    }
    let dir = tempfile::tempdir().unwrap();
    for name in [
        "demo.leo",
        "agents.toml",
        "echo-agent.sh",
        "outline-agent.sh",
    ] {
        std::fs::copy(src.join(name), dir.path().join(name)).unwrap();
    }
    leoapp::plugins::register(vec![&AgentPlugin]);
    let leo = dir.path().join("demo.leo");
    let doc =
        Document::open_with(&leo.to_string_lossy(), true, leolib::ext::Kinds::empty()).unwrap();
    let mut app = App::new(doc);
    let text = std::fs::read_to_string(dir.path().join("agents.toml")).unwrap();
    let (table, warnings) = Agents::parse(&text);
    assert!(warnings.is_empty(), "{warnings:?}");
    leo_agent::set_agents(&mut app, table);
    Some((dir, app))
}

fn find(app: &App, h: &str) -> Position {
    let o = app.outline();
    o.all_positions().into_iter().find(|p| p.h(o) == h).unwrap()
}

/// Select `h`, put the body cursor on `row`, select through line `to` if
/// given, and run `command` to the end.
fn ask_from(app: &mut App, h: &str, row: usize, to: Option<usize>, command: &str) {
    let p = find(app, h);
    app.select(p);
    app.focus = Focus::Body;
    app.editor.cursor = (row, 0);
    app.editor.visual = to.map(|to| ((to, 0), leoapp::editor::motion::Kind::Linewise));
    app.run_command_line(command);
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

fn ask(app: &mut App, h: &str, row: usize, command: &str) {
    ask_from(app, h, row, None, command)
}

/// Select `h` and `:agent-run` it, answering y if it asks.
fn agent_run(app: &mut App, h: &str) {
    let p = find(app, h);
    app.select(p);
    app.run_command_line("agent-run");
    if app.mode == leoapp::app::Mode::Confirm {
        assert!(app.answer_key('y'));
    }
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

fn body(app: &App, h: &str) -> String {
    find(app, h).b(app.outline()).to_string()
}

#[test]
fn the_exercises_hold() {
    let Some((_dir, mut app)) = demo() else {
        return;
    };
    // Saved as leo-rs writes it, so a save in the demo changes only what
    // the exercises add.
    let leo = app.outline().file_name.clone();
    let original = std::fs::read_to_string(&leo).unwrap();
    let mut o = leolib::open_outline(&leo, false).unwrap();
    assert!(
        leolib::to_xml(&mut o) == original,
        "demo.leo is not as leo-rs writes it"
    );
    // 1: below the cursor's line.
    let h = "1. A reply below the cursor";
    ask(&mut app, h, 0, "ask agent=echo list three sort algorithms");
    let b = body(&app, h);
    assert!(
        b.starts_with("Put the cursor on this line, then type:\necho-agent (model: none) got 1 lines:\n> list three sort algorithms\n"),
        "{b}"
    );

    // 2: an empty body becomes the reply.
    let h = "2. An empty body";
    ask(&mut app, h, 0, "ask agent=echo hello");
    assert_eq!(
        body(&app, h),
        "echo-agent (model: none) got 1 lines:\n> hello\n"
    );

    // 3: scope=1 from a child sends its parent's subtree; scope=2 is past
    // the top and refuses.
    ask(&mut app, "Quicksort", 0, "ask agent=echo scope=2 compare");
    assert!(app.message.contains("past the top"), "{}", app.message);
    ask(&mut app, "Quicksort", 0, "ask agent=echo scope=1 compare");
    let b = body(&app, "Quicksort");
    for line in [
        "> * 3. Scope: outline text with the prompt",
        ">   * Quicksort",
        ">     Always n log n. In place. Not stable.",
    ] {
        assert!(b.contains(&format!("\n{line}\n")), "{line} in {b}");
    }

    // 4: a listed model reaches the agent; others refuse.
    let h = "4. Models";
    ask(&mut app, h, 0, "ask agent=echo model=small hi");
    assert!(body(&app, h).contains("\necho-agent (model: small) got 1 lines:\n"));
    ask(&mut app, h, 0, "ask agent=echo model=huge hi");
    assert!(
        app.message.contains("not in echo's models"),
        "{}",
        app.message
    );

    // 8: failures say why.
    let h = "8. Failures and refusals";
    let before = body(&app, h);
    ask(&mut app, h, 0, "ask agent=echo provider=broken hi");
    assert_eq!(
        app.message,
        "ask: echo broken failed (exit 3): echo-agent: asked to fail"
    );
    if std::env::var_os("LEO_DEMO_TOKEN").is_none() {
        ask(&mut app, h, 0, "ask agent=echo provider=keyed hi");
        assert!(
            app.message.contains("`LEO_DEMO_TOKEN` is not set"),
            "{}",
            app.message
        );
    }
    assert_eq!(body(&app, h), before);

    // 11: the selected lines go with the prompt; replace puts the reply
    // in their place.
    let h = "11. Selected lines";
    let apples = body(&app, h).lines().position(|l| l == "apples").unwrap();
    let command = "ask agent=echo number these";
    ask_from(&mut app, h, apples, Some(apples + 2), command);
    let b = body(&app, h);
    let quoted = "\n> apples\n> pears\n> plums\n";
    assert!(
        b.contains("plums\necho-agent (model: none) got 7 lines:\n"),
        "{b}"
    );
    assert!(b.contains(quoted), "{b}");
    app.doc.undo();
    let command = "ask agent=echo reply=replace number these";
    ask_from(&mut app, h, apples, Some(apples + 2), command);
    let b = body(&app, h);
    assert!(!b.contains("\napples\n") && b.contains(quoted), "{b}");

    // 12: an @agent node appends; a child-reply one sends its section but
    // not its own children.
    let h = "@agent agent=echo list three sort algorithms";
    agent_run(&mut app, h);
    let b = body(&app, h);
    assert!(
        b.starts_with("-- ")
            && b.contains(" echo --\necho-agent")
            && b.ends_with("> list three sort algorithms\n"),
        "{b}"
    );
    let h = "@agent agent=echo scope=1 reply=child compare the notes in this section";
    agent_run(&mut app, h);
    let p = find(&app, h);
    let o = app.outline();
    let reply = p.children(o)[0].b(o).to_string();
    assert!(reply.contains("> * 12. @agent nodes\n"), "{reply}");
    assert!(
        reply.contains(">     -- "),
        "the first child's reply is sent: {reply}"
    );

    // 13: a block replaces only its replies; a line takes the headline.
    let h = "Sorting notes";
    agent_run(&mut app, h);
    agent_run(&mut app, h);
    let b = body(&app, h);
    assert!(
        b.starts_with(
            "Merge sort is stable; heapsort is not.\n\n@agent agent=echo reply=replace\n"
        ),
        "{b}"
    );
    assert_eq!(b.matches(" echo --\n").count(), 1, "{b}");
    assert!(
        b.contains("> Say which of these sorts are stable,\n> in one line each.\n"),
        "{b}"
    );
    let h = "Top 5 sort algorithms";
    agent_run(&mut app, h);
    assert!(
        body(&app, h).ends_with("> Top 5 sort algorithms\n"),
        "{}",
        body(&app, h)
    );

    // 14: the outliner adds children over MCP, but not beside its node
    // unless the scope reaches there. It needs curl.
    let has_curl = std::process::Command::new("curl")
        .arg("--version")
        .output()
        .is_ok();
    if has_curl {
        let h = "@agent agent=outliner plan write test";
        agent_run(&mut app, h);
        let p = find(&app, h);
        let o = app.outline();
        let heads: Vec<&str> = p.children(o).iter().map(|c| c.h(o)).collect();
        assert_eq!(heads, ["plan", "write", "test"], "{}", app.message);
        let b = body(&app, h);
        assert!(
            b.ends_with("outline-agent added children: plan write test\nbeside its node: refused: it would put a node outside this client's subtree\n"),
            "{b}"
        );
        app.doc.undo();
        assert!(find(&app, h).children(app.outline()).is_empty());
        assert_eq!(body(&app, h), "");
        let p = find(&app, h);
        let scoped = "@agent agent=outliner scope=1 plan write test";
        app.doc.set_headline(&p, scoped);
        agent_run(&mut app, scoped);
        assert!(
            body(&app, scoped).ends_with("beside its node: added\n"),
            "{}",
            body(&app, scoped)
        );
        find(&app, "Added beside its node");
    }

    // 9: `--` ends the parameters.
    let h = "9. A prompt that looks like a parameter";
    ask(&mut app, h, 0, "ask agent=echo -- scope=all is a phrase");
    assert!(body(&app, h).contains("\n> scope=all is a phrase\n"));
}
