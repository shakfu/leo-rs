//! One language server: JSON-RPC over its stdin and stdout.
//!
//! A thread reads the server's messages into a channel, so a front end polls
//! and never blocks on a server. Messages written before the server answers
//! `initialize` wait in a queue, as the protocol requires.

use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;

use serde_json::{json, Value};

use crate::map::Encoding;

/// Called from the reader thread when a message arrives, so a front end that
/// sleeps between events can wake and poll.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Write one message with its `Content-Length` header.
pub fn write_message(w: &mut dyn Write, msg: &Value) -> io::Result<()> {
    let body = msg.to_string();
    write!(w, "Content-Length: {}\r\n\r\n{body}", body.len())?;
    w.flush()
}

/// Read one message, or None at the end of the stream.
pub fn read_message(r: &mut dyn BufRead) -> io::Result<Option<Value>> {
    let mut length = None;
    loop {
        let mut header = String::new();
        if r.read_line(&mut header)? == 0 {
            return Ok(None);
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse::<usize>().ok();
            }
        }
    }
    let length = length.ok_or_else(|| io::Error::other("message without Content-Length"))?;
    let mut body = vec![0; length];
    r.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(io::Error::other)
}

/// What the server sent: an answer to one of our requests, or a
/// notification of its own.
#[derive(Debug)]
pub enum Incoming {
    Response {
        id: i64,
        result: Result<Value, String>,
    },
    Notification {
        method: String,
        params: Value,
    },
    /// A request of ours to answer with `respond`: only those the front end
    /// acts on come here.
    Request {
        id: Value,
        method: String,
        params: Value,
    },
}

pub struct Server {
    writer: Box<dyn Write + Send>,
    incoming: Receiver<Value>,
    child: Option<Child>,
    next_id: i64,
    /// Written once `initialize` is answered.
    queue: VecDeque<Value>,
    initialize_id: i64,
    pub initialized: bool,
    pub encoding: Encoding,
    /// Set when the server stopped or a write failed; the server is dead.
    pub error: Option<String>,
}

impl Server {
    /// Start `command` (a program and its arguments, split on blanks) with
    /// `root` as its workspace.
    pub fn spawn(command: &str, root: &Path, wake: Wake) -> io::Result<Server> {
        let mut words = command.split_whitespace();
        let program = words
            .next()
            .ok_or_else(|| io::Error::other("empty server command"))?;
        let mut child = Command::new(program)
            .args(words)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // A terminal front end draws on stderr's terminal.
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take().expect("piped");
        let stdout = child.stdout.take().expect("piped");
        let mut server = Server::connect(stdout, stdin, root, wake);
        server.child = Some(child);
        Ok(server)
    }

