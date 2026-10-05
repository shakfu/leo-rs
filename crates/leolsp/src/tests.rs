//! `Lsp` against a fake server: an outline's file goes out as one document,
//! and every answer comes back in body rows.

use super::*;
use std::sync::{Arc, Mutex};

/// `@clean x.py` whose `@others` is indented, with child `f`.
fn outline() -> (Outline, Position, Position) {
    let mut o = Outline::new_empty();
    let root = o.root_position().unwrap();
    o.set_headline(&root, "@clean x.py");
    o.set_body(
        &root,
        "@language python\nimport os\nclass A:\n    @others\n",
    );
    let f = o.insert_as_last_child(&root);
    o.set_headline(&f, "f");
    o.set_body(&f, "def f():\n    return 1\n");
    (o, root, f)
}

/// What the fake has been sent.
type Seen = Arc<Mutex<Vec<Value>>>;

/// The `didOpen`s the fake has been sent.
fn opened(seen: &Seen) -> Vec<Value> {
    let seen = seen.lock().unwrap();
    seen.iter()
        .filter(|m| m["method"] == "textDocument/didOpen")
        .map(|m| m["params"]["textDocument"].clone())
        .collect()
}

/// An `Lsp` whose python server is a fake that answers as a server would
/// for `outline()`'s file: a diagnostic on `return`, a hover, a definition
/// and a rename of `f`.
fn lsp(seen: Seen) -> Lsp {
    let configs = vec![ServerConfig {
        language: "python".into(),
        command: "fake".into(),
    }];
    let connect: Connect = Box::new(move |_, _, _| {
        let seen = seen.clone();
        let mut uri = Value::Null;
        Ok(server::fake::server(
            Box::new(move |msg| {
                seen.lock().unwrap().push(msg.clone());
                let reply = |result: Value| {
                    vec![json!({"jsonrpc": "2.0", "id": msg["id"], "result": result})]
                };
                // The file: import os / class A: / def f(): / return 1, the
                // last two behind the four-space `@others` indent.
                let range = |l: u32, a: u32, b: u32| json!({"start": {"line": l, "character": a}, "end": {"line": l, "character": b}});
                match msg["method"].as_str() {
                    Some("textDocument/didOpen") => {
                        uri = msg["params"]["textDocument"]["uri"].clone();
                        vec![json!({
                            "jsonrpc": "2.0", "method": "textDocument/publishDiagnostics",
                            "params": {"uri": uri, "diagnostics": [
                                {"range": range(3, 8, 14), "severity": 2, "message": "unreachable"},
                            ]},
                        })]
                    }
                    Some("textDocument/hover") => reply(
                        json!({"contents": {"kind": "plaintext", "value": "def f()\n-> int"}}),
                    ),
                    Some("textDocument/definition") => {
                        reply(json!({"uri": uri, "range": range(2, 8, 9)}))
                    }
                    Some("textDocument/codeAction") => {
                        let n = msg["params"]["context"]["diagnostics"]
                            .as_array()
                            .unwrap()
                            .len();
                        reply(json!([
                            {"title": format!("return 2 ({n} diagnostic)"), "kind": "quickfix",
                             "isPreferred": true,
                             "edit": {"changes": {uri.as_str().unwrap(): [
                                 {"range": range(3, 15, 16), "newText": "2"},
                             ]}}},
                            {"title": "organize", "command": "organize", "arguments": []},
                            {"title": "off", "disabled": {"reason": "not here"}},
                        ]))
                    }
                    Some("workspace/executeCommand") => vec![
                        json!({"jsonrpc": "2.0", "id": msg["id"], "result": null}),
                        json!({"jsonrpc": "2.0", "id": 77, "method": "workspace/applyEdit",
                        "params": {"label": "organize", "edit": {"changes": {
                            uri.as_str().unwrap(): [{"range": range(0, 7, 9), "newText": "sys"}],
                        }}}}),
                    ],
                    Some("textDocument/rename") => {
                        reply(json!({"changes": {uri.as_str().unwrap(): [
                            {"range": range(2, 8, 9), "newText": msg["params"]["newName"]},
                        ]}}))
                    }
                    _ => vec![],
                }
            }),
            "utf-16",
        ))
    });
    Lsp::with_connect(configs, PathBuf::from("/tmp"), Arc::new(|| {}), connect)
}

