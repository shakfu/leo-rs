//! leomcp: an MCP server over streamable HTTP, on 127.0.0.1 only.
//!
//! The protocol is the [Model Context Protocol]'s streamable HTTP transport
//! in its simplest form: a client POSTs one JSON-RPC message to `/mcp` and
//! gets one JSON response back. `initialize`, `ping` and `tools/list` are
//! answered here, on the server's own threads. A `tools/call` is handed to
//! the app through `poll`, because the outline lives on the app's thread and
//! an edit must land there, in its undo history, while the user watches.
//!
//! Every request must carry the token as `Authorization: Bearer TOKEN`, and
//! its `Host` and any `Origin` must be local: a web page the user opens can
//! reach 127.0.0.1, and the token and the origin check are what stop it.
//!
//! [Model Context Protocol]: https://modelcontextprotocol.io

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

/// Called from a server thread when a tool call is waiting, so a front end
/// that sleeps between events can wake and `poll`.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// A tool the server offers: its name, what it does, and its arguments as
/// JSON Schema.
#[derive(Clone, Debug)]
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

/// A tool call waiting for the app to answer it.
pub struct Call {
    pub name: String,
    pub arguments: Value,
    reply: Sender<Result<Value, String>>,
}

impl Call {
    /// Answer with a result, which the client receives as text, or an error.
    pub fn reply(self, result: Result<Value, String>) {
        let _ = self.reply.send(result);
    }
}

/// How long a server thread waits for the app to answer a tool call. A
/// front end polls every frame or tick, so this is reached only if it hangs.
const ANSWER_WITHIN: Duration = Duration::from_secs(30);

/// Connections served at once, across servers in this process.
static OPEN: AtomicUsize = AtomicUsize::new(0);
const MAX_CONNECTIONS: usize = 16;
/// The longest request or header line, and the most headers, read before
/// the request is authorized.
const MAX_LINE: u64 = 8 << 10;
const MAX_HEADERS: usize = 100;

/// The protocol versions this server speaks, newest first.
const VERSIONS: &[&str] = &["2025-06-18", "2025-03-26"];

pub struct Server {
    calls: Receiver<Call>,
    port: u16,
    stop: Arc<AtomicBool>,
    wake: Arc<Mutex<Wake>>,
}

struct Shared {
    token: String,
    tools: Vec<Tool>,
    name: String,
    calls: Mutex<Sender<Call>>,
    wake: Arc<Mutex<Wake>>,
    stop: Arc<AtomicBool>,
}