    /// Talk to a server over `reader` and `writer`, and send `initialize`.
    pub fn connect(
        reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
        root: &Path,
        wake: Wake,
    ) -> Server {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(reader);
            while let Ok(Some(msg)) = read_message(&mut reader) {
                if tx.send(msg).is_err() {
                    break;
                }
                wake();
            }
            wake();
        });
        let mut server = Server {
            writer: Box::new(writer),
            incoming: rx,
            child: None,
            next_id: 1,
            queue: VecDeque::new(),
            initialize_id: 0,
            initialized: false,
            encoding: Encoding::Utf16,
            error: None,
        };
        server.initialize(root);
        server
    }

    fn initialize(&mut self, root: &Path) {
        let root_uri = crate::uri::from_path(root);
        let name = root
            .file_name()
            .map_or("root".into(), |n| n.to_string_lossy());
        let params = json!({
            "processId": std::process::id(),
            "clientInfo": {"name": "leolsp", "version": env!("CARGO_PKG_VERSION")},
            "rootUri": root_uri,
            "workspaceFolders": [{"uri": root_uri, "name": name}],
            "capabilities": {
                "general": {"positionEncodings": ["utf-8", "utf-16"]},
                "textDocument": {
                    "synchronization": {"didSave": false, "dynamicRegistration": false},
                    "publishDiagnostics": {"relatedInformation": false},
                    "hover": {"contentFormat": ["plaintext", "markdown"]},
                    "definition": {"linkSupport": true},
                    "rename": {"prepareSupport": false},
                    "codeAction": {
                        "isPreferredSupport": true,
                        "disabledSupport": true,
                        "codeActionLiteralSupport": {"codeActionKind": {"valueSet": [
                            "", "quickfix", "refactor", "refactor.extract", "refactor.inline",
                            "refactor.rewrite", "source", "source.organizeImports",
                        ]}},
                    },
                },
                "workspace": {
                    "workspaceFolders": true,
                    "configuration": true,
                    "applyEdit": true,
                    "executeCommand": {},
                },
            },
        });
        self.initialize_id = self.next_id;
        self.next_id += 1;
        let msg = json!({"jsonrpc": "2.0", "id": self.initialize_id, "method": "initialize", "params": params});
        self.write(&msg);
    }

    fn write(&mut self, msg: &Value) {
        if self.error.is_some() {
            return;
        }
        if let Err(e) = write_message(&mut self.writer, msg) {
            self.error = Some(format!("language server stopped: {e}"));
        }
    }

    /// Write `msg` now, or once the server is initialized.
    fn send(&mut self, msg: Value) {
        match self.initialized {
            true => self.write(&msg),
            false => self.queue.push_back(msg),
        }
    }

    /// Send a request and return its id.
    pub fn request(&mut self, method: &str, params: Value) -> i64 {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        id
    }

    pub fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    /// The messages that have arrived. A request from the server is
    /// answered here, except `workspace/applyEdit`, which the caller answers.
    pub fn poll(&mut self) -> Vec<Incoming> {
        let mut out = Vec::new();
        loop {
            let msg = match self.incoming.try_recv() {
                Ok(msg) => msg,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.error
                        .get_or_insert_with(|| "language server stopped".to_string());
                    break;
                }
            };
            let id = msg.get("id").cloned();
            match (msg.get("method").and_then(Value::as_str), id) {
                (Some("workspace/applyEdit"), Some(id)) => out.push(Incoming::Request {
                    id,
                    method: "workspace/applyEdit".to_string(),
                    params: msg["params"].clone(),
                }),
                (Some(method), Some(id)) => {
                    let result = server_request_result(method, &msg["params"]);
                    self.respond(id, result);
                }
                (Some(method), None) => out.push(Incoming::Notification {
                    method: method.to_string(),
                    params: msg["params"].clone(),
                }),
                (None, Some(id)) => {
                    let Some(id) = id.as_i64() else { continue };
                    let result = match msg.get("error") {
                        Some(e) => Err(e["message"].as_str().unwrap_or("error").to_string()),
                        None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                    };
                    if id == self.initialize_id {
                        self.initialized(result);
                    } else {
                        out.push(Incoming::Response { id, result });
                    }
                }
                (None, None) => {}
            }
        }
        out
    }

    /// Answer request `id` of the server's.
    pub fn respond(&mut self, id: Value, result: Value) {
        self.write(&json!({"jsonrpc": "2.0", "id": id, "result": result}));
    }

    fn initialized(&mut self, result: Result<Value, String>) {
        let caps = match result {
            Ok(r) => r["capabilities"].clone(),
            Err(e) => {
                self.error = Some(format!("language server refused to start: {e}"));
                return;
            }
        };
        if caps["positionEncoding"].as_str() == Some("utf-8") {
            self.encoding = Encoding::Utf8;
        }
        self.initialized = true;
        self.write(&json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}));
        while let Some(msg) = self.queue.pop_front() {
            self.write(&msg);
        }
    }
}