/// Poll until `done`.
fn wait(lsp: &mut Lsp, o: &Outline, mut done: impl FnMut(&Event) -> bool) -> Event {
    for _ in 0..500 {
        if let Some(event) = lsp.poll(o).into_iter().find(|e| done(e)) {
            return event;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    panic!("no answer");
}

fn at(o: &Outline, p: &Position, row: usize, col: usize) -> BodyPos {
    BodyPos {
        gnx: p.gnx(o).to_string(),
        row,
        col,
    }
}

#[test]
fn a_file_is_opened_once_and_its_diagnostics_land_in_the_body() {
    let seen: Seen = Default::default();
    let mut lsp = lsp(seen.clone());
    let (o, root, f) = outline();
    assert!(lsp.sync(&o, &f).is_empty());
    wait(&mut lsp, &o, |e| *e == Event::Diagnostics);
    // `return` is body column 4 of row 1, behind the indent in the file.
    let d = lsp.diagnostics(f.gnx(&o));
    assert_eq!(d.len(), 1);
    assert_eq!(
        (d[0].row, d[0].col, d[0].end_row, d[0].end_col),
        (1, 4, 1, 10)
    );
    assert_eq!(d[0].severity, Severity::Warning);
    assert!(lsp.diagnostics(root.gnx(&o)).is_empty());
    // The root is in the same document: syncing it opens nothing new.
    lsp.sync(&o, &root);
    let opens = opened(&seen);
    assert_eq!(opens.len(), 1);
    assert_eq!(
        opens[0]["text"],
        "import os\nclass A:\n    def f():\n        return 1\n"
    );
}

#[test]
fn an_edit_is_sent_as_a_new_version() {
    let seen: Seen = Default::default();
    let mut lsp = lsp(seen.clone());
    let (mut o, _, f) = outline();
    lsp.sync(&o, &f);
    o.set_body(&f, "def f():\n    return 2\n");
    lsp.sync(&o, &f);
    // Unchanged text sends nothing.
    lsp.sync(&o, &f);
    wait(&mut lsp, &o, |e| *e == Event::Diagnostics);
    for _ in 0..100 {
        if seen
            .lock()
            .unwrap()
            .iter()
            .any(|m| m["method"] == "textDocument/didChange")
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let seen = seen.lock().unwrap();
    let changes: Vec<&Value> = seen
        .iter()
        .filter(|m| m["method"] == "textDocument/didChange")
        .collect();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0]["params"]["textDocument"]["version"], 2);
    let text = changes[0]["params"]["contentChanges"][0]["text"]
        .as_str()
        .unwrap();
    assert!(text.ends_with("        return 2\n"));
}

#[test]
fn hover_definition_and_rename_answer_in_body_positions() {
    let mut lsp = lsp(Default::default());
    let (o, _, f) = outline();
    lsp.sync(&o, &f);
    let gnx = f.gnx(&o).to_string();

    lsp.request(&gnx, 1, 4, Request::Hover).unwrap();
    let hover = wait(&mut lsp, &o, |e| matches!(e, Event::Hover(_)));
    assert_eq!(hover, Event::Hover(vec!["def f()".into(), "-> int".into()]));

    lsp.request(&gnx, 1, 4, Request::Definition).unwrap();
    let def = wait(&mut lsp, &o, |e| matches!(e, Event::Definition(_)));
    assert_eq!(def, Event::Definition(vec![Target::Body(at(&o, &f, 0, 4))]));

    lsp.request(&gnx, 0, 4, Request::Rename("g".into()))
        .unwrap();
    let rename = wait(&mut lsp, &o, |e| matches!(e, Event::Rename(_)));
    let edit = BodyEdit {
        start: at(&o, &f, 0, 4),
        end: at(&o, &f, 0, 5),
        text: "g".into(),
    };
    assert_eq!(rename, Event::Rename(Ok(vec![edit])));
}

#[test]
fn a_request_on_a_line_the_file_does_not_hold_is_refused() {
    let mut lsp = lsp(Default::default());
    let (o, root, _) = outline();
    lsp.sync(&o, &root);
    // Row 0 is `@language python`, which an `@clean` file does not write.
    let err = lsp.request(root.gnx(&o), 0, 0, Request::Hover).unwrap_err();
    assert_eq!(err, "this line is not in the file as written");
    assert!(lsp.request("nobody", 0, 0, Request::Hover).is_err());
}

#[test]
fn a_rename_from_stale_text_is_refused() {
    let mut lsp = lsp(Default::default());
    let (mut o, _, f) = outline();
    lsp.sync(&o, &f);
    lsp.request(f.gnx(&o), 0, 4, Request::Rename("g".into()))
        .unwrap();
    o.set_body(&f, "def f():\n    return 3\n");
    lsp.sync(&o, &f);
    let rename = wait(&mut lsp, &o, |e| matches!(e, Event::Rename(_)));
    assert!(matches!(rename, Event::Rename(Err(_))));
}

#[test]
fn a_node_in_no_file_is_a_document_of_its_own() {
    let seen: Seen = Default::default();
    let mut lsp = lsp(seen.clone());
    let mut o = Outline::new_empty();
    let p = o.root_position().unwrap();
    o.set_body(&p, "@language python\nx = 1\n");
    lsp.sync(&o, &p);
    wait(&mut lsp, &o, |e| *e == Event::Diagnostics);
    let doc = &opened(&seen)[0];
    assert_eq!(doc["text"], "\nx = 1\n");
    assert_eq!(doc["uri"], format!("untitled:leo/{}", p.gnx(&o)));
}

#[test]
fn a_language_without_a_server_starts_nothing() {
    let mut lsp = lsp(Default::default());
    let mut o = Outline::new_empty();
    let p = o.root_position().unwrap();
    o.set_body(&p, "@language rust\nfn main() {}\n");
    assert!(lsp.sync(&o, &p).is_empty());
    assert!(!lsp.serves(p.gnx(&o)));
}

#[test]
fn a_server_that_cannot_start_is_said_once() {
    let configs = vec![ServerConfig {
        language: "python".into(),
        command: "/nonexistent/leolsp-server".into(),
    }];
    let mut lsp = Lsp::new(configs, PathBuf::from("/tmp"), Arc::new(|| {}));
    let (o, _, f) = outline();
    let events = lsp.sync(&o, &f);
    assert!(matches!(events.as_slice(), [Event::Message(m)] if m.contains("leolsp-server")));
    assert!(lsp.sync(&o, &f).is_empty());
}

#[test]
fn code_actions_come_with_their_edits_in_body_positions() {
    let seen: Seen = Default::default();
    let mut lsp = lsp(seen.clone());
    let (o, root, f) = outline();
    lsp.sync(&o, &f);
    wait(&mut lsp, &o, |e| *e == Event::Diagnostics);
    lsp.request(f.gnx(&o), 1, 4, Request::CodeActions).unwrap();
    let Event::CodeActions(actions) = wait(&mut lsp, &o, |e| matches!(e, Event::CodeActions(_)))
    else {
        unreachable!()
    };
    // The disabled one is left out; the line's diagnostic went with the ask.
    let titles: Vec<&str> = actions.iter().map(|a| a.title.as_str()).collect();
    assert_eq!(titles, ["return 2 (1 diagnostic)", "organize"]);
    assert!(actions[0].preferred);
    assert_eq!(actions[0].kind.as_deref(), Some("quickfix"));
    let fix = BodyEdit {
        start: at(&o, &f, 1, 11),
        end: at(&o, &f, 1, 12),
        text: "2".into(),
    };
    assert_eq!(actions[0].edit, Some(Ok(vec![fix])));

    // A command runs on the server, which sends its edit back to apply.
    let command = actions[1].command.clone().unwrap();
    lsp.execute(f.gnx(&o), &command).unwrap();
    let edit = wait(&mut lsp, &o, |e| matches!(e, Event::Edit(..)));
    let organize = BodyEdit {
        start: at(&o, &root, 1, 7),
        end: at(&o, &root, 1, 9),
        text: "sys".into(),
    };
    assert_eq!(edit, Event::Edit("organize".into(), Ok(vec![organize])));
    for _ in 0..100 {
        if seen.lock().unwrap().iter().any(|m| m["id"] == 77) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let seen = seen.lock().unwrap();
    let answer = seen.iter().find(|m| m["id"] == 77).expect("answered");
    assert_eq!(answer["result"], json!({"applied": true}));
}

/// `top` with `@clean a.py` and `@clean b.py`, each with one child.
fn two_files() -> (Outline, Vec<Position>) {
    let mut o = Outline::new_empty();
    let top = o.root_position().unwrap();
    o.set_headline(&top, "top");
    o.set_body(&top, "@language python\n");
    let mut leaves = Vec::new();
    for name in ["a", "b"] {
        let file = o.insert_as_last_child(&top);
        o.set_headline(&file, &format!("@clean {name}.py"));
        o.set_body(&file, "@others\n");
        let leaf = o.insert_as_last_child(&file);
        o.set_headline(&leaf, name);
        o.set_body(&leaf, &format!("{name} = 1\n"));
        leaves.push(leaf);
    }
    (o, leaves)
}

/// The text of each `didChange` sent for a file named `name`, waiting a
/// little for `want` of them.
fn changes(seen: &Seen, name: &str, want: usize) -> Vec<String> {
    let find = || -> Vec<String> {
        seen.lock()
            .unwrap()
            .iter()
            .filter(|m| m["method"] == "textDocument/didChange")
            .filter(|m| {
                m["params"]["textDocument"]["uri"]
                    .as_str()
                    .is_some_and(|u| u.ends_with(name))
            })
            .map(|m| {
                m["params"]["contentChanges"][0]["text"]
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect()
    };
    for _ in 0..100 {
        if find().len() >= want {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    find()
}

#[test]
fn an_edit_in_another_open_file_is_sent_and_nothing_else_is() {
    let seen: Seen = Default::default();
    let mut lsp = lsp(seen.clone());
    let (mut o, leaves) = two_files();
    lsp.sync(&o, &leaves[0]);
    lsp.sync(&o, &leaves[1]);
    wait(&mut lsp, &o, |e| *e == Event::Diagnostics);
    // Moving between them, or editing nothing, sends nothing.
    lsp.sync(&o, &leaves[0]);
    lsp.sync(&o, &leaves[1]);
    o.set_body(&leaves[1], "b = 1\n");
    lsp.sync(&o, &leaves[0]);
    // With `a` selected, `b`'s edit still goes out.
    o.set_body(&leaves[1], "b = 2\n");
    lsp.sync(&o, &leaves[0]);
    assert_eq!(changes(&seen, "b.py", 1), ["b = 2\n"]);
    assert!(changes(&seen, "a.py", 0).is_empty());
}

#[test]
fn a_file_whose_node_moved_is_found_again() {
    let seen: Seen = Default::default();
    let mut lsp = lsp(seen.clone());
    let (mut o, leaves) = two_files();
    lsp.sync(&o, &leaves[1]);
    wait(&mut lsp, &o, |e| *e == Event::Diagnostics);
    // A node inserted before both files moves every position after it.
    let top = o.root_position().unwrap();
    let first = top.first_child(&o).unwrap();
    o.insert_before(&first);
    let leaf = o
        .all_positions()
        .into_iter()
        .find(|p| p.h(&o) == "b")
        .unwrap();
    o.set_body(&leaf, "b = 3\n");
    lsp.sync(&o, &leaf);
    assert_eq!(changes(&seen, "b.py", 1), ["b = 3\n"]);
}
