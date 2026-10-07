//! What drawing a long body costs, frame by frame.
//!
//!     cargo bench -p leoapp
//!
//! Sibling nodes, each a 5,000-line Python body, as an `@edit` node holding
//! a whole file would be.

use std::time::{Duration, Instant};

use criterion::{criterion_group, criterion_main, Criterion};
use leoapp::app::App;
use leoapp::view::Viewport;
use leolib::{Document, Position};

const LINES: usize = 5_000;
const SCREEN: Viewport = Viewport {
    rows: 50,
    cols: 120,
};

fn body(tag: &str) -> String {
    let mut s = String::new();
    for i in 0..LINES / 5 {
        s += &format!("def {tag}_{i}(x, y):\n");
        s += &format!("    \"\"\"Return {i} more.\"\"\"\n");
        s += &format!("    z = x + y * {i}  # a comment\n");
        s += "    return [z, 'text', 1.5]\n\n";
    }
    s
}

/// The app with `n` long bodies, the first selected, and their positions.
fn bodies(n: usize) -> (App, Vec<Position>) {
    let mut doc = Document::new_empty("");
    let mut at = doc.outline().root_position().unwrap();
    let mut nodes = Vec::new();
    for k in 0..n {
        if k > 0 {
            at = doc.outline_mut_untracked().insert_after(&at);
        }
        let tag = format!("n{k}");
        doc.set_headline(&at, &tag);
        doc.set_body(&at, &format!("@language python\n{}", body(&tag)));
        nodes.push(at.clone());
    }
    doc.clear_undo();
    let mut app = App::new(doc);
    app.select(nodes[0].clone());
    (app, nodes)
}

/// The app with two long bodies, the first selected.
fn app() -> (App, Position, Position) {
    let (app, nodes) = bodies(2);
    let [a, b] = <[Position; 2]>::try_from(nodes).unwrap();
    (app, a, b)
}

/// Until `poll` puts in the shown body's whole colouring, or has nothing
/// pending.
fn settle(app: &mut App) {
    while !app.poll() && app.poll_after().is_some() {
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// What a burst of selections costs, one frame each with `gap` between, as a
/// held arrow key or quick clicks through `BURST` long bodies: the UI
/// thread's time for the burst, and then how long until the last body is
/// coloured whole.
fn burst(gap: Duration) -> (Duration, Duration) {
    let (mut app, nodes) = bodies(BURST);
    let mut ui = Duration::ZERO;
    for (k, p) in nodes.iter().enumerate() {
        if k > 0 {
            std::thread::sleep(gap);
        }
        let start = Instant::now();
        app.select(p.clone());
        app.body_view(SCREEN);
        app.poll();
        ui += start.elapsed();
    }
    let start = Instant::now();
    settle(&mut app);
    (ui, start.elapsed())
}

const BURST: usize = 10;

fn bench(c: &mut Criterion) {
    // Each visit's whole colouring runs on a thread; it is let finish
    // outside the timing, as it would between visits.
    c.bench_function("first visit", |bench| {
        bench.iter_custom(|iters| {
            let mut total = Duration::ZERO;
            for _ in 0..iters {
                let (mut app, _, _) = app();
                app.colouring = Default::default();
                let start = Instant::now();
                app.body_view(SCREEN);
                total += start.elapsed();
                settle(&mut app);
            }
            total
        })
    });

    // Each body is shown until its whole colouring is in, as a user reading
    // it would see.
    let show = |app: &mut App| {
        app.body_view(SCREEN);
        settle(app);
    };
    let (mut app, a, b) = app();
    show(&mut app);
    c.bench_function("unchanged frame", |bench| {
        bench.iter(|| app.body_view(SCREEN))
    });

    app.select(b.clone());
    show(&mut app);
    c.bench_function("switch between two", |bench| {
        bench.iter(|| {
            app.select(a.clone());
            app.body_view(SCREEN);
            app.select(b.clone());
            app.body_view(SCREEN)
        })
    });

    // Each burst ends with the worker idle: the last job queued is the last
    // to finish.
    for (name, gap) in [
        ("no gap", Duration::ZERO),
        ("30 ms gap", Duration::from_millis(30)),
    ] {
        c.bench_function(&format!("burst of {BURST}, {name}: UI thread"), |bench| {
            bench.iter_custom(|iters| (0..iters).map(|_| burst(gap).0).sum())
        });
        c.bench_function(
            &format!("burst of {BURST}, {name}: last body whole after"),
            |bench| bench.iter_custom(|iters| (0..iters).map(|_| burst(gap).1).sum()),
        );
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(20);
    targets = bench
}
criterion_main!(benches);
