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

/// Send `raw` and read the status line, or "" if the server closed first.
fn send_raw(port: u16, raw: &[u8]) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let _ = s.write_all(raw);
    let mut status = String::new();
    let _ = BufReader::new(s).read_line(&mut status);
    status.trim().to_string()
}

#[test]
fn an_unauthorized_or_oversized_request_is_refused_and_the_server_keeps_answering() {
    let server = Server::start(0, "secret", tools(), "test").unwrap();
    let port = server.port();
    // A huge body is never read without the token.
    let status = send_raw(
        port,
        format!("POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 16000000\r\n\r\n")
            .as_bytes(),
    );
    assert!(status.contains("401"), "{status}");
    // A line with no end is cut off.
    let long = format!("GET /{} HTTP/1.1\r\n\r\n", "a".repeat(20_000));
    assert_eq!(send_raw(port, long.as_bytes()), "");
    // A body that is not JSON is a parse error.
    let mut req = format!("POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer secret\r\nContent-Length: 3\r\n\r\n").into_bytes();
    req.extend_from_slice(b"{x}");
    assert!(send_raw(port, &req).contains("400"));
    let ping = json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});
    let (status, _) = post(port, &as_refs(&auth(port)), &ping);
    assert!(status.contains("200"), "{status}");
}

#[test]
fn a_new_token_replaces_the_old_one_at_once() {
    let server = Server::start(0, "secret", tools(), "test").unwrap();
    let port = server.port();
    let ping = json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});
    server.set_token("fresh");
    let (status, _) = post(port, &as_refs(&auth(port)), &ping);
    assert!(status.contains("401"), "{status}");
    let host = format!("127.0.0.1:{port}");
    let h = [("Host", host.as_str()), ("Authorization", "Bearer fresh")];
    let (status, _) = post(port, &h, &ping);
    assert!(status.contains("200"), "{status}");
}

/// A per-read timeout let a client sending a byte at a time hold one of the
/// 16 connections for as long as it liked.
#[test]
fn a_trickled_request_is_cut_off_at_the_deadline() {
    let server = Server::start(0, "secret", tools(), "test").unwrap();
    let mut s = TcpStream::connect(("127.0.0.1", server.port())).unwrap();
    let start = Instant::now();
    let mut w = s.try_clone().unwrap();
    std::thread::spawn(move || {
        for b in b"POST /mcp HTTP/1.1\r\nX-Slow: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" {
            std::thread::sleep(Duration::from_millis(200));
            if w.write_all(&[*b]).is_err() {
                return;
            }
        }
    });
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut buf = [0; 64];
    let _ = s.read(&mut buf);
    let took = start.elapsed();
    assert!(took >= HEAD_WITHIN && took < HEAD_WITHIN * 3, "{took:?}");
}

fn post_raw(port: u16, head: &str, body: &str) -> String {
    let req = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer secret\r\n{head}\r\n{body}"
    );
    send_raw(port, req.as_bytes())
}

#[test]
fn a_body_needs_a_content_length() {
    let server = Server::start(0, "secret", tools(), "test").unwrap();
    let port = server.port();
    let ping = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
    assert!(post_raw(port, "", ping).contains("411"));
    assert!(post_raw(port, "Content-Length: x\r\n", ping).contains("400"));
    assert!(post_raw(port, "Transfer-Encoding: chunked\r\n", ping).contains("501"));
}

#[test]
fn a_reply_that_ends_the_connection_says_so() {
    let server = Server::start(0, "secret", tools(), "test").unwrap();
    let port = server.port();
    for head in [
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer secret\r\n\r\n",
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
    ] {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(head.as_bytes()).unwrap();
        let mut all = String::new();
        s.read_to_string(&mut all).unwrap();
        assert!(all.contains("\r\nConnection: close\r\n"), "{all}");
    }
}

#[test]
fn a_posted_response_is_accepted_without_a_reply() {
    let server = Server::start(0, "secret", tools(), "test").unwrap();
    let port = server.port();
    for msg in [
        json!({"jsonrpc": "2.0", "id": 7, "result": {}}),
        json!({"jsonrpc": "2.0", "id": 7, "error": {"code": 1, "message": "no"}}),
    ] {
        let (status, body) = post(port, &as_refs(&auth(port)), &msg);
        assert!(status.contains("202"), "{status}");
        assert!(body.is_none());
    }
}

#[test]
fn the_auth_scheme_is_case_insensitive() {
    let server = Server::start(0, "secret", tools(), "test").unwrap();
    let port = server.port();
    let host = format!("127.0.0.1:{port}");
    let ping = json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});
    for value in ["bearer secret", "BEARER  secret"] {
        let (status, _) = post(port, &[("Host", &host), ("Authorization", value)], &ping);
        assert!(status.contains("200"), "{value}: {status}");
    }
    let basic = [("Host", host.as_str()), ("Authorization", "Basic secret")];
    let (status, _) = post(port, &basic, &ping);
    assert!(status.contains("401"), "{status}");
}

#[test]
fn a_batch_is_answered_for_2025_03_26_and_refused_after() {
    let server = Server::start(0, "secret", tools(), "test").unwrap();
    let port = server.port();
    let batch = json!([
        {"jsonrpc": "2.0", "id": 1, "method": "ping"},
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
        1,
    ]);
    let (status, body) = post(port, &as_refs(&auth(port)), &batch);
    assert!(status.contains("200"), "{status}");
    let replies = body.unwrap();
    let ids: Vec<&Value> = replies
        .as_array()
        .unwrap()
        .iter()
        .map(|r| &r["id"])
        .collect();
    assert_eq!(ids, [&json!(1), &json!(2), &Value::Null]);
    assert_eq!(replies[2]["error"]["code"], -32600);
    let notes = json!([{"jsonrpc": "2.0", "method": "notifications/initialized"}]);
    let (status, body) = post(port, &as_refs(&auth(port)), &notes);
    assert!(status.contains("202") && body.is_none(), "{status}");
    let owned = auth(port);
    let mut h = as_refs(&owned);
    h.push(("MCP-Protocol-Version", "2025-06-18"));
    let (status, body) = post(port, &h, &batch);
    assert!(status.contains("400"), "{status}");
    assert_eq!(body.unwrap()["error"]["code"], -32600);
}

/// The reply said "did not answer" though the app could still run the call
/// at its next poll.
#[test]
fn a_call_the_app_never_took_is_withdrawn() {
    let server = Server::start(0, "secret", tools(), "test").unwrap();
    let port = server.port();
    let call = json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "echo", "arguments": {}}});
    let (_, body) = post(port, &as_refs(&auth(port)), &call);
    let result = &body.unwrap()["result"];
    assert_eq!(result["isError"], true);
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("did not run"), "{text}");
    assert!(server.poll().is_empty());
}

#[test]
fn a_call_the_app_took_is_answered_however_long_it_runs() {
    let server = Server::start(0, "secret", tools(), "test").unwrap();
    let port = server.port();
    let app = std::thread::spawn(move || loop {
        if let Some(call) = server.poll().into_iter().next() {
            std::thread::sleep(ANSWER_WITHIN * 2);
            call.reply(Ok(json!("done")));
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    });
    let call = json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "echo", "arguments": {}}});
    let (_, body) = post(port, &as_refs(&auth(port)), &call);
    app.join().unwrap();
    let result = &body.unwrap()["result"];
    assert_eq!(result["isError"], false, "{result}");
    assert_eq!(result["content"][0]["text"], "\"done\"");
}
