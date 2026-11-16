//! `:ask` in leoapp, with shell commands standing in for the agent. Its own
//! test binary, as the plugin list is one per process.
#![cfg(unix)]

use std::time::{Duration, Instant};

use leo_agent::{AgentPlugin, Agents};
use leoapp::app::{App, Focus, Mode};
use leolib::{Document, Position};

/// An outline `Parent` > `Here`, `Sibling`, with the cursor on line 1 of
/// `Here`'s body, and `agents` as the agent table.
fn app(agents: &str) -> App {
    leoapp::plugins::register(vec![&AgentPlugin]);
    let dir = std::env::temp_dir().join(format!("leo-agent-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut doc = Document::new_empty(&dir.join("t.leo").to_string_lossy());
    let root = doc.outline().root_position().unwrap();
    doc.set_headline(&root, "Parent");
    doc.set_body(&root, "parent text\n");
    let here = doc.outline_mut_untracked().insert_as_last_child(&root);
    doc.set_headline(&here, "Here");
    doc.set_body(&here, "one\ntwo\nthree\n");
    let sibling = doc.outline_mut_untracked().insert_as_last_child(&root);
    doc.set_headline(&sibling, "Sibling");
    doc.clear_undo();
    let mut app = App::new(doc);
    let here = find(&app, "Here");
    app.select(here);
    app.focus = Focus::Body;
    app.editor.cursor = (0, 0);
    let (table, warnings) = Agents::parse(agents);
    assert!(warnings.is_empty(), "{warnings:?}");
    leo_agent::set_agents(&mut app, table);
    app
}

/// An executable shell script running `body`, for an agent's command: the
/// table splits commands on blanks, so `sh -c "..."` would not survive.
fn script(name: &str, body: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("leo-agent-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path.to_string_lossy().to_string()
}

fn find(app: &App, h: &str) -> Position {
    let o = app.outline();
    o.all_positions().into_iter().find(|p| p.h(o) == h).unwrap()
}

fn body(app: &App, h: &str) -> String {
    find(app, h).b(app.outline()).to_string()
}

/// Poll until the run is over and its reply placed.
fn finish(app: &mut App) {
    let start = Instant::now();
    while leo_agent::busy(app) {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the run never finished"
        );
        assert!(app.poll_after().is_some(), "a front end would not wake");
        app.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn the_reply_goes_below_the_cursors_line_as_one_undo_step() {
    let mut app = app("default = echo\necho = cat\n");
    app.run_command_line("ask list sort algorithms");
    assert_eq!(app.message, "ask: echo is running");
    finish(&mut app);
    assert_eq!(
        body(&app, "Here"),
        "one\nlist sort algorithms\ntwo\nthree\n"
    );
    assert_eq!(app.message, "ask: 1 line inserted");
    assert_eq!(app.editor.cursor, (1, 0));
    app.doc.undo();
    assert_eq!(body(&app, "Here"), "one\ntwo\nthree\n");
}

#[test]
fn scope_sends_the_ancestors_subtree() {
    let mut app = app("echo = cat\n");
    app.editor.cursor = (2, 0);
    app.run_command_line("ask agent=echo scope=1 summarise");
    finish(&mut app);
    let b = body(&app, "Here");
    assert!(
        b.starts_with("one\ntwo\nthree\nsummarise\n\nContext"),
        "{b}"
    );
    for line in [
        "* Parent",
        "  parent text",
        "  * Here",
        "    two",
        "  * Sibling",
    ] {
        assert!(b.contains(&format!("\n{line}\n")), "{line} in {b}");
    }
    let sent = app
        .messages
        .iter()
        .find(|m| m.starts_with("ask: sent "))
        .unwrap();
    assert!(
        sent.starts_with("ask: sent 3 nodes, ") && sent.ends_with(" bytes, to echo"),
        "{sent}"
    );
}

#[test]
fn an_empty_body_becomes_the_reply() {
    let mut app = app("default = echo\necho = cat\n");
    let sibling = find(&app, "Sibling");
    app.select(sibling);
    app.focus = Focus::Body;
    app.run_command_line("ask hello");
    finish(&mut app);
    assert_eq!(body(&app, "Sibling"), "hello\n");
}

#[test]
fn a_reply_waits_for_normal_mode() {
    let mut app = app("default = echo\necho = cat\n");
    app.run_command_line("ask hi");
    app.mode = Mode::Insert;
    let start = Instant::now();
    while !app.message.contains("replied") {
        assert!(start.elapsed() < Duration::from_secs(10));
        app.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    app.poll();
    assert_eq!(body(&app, "Here"), "one\ntwo\nthree\n");
    app.mode = Mode::Normal;
    finish(&mut app);
    assert_eq!(body(&app, "Here"), "one\nhi\ntwo\nthree\n");
}

#[test]
fn a_body_edited_during_the_run_gets_the_reply_in_the_register() {
    let slow = script("slow-cat", "sleep 0.2; cat");
    let mut app = app(&format!("default = slow\nslow = {slow}\n"));
    app.run_command_line("ask hi");
    let here = find(&app, "Here");
    app.doc.set_body(&here, "edited\n");
    finish(&mut app);
    assert_eq!(body(&app, "Here"), "edited\n");
    assert_eq!(app.editor.register.text, "hi\n");
    assert!(app.editor.register.linewise);
    assert!(
        app.message.contains("`p` puts the reply"),
        "{}",
        app.message
    );
}

#[test]
fn models_and_providers_reach_the_command() {
    let args = script("args", "echo \"$1 $FOO\"");
    let mut app = app(&format!(
        "default = args\nargs = {args}\nargs.model = {{model}}\nargs.p.env = FOO=bar\n"
    ));
    app.run_command_line("ask provider=p model=m1 x");
    assert_eq!(app.message, "ask: args p m1 is running");
    finish(&mut app);
    assert_eq!(body(&app, "Here"), "one\nm1 bar\ntwo\nthree\n");
}

#[test]
fn failures_are_said_and_nothing_is_inserted() {
    let fail = script("fail", "echo broken >&2; exit 3");
    let mut app = app(&format!(
        "default = fail\nfail = {fail}\nnone = no-such-agent-command\n"
    ));
    app.run_command_line("ask x");
    finish(&mut app);
    assert_eq!(app.message, "ask: fail failed (exit 3): broken");
    assert_eq!(body(&app, "Here"), "one\ntwo\nthree\n");

    app.run_command_line("ask agent=none x");
    assert!(
        app.message
            .contains("no `no-such-agent-command` command found"),
        "{}",
        app.message
    );
    app.run_command_line("ask agent=gpt x");
    assert!(
        app.message.starts_with("ask: no agent `gpt`"),
        "{}",
        app.message
    );
    app.run_command_line("ask model=--dangerously-skip-permissions x");
    assert!(app.message.contains("not start with -"), "{}", app.message);
}

#[test]
fn a_run_can_be_cancelled_and_one_runs_at_a_time() {
    let mut app = app("default = slow\nslow = sleep 5\n");
    app.run_command_line("ask x");
    app.run_command_line("ask y");
    assert_eq!(app.message, "ask: a run is going; :agent-cancel stops it");
    app.run_command_line("agent-cancel");
    assert_eq!(app.message, "ask: slow stopped");
    assert!(!leo_agent::busy(&app));
}

#[test]
fn a_run_past_the_timeout_is_stopped() {
    let mut app = app("default = slow\nslow = sleep 5\ntimeout = 1\n");
    app.run_command_line("ask x");
    finish(&mut app);
    assert!(app.message.contains("took over 1 s"), "{}", app.message);
}

#[test]
fn the_outline_pane_and_a_missing_prompt_refuse() {
    let mut app = app("echo = cat\n");
    app.run_command_line("ask");
    assert!(app.message.starts_with("usage: :ask"), "{}", app.message);
    app.focus = Focus::Tree;
    app.run_command_line("ask hi");
    assert!(
        app.message.contains("move to the body first"),
        "{}",
        app.message
    );
}

#[test]
fn agents_toml_is_read_again_when_it_changes() {
    let mut app = app("");
    let dir = std::env::temp_dir().join(format!("leo-agent-reread-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("agents.toml");
    std::fs::write(&file, "one = cat\n").unwrap();
    leo_agent::use_agents_file(&mut app, file.clone());
    app.run_command_line("ask agent=one hi");
    finish(&mut app);
    assert_eq!(body(&app, "Here"), "one\nhi\ntwo\nthree\n");

    // A longer file, so the change shows even where times are coarse.
    std::fs::write(&file, "one = cat\ntwo = cat\n").unwrap();
    app.run_command_line("ask agent=two again");
    finish(&mut app);
    assert!(body(&app, "Here").contains("\nagain\n"));
    let again = format!("ask: read {} again", file.display());
    assert!(app.messages.contains(&again), "{:?}", app.messages);

    std::fs::remove_file(&file).unwrap();
    app.run_command_line("ask agent=two x");
    assert!(app.message.contains("does not exist)"), "{}", app.message);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Type `keys` as the user would, then Enter if `enter`.
fn typed(app: &mut App, keys: &str) {
    use leoapp::keys::{KeyCode, KeyEvent, KeyModifiers};
    for c in keys.chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
}

#[test]
fn from_visual_the_lines_go_with_the_prompt_and_the_reply_below_them() {
    let mut app = app("default = echo\necho = cat\n");
    // V selects line 1, j extends to line 2, `:` opens the command line.
    typed(&mut app, "Vj:ask agent=echo sum these");
    finish(&mut app);
    assert_eq!(
        body(&app, "Here"),
        "one\ntwo\nsum these\n\nThe text, lines 1 to 2 of the body:\n\none\ntwo\nthree\n"
    );
    assert_eq!(app.message, "ask: 6 lines inserted");
    assert_eq!(app.editor.cursor, (2, 0));
    assert!(app.editor.visual.is_none());
}

#[test]
fn reply_replace_puts_the_reply_in_place_of_the_selected_lines() {
    let upper = script("upper", "tail -n 2 | tr a-z A-Z");
    let mut app = app(&format!("default = up\nup = {upper}\n"));
    app.editor.cursor = (1, 0);
    typed(&mut app, "Vj:ask reply=replace upper case these");
    finish(&mut app);
    assert_eq!(body(&app, "Here"), "one\nTWO\nTHREE\n");
    assert_eq!(app.message, "ask: 2 lines replaced by 2 lines");
    assert_eq!(app.editor.cursor, (1, 0));
    app.doc.undo();
    assert_eq!(body(&app, "Here"), "one\ntwo\nthree\n");
}

#[test]
fn reply_replace_needs_a_selection() {
    let mut app = app("default = echo\necho = cat\n");
    app.run_command_line("ask reply=replace x");
    assert!(
        app.message.contains("select them in VISUAL first"),
        "{}",
        app.message
    );
    assert!(!leo_agent::busy(&app));
}

#[test]
fn lines_all_sends_the_whole_body_and_replace_replaces_it() {
    let mut app = app("default = echo\necho = cat\n");
    app.editor.cursor = (1, 0);
    app.run_command_line("ask lines=all count");
    finish(&mut app);
    assert_eq!(
        body(&app, "Here"),
        "one\ntwo\nthree\ncount\n\nThe text, lines 1 to 3 of the body:\n\none\ntwo\nthree\n"
    );
    app.doc.undo();

    let upper = script("upper-all", "tail -n 3 | tr a-z A-Z");
    let mut app = self::app(&format!("default = up\nup = {upper}\n"));
    // A selection of line 1 alone: lines=all wins.
    app.editor.visual = Some(((0, 0), leoapp::editor::motion::Kind::Linewise));
    app.run_command_line("ask lines=all reply=replace upper case");
    finish(&mut app);
    assert_eq!(body(&app, "Here"), "ONE\nTWO\nTHREE\n");
    assert_eq!(app.message, "ask: 3 lines replaced by 3 lines");
    app.doc.undo();
    assert_eq!(body(&app, "Here"), "one\ntwo\nthree\n");
}
