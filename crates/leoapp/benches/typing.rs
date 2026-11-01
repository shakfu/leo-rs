//! What typing into a long body costs, replayed in real time.
//!
//!     cargo bench -p leoapp --bench typing
//!
//! About 200 scripted keys, 80 ms apart, with 500 ms pauses, and a 16 ms frame
//! between, with the view following the edit. Each row reports the UI thread's
//! time per key, the colouring thread's CPU (Linux only), and how long colours
//! differed from a whole colouring of the same text: on screen, and anywhere in
//! the body. A last section reports resident memory (Linux only).

use std::fs;
use std::ops::Range;
use std::rc::Rc;
use std::time::{Duration, Instant};

use leoapp::highlight::{highlight, Colouring, Span};

const FRAME: Duration = Duration::from_millis(16);
const KEY_GAP: Duration = Duration::from_millis(80);
const PAUSE: Duration = Duration::from_millis(500);
const PAUSE_EVERY: usize = 20;
const ROWS: usize = 50;

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

/// The view after a key: `ROWS` lines around the first line it changed, or
/// `view` as it was if it changed none.
fn follow(view: Range<usize>, before: &[String], after: &[String]) -> Range<usize> {
    let Some(line) = (0..after.len()).find(|&i| before.get(i) != after.get(i)) else {
        return view;
    };
    let start = line
        .saturating_sub(ROWS / 2)
        .min(after.len().saturating_sub(ROWS));
    start..(start + ROWS).min(after.len())
}

fn replay(name: &str, body: &[String], language: &str, keyword: &str, quote: &str) {
    let states = script(body, keyword, quote);
    let truth: Vec<Vec<Vec<Span>>> = states.iter().map(|s| highlight(s, language)).collect();

    let mut colouring = Colouring::default();
    settle(&mut colouring, body, language, 0..ROWS);

    let cpu_before = worker_cpu();
    let mut per_key = Vec::with_capacity(states.len());
    let (mut wrong_body, mut wrong_screen) = (Duration::ZERO, Duration::ZERO);
    let mut view = 0..ROWS;
    let mut before = body;
    let begun = Instant::now();
    for (k, lines) in states.iter().enumerate() {
        let gap = match (k + 1) % PAUSE_EVERY {
            0 => PAUSE,
            _ => KEY_GAP,
        };
        view = follow(view, before, lines);
        before = lines;
        let judge = |spans: &[Vec<Span>]| {
            (
                spans == truth[k].as_slice(),
                spans[view.clone()] == truth[k][view.clone()],
            )
        };
        let start = Instant::now();
        let mut shown = colouring.of("n", lines, language, view.clone());
        per_key.push(start.elapsed());
        let (mut body_right, mut screen_right) = judge(&shown);
        let mut last = Instant::now();
        while start.elapsed() < gap {
            std::thread::sleep(FRAME);
            colouring.poll();
            let spans = colouring.of("n", lines, language, view.clone());
            let elapsed = last.elapsed();
            if !body_right {
                wrong_body += elapsed;
            }
            if !screen_right {
                wrong_screen += elapsed;
            }
            if !Rc::ptr_eq(&spans, &shown) {
                (body_right, screen_right) = judge(&spans);
                shown = spans;
            }
            last = Instant::now();
        }
    }
    let total = begun.elapsed();
    let last = states.last().unwrap();
    settle(&mut colouring, last, language, view.clone());
    let end = colouring.of("n", last, language, view);
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
        "{name:<16} {:>3} keys in {:>4.1} s  UI mean {:>7.2?}  max {:>7.2?}  worker CPU {cpu:>9}  wrong on screen {:>5.2} s, in body {:>5.2} s",
        states.len(),
        total.as_secs_f64(),
        mean,
        per_key[per_key.len() - 1],
        wrong_screen.as_secs_f64(),
        wrong_body.as_secs_f64(),
    );
}

fn settle(colouring: &mut Colouring, lines: &[String], language: &str, view: Range<usize>) {
    colouring.of("n", lines, language, view.clone());
    while colouring.due_in().is_some() {
        std::thread::sleep(Duration::from_millis(1));
        colouring.poll();
        colouring.of("n", lines, language, view.clone());
    }
}

/// Resident bytes, where the OS says.
fn resident() -> Option<usize> {
    let statm = fs::read_to_string("/proc/self/statm").ok()?;
    let pages: usize = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * 4096)
}

/// What one kept tree costs, from 20 held at once, and what `Colouring`'s
/// four kept bodies cost in all: their text, spans and trees.
fn memory(body: &[String]) {
    let mb = |bytes: usize| bytes as f64 / 1e6;
    let text = body.join("\n");
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .unwrap();
    let Some(before) = resident() else {
        println!("memory: n/a");
        return;
    };
    let trees: Vec<_> = (0..20)
        .map(|_| parser.parse(&text, None).unwrap())
        .collect();
    let after = resident().unwrap();
    println!(
        "one tree {:.2} MB, for {:.2} MB of text",
        mb(after.saturating_sub(before)) / 20.0,
        mb(text.len())
    );
    drop(trees);

    let before = resident().unwrap();
    let mut colouring = Colouring::default();
    for k in 0..4 {
        let lines: Vec<String> = body.iter().map(|l| format!("{l} ")).collect();
        let node = k.to_string();
        colouring.of(&node, &lines, "python", 0..ROWS);
        while colouring.due_in().is_some() {
            std::thread::sleep(Duration::from_millis(1));
            colouring.poll();
            colouring.of(&node, &lines, "python", 0..ROWS);
        }
    }
    println!(
        "four kept bodies {:.1} MB",
        mb(resident().unwrap().saturating_sub(before))
    );
}

fn main() {
    // `cargo test` runs a harness-less bench with no `--bench` flag.
    if !std::env::args().any(|a| a == "--bench") {
        return;
    }
    // First: a replay leaves free heap, which would hide what these allocate.
    memory(&python_body());
    replay("python, 5000", &python_body(), "python", "def", "\"\"\"");
    replay("rust, 5000", &rust_body(), "rust", "fn", "\"");
    replay("rust, real file", &real_body(), "rust", "fn", "\"");
}