impl Server {
    /// Listen on 127.0.0.1:`port`, offering `tools` to clients that send
    /// `token`. Port 0 picks a free one, which `port` then says.
    pub fn start(port: u16, token: &str, tools: Vec<Tool>, name: &str) -> io::Result<Server> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let port = listener.local_addr()?.port();
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let wake: Arc<Mutex<Wake>> = Arc::new(Mutex::new(Arc::new(|| {})));
        let shared = Arc::new(Shared {
            token: token.to_string(),
            tools,
            name: name.to_string(),
            calls: Mutex::new(tx),
            wake: wake.clone(),
            stop: stop.clone(),
        });
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                if shared.stop.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                // A client needs one or two; more is a local process
                // spending the editor's threads and memory.
                if OPEN.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    OPEN.fetch_sub(1, Ordering::SeqCst);
                    continue;
                }
                let shared = shared.clone();
                std::thread::spawn(move || {
                    let _ = serve(stream, &shared);
                    OPEN.fetch_sub(1, Ordering::SeqCst);
                });
            }
        });
        Ok(Server {
            calls: rx,
            port,
            stop,
            wake,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Call `wake` when a tool call arrives from now on.
    pub fn set_wake(&self, wake: Wake) {
        if let Ok(mut w) = self.wake.lock() {
            *w = wake;
        }
    }

    /// The tool calls waiting. Answer each with `Call::reply`.
    pub fn poll(&self) -> Vec<Call> {
        self.calls.try_iter().collect()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // The accept loop wakes for a connection, so make one.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

/// One HTTP request: method, path, headers by lower-case name, body.
struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// One line of at most [`MAX_LINE`] bytes, or an error; 0 at the end.
fn read_line(r: &mut impl BufRead, line: &mut String) -> io::Result<usize> {
    let n = r.by_ref().take(MAX_LINE).read_line(line)?;
    if n as u64 == MAX_LINE && !line.ends_with('\n') {
        return Err(io::Error::other("line too long"));
    }
    Ok(n)
}

/// Read one request's line and headers, or None at the end of the
/// connection. The body is read by [`read_body`], once the request is
/// authorized.
fn read_request(r: &mut impl BufRead) -> io::Result<Option<Request>> {
    let mut line = String::new();
    if read_line(r, &mut line)? == 0 {
        return Ok(None);
    }
    let mut parts = line.split_whitespace();
    let (method, path) = (
        parts.next().unwrap_or("").to_string(),
        parts.next().unwrap_or("").to_string(),
    );
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        if read_line(r, &mut h)? == 0 {
            return Ok(None);
        }
        if headers.len() == MAX_HEADERS {
            return Err(io::Error::other("too many headers"));
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    Ok(Some(Request {
        method,
        path,
        headers,
        body: Vec::new(),
    }))
}

/// Read the body `req`'s Content-Length gives.
fn read_body(r: &mut impl BufRead, req: &mut Request) -> io::Result<()> {
    let length: usize = req
        .header("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    // A request this large is not an MCP message.
    if length > 16 << 20 {
        return Err(io::Error::other("request too large"));
    }
    req.body = vec![0; length];
    r.read_exact(&mut req.body)
}

fn respond(w: &mut impl Write, status: &str, body: Option<&Value>) -> io::Result<()> {
    let text = body.map(|b| b.to_string()).unwrap_or_default();
    let kind = if body.is_some() {
        "Content-Type: application/json\r\n"
    } else {
        ""
    };
    write!(
        w,
        "HTTP/1.1 {status}\r\n{kind}Content-Length: {}\r\n\r\n{text}",
        text.len()
    )?;
    w.flush()
}

/// Whether `host` names this machine: `127.0.0.1`, `localhost` or `[::1]`,
/// with or without a port. A DNS-rebinding page sends its own name.
fn is_local(host: &str) -> bool {
    let host = host
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let name = match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => host.split(':').next().unwrap_or(""),
    };
    matches!(name, "127.0.0.1" | "localhost" | "::1")
}

/// Compare without stopping at the first difference.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}

fn serve(stream: TcpStream, shared: &Shared) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(300)))?;
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    while let Some(mut req) = read_request(&mut reader)? {
        if shared.stop.load(Ordering::SeqCst) {
            break;
        }
        let local =
            req.header("host").is_some_and(is_local) && req.header("origin").is_none_or(is_local);
        let token = req
            .header("authorization")
            .and_then(|v| v.strip_prefix("Bearer "))
            .is_some_and(|t| same(t, &shared.token));
        // Refused before its body is read, so the connection ends here.
        if !local || !token {
            let status = if local {
                "401 Unauthorized"
            } else {
                "403 Forbidden"
            };
            respond(&mut writer, status, None)?;
            break;
        }
        read_body(&mut reader, &mut req)?;
        match (local, token, req.method.as_str(), req.path.as_str()) {
            (_, _, "POST", "/mcp") => {
                let msg: Value = match serde_json::from_slice(&req.body) {
                    Ok(v) => v,
                    Err(e) => {
                        let err = error(Value::Null, -32700, &format!("parse error: {e}"));
                        respond(&mut writer, "400 Bad Request", Some(&err))?;
                        continue;
                    }
                };
                match handle(&msg, shared) {
                    Some(reply) => respond(&mut writer, "200 OK", Some(&reply))?,
                    None => respond(&mut writer, "202 Accepted", None)?,
                }
            }
            // No server-sent stream and no sessions to end.
            (_, _, "GET" | "DELETE", "/mcp") => {
                respond(&mut writer, "405 Method Not Allowed", None)?
            }
            _ => respond(&mut writer, "404 Not Found", None)?,
        }
    }
    Ok(())
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// Answer one JSON-RPC message; None for a notification.
fn handle(msg: &Value, shared: &Shared) -> Option<Value> {
    let id = msg.get("id")?.clone();
    let method = msg["method"].as_str().unwrap_or("");
    let ok = |result: Value| json!({"jsonrpc": "2.0", "id": id, "result": result});
    Some(match method {
        "initialize" => {
            let asked = msg["params"]["protocolVersion"].as_str().unwrap_or("");
            let version = VERSIONS
                .iter()
                .find(|v| **v == asked)
                .unwrap_or(&VERSIONS[0]);
            ok(json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": shared.name, "version": env!("CARGO_PKG_VERSION")},
            }))
        }
        "ping" => ok(json!({})),
        "tools/list" => {
            let tools: Vec<Value> = shared
                .tools
                .iter()
                .map(|t| json!({"name": t.name, "description": t.description, "inputSchema": t.input_schema}))
                .collect();
            ok(json!({ "tools": tools }))
        }
        "tools/call" => {
            let name = msg["params"]["name"].as_str().unwrap_or("").to_string();
            if !shared.tools.iter().any(|t| t.name == name) {
                return Some(error(id, -32602, &format!("no such tool: {name}")));
            }
            let arguments = msg["params"]["arguments"].clone();
            let (tx, rx) = mpsc::channel();
            let call = Call {
                name,
                arguments,
                reply: tx,
            };
            let sent = shared
                .calls
                .lock()
                .map(|c| c.send(call).is_ok())
                .unwrap_or(false);
            if !sent {
                return Some(error(id, -32603, "the app has stopped"));
            }
            if let Ok(wake) = shared.wake.lock() {
                wake();
            }
            let (text, is_error) = match rx.recv_timeout(ANSWER_WITHIN) {
                Ok(Ok(value)) => (value.to_string(), false),
                Ok(Err(e)) => (e, true),
                Err(_) => ("the app did not answer".to_string(), true),
            };
            ok(json!({"content": [{"type": "text", "text": text}], "isError": is_error}))
        }
        _ => error(id, -32601, &format!("no such method: {method}")),
    })
}

/// A token for the settings file: 128 bits from the standard library's
/// randomly seeded hasher, which draws its keys from the system.
pub fn new_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let half = || {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        h.finish()
    };
    format!("{:016x}{:016x}", half(), half())
}

#[cfg(test)]
mod tests;
