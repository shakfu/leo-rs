//! The server against a raw HTTP client: what a request must carry, and a
//! tool call answered by an app thread.

use super::*;
use std::io::Read;

fn tools() -> Vec<Tool> {
    vec![Tool {
        name: "echo",
        description: "say it back",
        input_schema: json!({"type": "object"}),
    }]
}

/// POST one message, and the status line and body of the answer.
fn post(port: u16, headers: &[(&str, &str)], body: &Value) -> (String, Option<Value>) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let body = body.to_string();
    let mut req = format!("POST /mcp HTTP/1.1\r\nContent-Length: {}\r\n", body.len());
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(&body);
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
    let value = (length > 0).then(|| serde_json::from_slice(&b).unwrap());
    (status.trim().to_string(), value)
}

fn auth(port: u16) -> Vec<(&'static str, String)> {
    vec![
        ("Host", format!("127.0.0.1:{port}")),
        ("Authorization", "Bearer secret".to_string()),
    ]
}

fn as_refs<'a>(h: &'a [(&'static str, String)]) -> Vec<(&'static str, &'a str)> {
    h.iter().map(|(k, v)| (*k, v.as_str())).collect()
}

#[test]
fn a_request_needs_the_token_and_a_local_host_and_origin() {
    let server = Server::start(0, "secret", tools(), "test").unwrap();
    let port = server.port();
    let ping = json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});
    let host = format!("127.0.0.1:{port}");
    let (status, _) = post(port, &[("Host", &host)], &ping);
    assert!(status.contains("401"), "{status}");
    let (status, _) = post(
        port,
        &[("Host", "evil.example"), ("Authorization", "Bearer secret")],
        &ping,
    );
    assert!(status.contains("403"), "{status}");
    let owned = auth(port);
    let mut h = as_refs(&owned);
    h.push(("Origin", "https://evil.example"));
    let (status, _) = post(port, &h, &ping);
    assert!(status.contains("403"), "{status}");
    let (status, body) = post(port, &as_refs(&auth(port)), &ping);
    assert!(status.contains("200"), "{status}");
    assert_eq!(body.unwrap()["result"], json!({}));
}

#[test]
fn initialize_and_tools_list_are_answered_without_the_app() {
    let server = Server::start(0, "secret", tools(), "leo").unwrap();
    let h = auth(server.port());
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "t"}}});
    let (_, body) = post(server.port(), &as_refs(&h), &init);
    let result = &body.unwrap()["result"];
    assert_eq!(result["protocolVersion"], "2025-03-26");
    assert_eq!(result["serverInfo"]["name"], "leo");
    let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
    let (status, body) = post(server.port(), &as_refs(&h), &note);
    assert!(status.contains("202") && body.is_none(), "{status}");
    let list = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
    let (_, body) = post(server.port(), &as_refs(&h), &list);
    assert_eq!(body.unwrap()["result"]["tools"][0]["name"], "echo");
}

#[test]
fn a_tool_call_is_answered_by_whoever_polls() {
    let server = Server::start(0, "secret", tools(), "test").unwrap();
    let port = server.port();
    // An unknown tool is refused without asking the app.
    let unknown = json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call",
        "params": {"name": "rm", "arguments": {}}});
    let (_, body) = post(port, &as_refs(&auth(port)), &unknown);
    assert_eq!(body.unwrap()["error"]["code"], -32602);
    let app = std::thread::spawn(move || loop {
        if let Some(call) = server.poll().into_iter().next() {
            let said = call.arguments["say"].clone();
            call.reply(Ok(json!({ "said": said })));
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    });
    let call = json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "echo", "arguments": {"say": "hi"}}});
    let (_, body) = post(port, &as_refs(&auth(port)), &call);
    app.join().unwrap();
    let result = &body.unwrap()["result"];
    assert_eq!(result["isError"], false);
    assert_eq!(result["content"][0]["text"], "{\"said\":\"hi\"}");
}

#[test]
fn hosts_and_tokens() {
    assert!(is_local("127.0.0.1:7341") && is_local("localhost") && is_local("[::1]:80"));
    assert!(is_local("http://localhost:3000"));
    assert!(!is_local("127.0.0.1.evil.example") && !is_local("example.com"));
    assert!(same("abc", "abc") && !same("abc", "abd") && !same("abc", "ab"));
    let (a, b) = (new_token(), new_token());
    assert_eq!(a.len(), 32);
    assert_ne!(a, b);
}