/// The answer to a request a server makes of its client: one null per item
/// for `workspace/configuration`, which means "no settings", else null.
fn server_request_result(method: &str, params: &Value) -> Value {
    match method {
        "workspace/configuration" => {
            let n = params["items"].as_array().map_or(0, |a| a.len());
            Value::Array(vec![Value::Null; n])
        }
        _ => Value::Null,
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if self.initialized {
            let shutdown = json!({"jsonrpc": "2.0", "id": self.next_id, "method": "shutdown"});
            self.write(&shutdown);
            self.write(&json!({"jsonrpc": "2.0", "method": "exit"}));
        }
        if let Some(mut child) = self.child.take() {
            // `exit` is a request to stop, not a promise; a server that keeps
            // running is not left behind.
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// A fake server for tests: answers `initialize`, then hands every message
/// to `answer`, which returns the messages to send back.
#[cfg(test)]
pub mod fake {
    use super::*;

    pub type Answer = Box<dyn FnMut(&Value) -> Vec<Value> + Send>;

    /// A `Server` connected to a fake that runs `answer` on its own thread.
    pub fn server(mut answer: Answer, encoding: &str) -> Server {
        let (from_client, to_server) = io::pipe().unwrap();
        let (from_server, to_client) = io::pipe().unwrap();
        let encoding = encoding.to_string();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(from_client);
            let mut writer = to_client;
            while let Ok(Some(msg)) = read_message(&mut reader) {
                let replies = match msg["method"].as_str() {
                    Some("initialize") => vec![json!({
                        "jsonrpc": "2.0", "id": msg["id"],
                        "result": {"capabilities": {"positionEncoding": encoding}},
                    })],
                    Some("exit") => break,
                    _ => answer(&msg),
                };
                for reply in replies {
                    write_message(&mut writer, &reply).unwrap();
                }
            }
        });
        Server::connect(from_server, to_server, Path::new("/tmp"), Arc::new(|| {}))
    }

    /// Poll until `done` says the messages so far are enough.
    pub fn wait(server: &mut Server, mut done: impl FnMut(&[Incoming]) -> bool) -> Vec<Incoming> {
        let mut seen = Vec::new();
        for _ in 0..500 {
            seen.extend(server.poll());
            if done(&seen) {
                return seen;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the fake server did not answer: {seen:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_round_trips_through_its_framing() {
        let msg = json!({"jsonrpc": "2.0", "method": "x", "params": {"s": "caf\u{e9}"}});
        let mut buf = Vec::new();
        write_message(&mut buf, &msg).unwrap();
        // The length counts bytes, not characters.
        assert!(buf.starts_with(format!("Content-Length: {}", msg.to_string().len()).as_bytes()));
        let mut r = io::Cursor::new(buf);
        assert_eq!(read_message(&mut r).unwrap(), Some(msg));
        assert_eq!(read_message(&mut r).unwrap(), None);
    }

    #[test]
    fn a_request_waits_for_initialize_and_gets_its_answer() {
        let mut server = fake::server(
            Box::new(|msg| match msg["method"].as_str() {
                Some("ping") => vec![json!({"jsonrpc": "2.0", "id": msg["id"], "result": "pong"})],
                _ => vec![],
            }),
            "utf-8",
        );
        let id = server.request("ping", Value::Null);
        let seen = fake::wait(&mut server, |seen| !seen.is_empty());
        assert!(server.initialized);
        assert_eq!(server.encoding, Encoding::Utf8);
        match &seen[0] {
            Incoming::Response { id: got, result } => {
                assert_eq!((*got, result.clone()), (id, Ok(json!("pong"))));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_servers_request_is_answered_for_it() {
        let mut server = fake::server(
            Box::new(|msg| match (msg["method"].as_str(), msg.get("result")) {
                (Some("go"), _) => vec![json!({
                    "jsonrpc": "2.0", "id": 99, "method": "workspace/configuration",
                    "params": {"items": [{}, {}]},
                })],
                (None, Some(result)) => vec![json!({
                    "jsonrpc": "2.0", "method": "got", "params": result,
                })],
                _ => vec![],
            }),
            "utf-16",
        );
        server.notify("go", Value::Null);
        let seen = fake::wait(&mut server, |seen| !seen.is_empty());
        match &seen[0] {
            Incoming::Notification { method, params } => {
                assert_eq!((method.as_str(), params), ("got", &json!([null, null])));
            }
            other => panic!("{other:?}"),
        }
    }
}
