//! An agent whose command names `{mcp}` edits the outline over the run's
//! own MCP server. A shell script stands in for the agent; this test plays
//! its MCP client. Its own test binary, as the plugin list is one per
//! process.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use leo_agent::{AgentPlugin, Agents};
use leoapp::app::{App, Mode};
use leolib::{Document, Position};

/// A directory of its own for each test, as tests run at once.
fn dir(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("leo-agent-mcp-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `Parent` > [`AGENT`, `Sibling`], then `Outside`; the agent node selected. The agent writes its config file's path, a copy of it, and its
/// prompt into `dir`, waits for `dir/done`, and replies `finished`.
const AGENT: &str = "@agent agent=edit scope=1 tidy the sibling";

fn app(dir: &Path) -> App {
    app_with(dir, AGENT)
}

/// As [`app`], with `headline` for the agent node.
fn app_with(dir: &Path, headline: &str) -> App {
    use std::os::unix::fs::PermissionsExt;
    leoapp::plugins::register(vec![&AgentPlugin]);
    let d = dir.display();
    let agent = dir.join("agent.sh");
    std::fs::write(
        &agent,
        format!(
            "#!/bin/sh\necho \"$1\" > {d}/path.txt\ncat > {d}/prompt.txt\n\
             cp \"$1\" {d}/c.tmp && mv {d}/c.tmp {d}/config.json\n\
             while [ ! -f {d}/done ]; do sleep 0.02; done\necho finished\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&agent, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut doc = Document::new_empty(&dir.join("t.leo").to_string_lossy());
    let parent = doc.outline().root_position().unwrap();
    doc.set_headline(&parent, "Parent");
    let node = doc.outline_mut_untracked().insert_as_last_child(&parent);
    doc.set_headline(&node, headline);
    let sibling = doc.outline_mut_untracked().insert_as_last_child(&parent);
    doc.set_headline(&sibling, "Sibling");
    doc.set_body(&sibling, "old text\n");
    let outside = doc.outline_mut_untracked().insert_after(&parent);
    doc.set_headline(&outside, "Outside");
    doc.clear_undo();
    let mut app = App::new(doc);
    let p = find(&app, headline).unwrap();
    app.select(p);
    let (table, warnings) = Agents::parse(&format!("edit = {} {{mcp}}\n", agent.display()));
    assert!(warnings.is_empty(), "{warnings:?}");
    leo_agent::set_agents(&mut app, table);
    app
}

fn find(app: &App, h: &str) -> Option<Position> {
    let o = app.outline();
    o.all_positions().into_iter().find(|p| p.h(o) == h)
}

fn body(app: &App, h: &str) -> String {
    find(app, h).unwrap().b(app.outline()).to_string()
}

fn gnx(app: &App, h: &str) -> String {
    find(app, h).unwrap().gnx(app.outline()).to_string()
}

/// The MCP client the config file names.
struct Client {
    port: u16,
    auth: String,
}

impl Client {
    fn from(config: &Path) -> Client {
        let c: Value = serde_json::from_str(&std::fs::read_to_string(config).unwrap()).unwrap();
        let leo = &c["mcpServers"]["leo"];
        let url = leo["url"].as_str().unwrap();
        let port = url
            .strip_prefix("http://127.0.0.1:")
            .and_then(|r| r.strip_suffix("/mcp"))
            .unwrap()
            .parse()
            .unwrap();
        let auth = leo["headers"]["Authorization"]
            .as_str()
            .unwrap()
            .to_string();
        Client { port, auth }
    }

    /// The HTTP status line and the JSON reply.
    fn post(&self, auth: &str, body: &Value) -> (String, Option<Value>) {
        let mut s = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        let body = body.to_string();
        let req = format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: {auth}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        s.write_all(req.as_bytes()).unwrap();
        let mut r = BufReader::new(s);
        let mut status = String::new();
        r.read_line(&mut status).unwrap();
        let mut length = 0;
        loop {
            let mut h = String::new();
            r.read_line(&mut h).unwrap();
            if h.trim().is_empty() {
                break;
            }
            if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                length = v.trim().parse().unwrap();
            }
        }
        let mut b = vec![0; length];
        r.read_exact(&mut b).unwrap();
        (status, serde_json::from_slice(&b).ok())
    }

    /// Call `tool`; the reply's text, and whether it is an error.
    fn call(&self, tool: &str, args: Value) -> (String, bool) {
        let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": tool, "arguments": args}});
        let (_, reply) = self.post(&self.auth, &msg);
        let result = &reply.unwrap()["result"];
        (
            result["content"][0]["text"].as_str().unwrap().to_string(),
            result["isError"].as_bool().unwrap(),
        )
    }
}

/// Start the run, answer y, and wait for the agent's config file.
fn start(app: &mut App, dir: &Path) -> Client {
    app.run_command_line("agent-run");
    assert_eq!(app.mode, Mode::Confirm);
    assert!(
        app.mini_label()
            .contains("with read and edit access to 3 nodes of this outline"),
        "{}",
        app.mini_label()
    );
    assert!(app.answer_key('y'));
    client(app, dir)
}

/// Wait for the agent's copy of its config file, and connect to its server.
fn client(app: &mut App, dir: &Path) -> Client {
    let config = dir.join("config.json");
    let start = Instant::now();
    while !config.exists() {
        assert!(start.elapsed() < Duration::from_secs(10), "no config file");
        app.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    Client::from(&config)
}

/// Poll until `client` has run on a thread and the run is over. `between`
/// runs on this thread each time the client sends on its channel.
fn drive(
    app: &mut App,
    dir: &Path,
    client: impl FnOnce(mpsc::Sender<()>, mpsc::Receiver<()>) + Send + 'static,
    mut between: impl FnMut(&mut App),
) {
    let (to_test, from_client) = mpsc::channel();
    let (to_client, from_test) = mpsc::channel();
    let done = dir.join("done");
    let thread = std::thread::spawn(move || {
        client(to_test, from_test);
        std::fs::write(done, "").unwrap();
    });
    let start = Instant::now();
    while leo_agent::busy(app) {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the run never finished"
        );
        app.poll();
        if from_client.try_recv().is_ok() {
            between(app);
            to_client.send(()).unwrap();
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    thread.join().unwrap();
}

#[test]
fn the_agent_edits_the_outline_and_the_run_undoes_as_one_step() {
    let dir = dir("one-step");
    let mut app = app(&dir);
    let agent = AGENT;
    let sibling = gnx(&app, "Sibling");
    let outside = gnx(&app, "Outside");
    let client = start(&mut app, &dir);
    let path = std::fs::read_to_string(dir.join("path.txt")).unwrap();
    let config = PathBuf::from(path.trim_end());
    let mode = std::fs::metadata(&config).unwrap();
    assert_eq!(
        std::os::unix::fs::PermissionsExt::mode(&mode.permissions()) & 0o777,
        0o600,
        "the config file holds the token"
    );
    let prompt = std::fs::read_to_string(dir.join("prompt.txt")).unwrap();
    assert!(
        prompt.contains(&format!("from the node with gnx {}", gnx(&app, agent))),
        "{prompt}"
    );
    assert!(
        prompt.contains(&format!(
            "reach only the node with gnx {} and its subtree",
            gnx(&app, "Parent")
        )),
        "{prompt}"
    );
    drive(
        &mut app,
        &dir,
        move |_, _| {
            let ping = json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});
            assert!(client.post("Bearer wrong", &ping).0.contains("401"));
            let (_, error) = client.call("set_body", json!({"gnx": sibling, "body": "new text\n"}));
            assert!(!error);
            let (_, error) = client.call(
                "insert_node",
                json!({"gnx": sibling, "place": "after", "headline": "Added"}),
            );
            assert!(!error);
            for tool in ["save", "select_node"] {
                let msg = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                    "params": {"name": tool, "arguments": {}}});
                let reply = client.post(&client.auth, &msg).1.unwrap();
                assert!(
                    reply["error"]["message"]
                        .as_str()
                        .unwrap()
                        .contains("no such tool"),
                    "{reply}"
                );
            }
            let (text, error) = client.call("read_node", json!({ "gnx": outside }));
            assert!(error && text.contains("subtree"), "{text}");
        },
        |_| {},
    );
    assert_eq!(body(&app, "Sibling"), "new text\n");
    assert!(find(&app, "Added").is_some());
    assert!(
        body(&app, agent).ends_with("\nfinished\n"),
        "{}",
        body(&app, agent)
    );
    assert!(!config.exists(), "the run removes its config file");
    assert_eq!(app.doc.undoer().undo_name(), Some("agent-run"));
    app.doc.undo();
    assert_eq!(body(&app, "Sibling"), "old text\n");
    assert!(find(&app, "Added").is_none());
    assert_eq!(body(&app, agent), "");
    assert!(!app.doc.undoer().can_undo(), "one step held the whole run");
}

#[test]
fn a_user_edit_during_the_run_keeps_the_steps_apart() {
    let dir = dir("apart");
    let mut app = app(&dir);
    let sibling = gnx(&app, "Sibling");
    let client = start(&mut app, &dir);
    drive(
        &mut app,
        &dir,
        move |to_test, from_test| {
            client.call("set_body", json!({"gnx": sibling, "body": "agent 1\n"}));
            to_test.send(()).unwrap();
            from_test.recv().unwrap();
            client.call("set_body", json!({"gnx": sibling, "body": "agent 2\n"}));
        },
        |app| {
            let p = find(app, "Sibling").unwrap();
            app.doc.set_headline(&p, "Sibling, renamed");
        },
    );
    let h = "Sibling, renamed";
    assert_eq!(body(&app, h), "agent 2\n");
    // The reply and the second edit are one step; the user's rename and the
    // first edit are one each.
    app.doc.undo();
    assert_eq!(body(&app, h), "agent 1\n");
    app.doc.undo();
    assert_eq!(body(&app, "Sibling"), "agent 1\n");
    app.doc.undo();
    assert_eq!(body(&app, "Sibling"), "old text\n");
    assert!(!app.doc.undoer().can_undo());
}

#[test]
fn ask_with_an_mcp_agent_edits_and_inserts_its_reply_as_one_step() {
    let dir = dir("ask");
    let mut app = app(&dir);
    let parent = gnx(&app, "Parent");
    app.select(find(&app, "Sibling").unwrap());
    app.focus = leoapp::app::Focus::Body;
    app.editor.cursor = (0, 0);
    app.run_command_line("ask agent=edit scope=1 tidy");
    assert_eq!(
        app.message,
        "ask: edit is running, and may edit the outline"
    );
    let client = client(&mut app, &dir);
    drive(
        &mut app,
        &dir,
        move |_, _| {
            let (text, error) = client.call("set_body", json!({"gnx": parent, "body": "edited\n"}));
            assert!(!error, "{text}");
        },
        |_| {},
    );
    assert_eq!(body(&app, "Parent"), "edited\n");
    assert_eq!(body(&app, "Sibling"), "old text\nfinished\n");
    app.doc.undo();
    assert_eq!(body(&app, "Parent"), "");
    assert_eq!(body(&app, "Sibling"), "old text\n");
    assert!(!app.doc.undoer().can_undo(), "one step held the whole run");
}

#[test]
fn a_file_an_agent_names_is_not_created_until_the_user_says_so() {
    let dir = dir("held");
    let mut app = app(&dir);
    let sibling = gnx(&app, "Sibling");
    let client = start(&mut app, &dir);
    drive(
        &mut app,
        &dir,
        move |_, _| {
            let (text, error) = client.call(
                "insert_node",
                json!({"gnx": sibling, "place": "inside", "headline": "@file new.txt"}),
            );
            assert!(!error, "{text}");
        },
        |_| {},
    );
    let p = find(&app, "@file new.txt").unwrap();
    assert!(!app.outline().may_overwrite(&p));
    app.run_command_line("w");
    assert!(
        app.mini_label()
            .contains("which an MCP client asked for? (y/n)"),
        "{}",
        app.mini_label()
    );
    assert!(!dir.join("new.txt").exists());
}

#[test]
fn a_cancelled_run_stops_its_server_and_removes_its_config() {
    let dir = dir("cancel");
    let mut app = app(&dir);
    let client = start(&mut app, &dir);
    let path = std::fs::read_to_string(dir.join("path.txt")).unwrap();
    let config = PathBuf::from(path.trim_end());
    app.run_command_line("agent-cancel");
    assert_eq!(app.message, "agent: edit stopped");
    assert!(
        !config.exists(),
        "the cancelled run removes its config file"
    );
    let start = Instant::now();
    while TcpStream::connect(("127.0.0.1", client.port)).is_ok() {
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "the server still listens"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn scope_all_asks_for_the_whole_outline_and_reaches_it() {
    let dir = dir("all");
    let headline = "@agent agent=edit scope=all tidy";
    let mut app = app_with(&dir, headline);
    let outside = gnx(&app, "Outside");
    app.run_command_line("agent-run");
    assert!(
        app.mini_label()
            .contains("with read and edit access to the whole outline?"),
        "{}",
        app.mini_label()
    );
    assert!(app.answer_key('y'));
    let client = client(&mut app, &dir);
    let prompt = std::fs::read_to_string(dir.join("prompt.txt")).unwrap();
    assert!(!prompt.contains("reach only"), "{prompt}");
    drive(
        &mut app,
        &dir,
        move |_, _| {
            let (text, error) =
                client.call("set_body", json!({"gnx": outside, "body": "reached\n"}));
            assert!(!error, "{text}");
        },
        |_| {},
    );
    assert_eq!(body(&app, "Outside"), "reached\n");
}
