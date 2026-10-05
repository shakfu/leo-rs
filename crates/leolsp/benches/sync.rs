//! What keeping the servers in step costs as documents accumulate.
//!
//!     cargo bench -p leolsp
//!
//! An outline of 20 `@file` trees, each 300 nodes and about 6,300 lines, all
//! open with a stand-in server that answers `initialize` and nothing else,
//! so the time is the client's: rendering, mapping and sending.

use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Criterion};
use leolib::{Outline, Position};
use leolsp::server::{read_message, write_message, Server};
use leolsp::{Connect, Lsp, ServerConfig};
use serde_json::json;

const FILES: usize = 20;
const NODES: usize = 300;

/// The outline, and a leaf of each file.
fn outline() -> (Outline, Vec<Position>) {
    let mut o = Outline::new_empty();
    let top = o.root_position().unwrap();
    o.set_headline(&top, "project");
    o.set_body(&top, "@language python\n");
    let mut leaves = Vec::new();
    for f in 0..FILES {
        let file = o.insert_as_last_child(&top);
        o.set_headline(&file, &format!("@file f{f}.py"));
        o.set_body(&file, "import os\n@others\n");
        for n in 0..NODES {
            let node = o.insert_as_last_child(&file);
            o.set_headline(&node, &format!("f{f}_{n}"));
            let mut body = format!("def f{f}_{n}(x):\n");
            for k in 0..19 {
                body += &format!("    x = x + {k}  # line {k} of f{f}_{n}\n");
            }
            o.set_body(&node, &body);
            if n == NODES - 1 {
                leaves.push(node);
            }
        }
    }
    (o, leaves)
}

/// A server that answers `initialize` and reads everything else.
fn stand_in() -> Connect {
    Box::new(|_: &ServerConfig, root: &Path, wake| {
        let (from_client, to_server) = std::io::pipe()?;
        let (from_server, to_client) = std::io::pipe()?;
        std::thread::spawn(move || {
            let mut reader = BufReader::new(from_client);
            let mut writer = to_client;
            while let Ok(Some(msg)) = read_message(&mut reader) {
                if msg["method"] == "initialize" {
                    let reply =
                        json!({"jsonrpc": "2.0", "id": msg["id"], "result": {"capabilities": {}}});
                    let _ = write_message(&mut writer, &reply);
                }
            }
        });
        Ok(Server::connect(from_server, to_server, root, wake))
    })
}

fn sync(c: &mut Criterion) {
    let (mut o, leaves) = outline();
    let configs = vec![ServerConfig {
        language: "python".into(),
        command: "stand-in".into(),
    }];
    let mut lsp = Lsp::with_connect(configs, PathBuf::from("."), Arc::new(|| {}), stand_in());
    for leaf in &leaves {
        lsp.sync(&o, leaf);
    }
    // Let `initialize` be answered, so later messages are written, not queued.
    for _ in 0..100 {
        lsp.poll(&o);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }

    let mut group = c.benchmark_group("sync, 20 documents open");
    group.sample_size(20);
    let mut i = 0;
    group.bench_function("selection moves", |b| {
        b.iter(|| {
            i += 1;
            lsp.sync(&o, &leaves[i % 2])
        })
    });
    let leaf = leaves[0].clone();
    group.bench_function("one body edited", |b| {
        b.iter(|| {
            i += 1;
            let body = format!("def edited(x):\n    return {}\n", i % 2);
            o.set_body(&leaf, &body);
            lsp.sync(&o, &leaf)
        })
    });
    group.finish();
}

criterion_group!(benches, sync);
criterion_main!(benches);
