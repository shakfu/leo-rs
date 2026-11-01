//! What typing into a long body costs, replayed in real time.
//!
//!     cargo bench -p leoapp --bench typing
//!
//! About 200 scripted keys, 80 ms apart, with 500 ms pauses, and a 16 ms frame
//! between. Each row reports the UI thread's time per key, the colouring
//! thread's CPU (Linux only), and how long the colours on screen differed from
//! a whole colouring of the same text.

use std::fs;
use std::rc::Rc;
use std::time::{Duration, Instant};

use leoapp::highlight::{highlight, Colouring, Span};

const FRAME: Duration = Duration::from_millis(16);
const KEY_GAP: Duration = Duration::from_millis(80);
const PAUSE: Duration = Duration::from_millis(500);
const PAUSE_EVERY: usize = 20;
const VISIBLE: std::ops::Range<usize> = 0..50;

fn python_body() -> Vec<String> {
    let mut lines = Vec::new();
    for i in 0..1000 {
        lines.push(format!("def f_{i}(x, y):"));
        lines.push(format!("    \"\"\"Return {i} more.\"\"\""));
        lines.push(format!("    z = x + y * {i}  # a comment"));
        lines.push("    return [z, 'text', 1.5]".to_string());
        lines.push(String::new());
    }
    lines
}

fn rust_body() -> Vec<String> {
    let mut lines = Vec::new();
    for i in 0..1000 {
        lines.push(format!("fn f_{i}(x: u32, y: u32) -> u32 {{"));
        lines.push(format!("    // Return {i} more."));
        lines.push(format!("    let z = x + y * {i};"));
        lines.push("    z + 1 }".to_string());
        lines.push(String::new());
    }
    lines
}

/// This crate's own colouring module: a real file, as an `@edit` node holds.
fn real_body() -> Vec<String> {
    include_str!("../src/highlight.rs")
        .lines()
        .map(str::to_string)
        .collect()
}

/// Every text the script passes through, one per key.
fn script(body: &[String], keyword: &str, quote: &str) -> Vec<Vec<String>> {
    let mut states = Vec::new();
    let mut now = body.to_vec();
    let mid = body.len() / 2;
    let def = (mid..body.len())
        .find(|&i| body[i].starts_with(keyword))
        .expect("the body holds the keyword");

    let typed = |now: &mut Vec<String>, states: &mut Vec<_>, line: usize, text: &str| {
        for c in text.chars() {
            now[line].push(c);
            states.push(now.clone());
        }
    };
    let erased = |now: &mut Vec<String>, states: &mut Vec<_>, line: usize, n: usize| {
        for _ in 0..n {
            now[line].pop();
            states.push(now.clone());
        }
    };

    // Inside a name or a comment: the syntax stays valid.
    typed(&mut now, &mut states, mid, "  // and some words of note");
    erased(&mut now, &mut states, mid, 27);
    // A new line, opening and closing a string.
    now.insert(mid + 1, String::new());
    states.push(now.clone());
    typed(
        &mut now,
        &mut states,
        mid + 1,
        "value = compute(12, 'abc', 3)",
    );
    erased(&mut now, &mut states, mid + 1, 29);
    now.remove(mid + 1);
    states.push(now.clone());
    // A quote left open: everything below it turns to string.
    typed(&mut now, &mut states, mid, quote);
    typed(&mut now, &mut states, mid, " text in the string");
    erased(&mut now, &mut states, mid, 19 + quote.chars().count());
    // A keyword broken and mended, which re-parses all below it.
    for _ in 0..10 {
        now[def].insert(0, 'q');
        states.push(now.clone());
        now[def].remove(0);
        states.push(now.clone());
    }
    typed(&mut now, &mut states, mid, " // done");
    erased(&mut now, &mut states, mid, 8);
    states
}

/// On-CPU nanoseconds of the colouring thread, where the OS says.
fn worker_cpu() -> Option<u64> {
    for task in fs::read_dir("/proc/self/task").ok()?.flatten() {
        let comm = fs::read_to_string(task.path().join("comm")).ok()?;
        if comm.trim() == "colouring" {
            let stat = fs::read_to_string(task.path().join("schedstat")).ok()?;
            return stat.split_whitespace().next()?.parse().ok();
        }
    }
    Some(0)
}

fn replay(name: &str, body: &[String], language: &str, keyword: &str, quote: &str) {
    let states = script(body, keyword, quote);
    let truth: Vec<Vec<Vec<Span>>> = states.iter().map(|s| highlight(s, language)).collect();

    let mut colouring = Colouring::default();
    colouring.of("n", body, language, VISIBLE);
    settle(&mut colouring, body, language);

    let cpu_before = worker_cpu();
    let mut per_key = Vec::with_capacity(states.len());
    let mut wrong = Duration::ZERO;
    for (k, lines) in states.iter().enumerate() {
        let gap = match (k + 1) % PAUSE_EVERY {
            0 => PAUSE,
            _ => KEY_GAP,
        };
        let start = Instant::now();
        let mut shown = colouring.of("n", lines, language, VISIBLE);
        per_key.push(start.elapsed());
        let mut right = *shown == truth[k];
        let mut last = Instant::now();
        while start.elapsed() < gap {
            std::thread::sleep(FRAME);
            colouring.poll();
            let spans = colouring.of("n", lines, language, VISIBLE);
            if !Rc::ptr_eq(&spans, &shown) {
                right = *spans == truth[k];
                shown = spans;
            }
            if !right {
                wrong += last.elapsed();
            }
            last = Instant::now();
        }
    }
    let last = states.last().unwrap();
    settle(&mut colouring, last, language);
    let end = colouring.of("n", last, language, VISIBLE);
    assert_eq!(
        *end,
        *truth.last().unwrap(),
        "{name}: the colours settled wrong"
    );

    per_key.sort();
    let mean = per_key.iter().sum::<Duration>() / per_key.len() as u32;
    let cpu = match (cpu_before, worker_cpu()) {
        (Some(a), Some(b)) => format!("{:.1} ms", (b - a) as f64 / 1e6),
        _ => "n/a".to_string(),
    };
    println!(
        "{name:<18} {:>4} keys  UI mean {:>7.2?}  max {:>7.2?}  worker CPU {cpu:>9}  wrong {:>7.0?}",
        states.len(),
        mean,
        per_key[per_key.len() - 1],
        wrong,
    );
}

fn settle(colouring: &mut Colouring, lines: &[String], language: &str) {
    while colouring.due_in().is_some() {
        std::thread::sleep(Duration::from_millis(1));
        colouring.poll();
        colouring.of("n", lines, language, VISIBLE);
    }
}

fn main() {
    // `cargo test` runs a harness-less bench with no `--bench` flag.
    if !std::env::args().any(|a| a == "--bench") {
        return;
    }
    replay("python, 5000", &python_body(), "python", "def", "\"\"\"");
    replay("rust, 5000", &rust_body(), "rust", "fn", "\"");
    replay("rust, real file", &real_body(), "rust", "fn", "\"");
}
