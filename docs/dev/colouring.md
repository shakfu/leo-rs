# Colouring long bodies: design comparison

Measured 2026-10-07 on an Apple M1 (8 cores: 4 performance, 4 efficiency), release build. Each body is 5,000 lines of Python. One whole colouring (`highlight::highlight`) takes 21 ms.

## Designs

1. **Blocking** (commit `ed1434f`). One cached colouring, keyed by a hash of every line. A new body is coloured whole on the UI thread. Past 500 lines, an edit recolours the changed lines and the whole body 300 ms later.

2. **Thread per job.** Four colourings cached by gnx. A first visit colours the visible lines; a new thread colours the whole body.

3. **One worker, 4 wanted.** As 2, but one shared worker thread runs the jobs in order. A job is skipped if its cache entry dropped it.

4. **Current.** As 3, but only the shown body's job is wanted. Hiding a body marks its job unwanted: the worker skips it if not begun, and a begun job's result is kept for when the body is shown again.

## Results

`cargo bench -p leoapp --bench body`. A burst selects 10 long bodies, one frame (`body_view` and `poll`) each.

| | Blocking | Thread per job | Worker, 4 wanted | Current |
|-|-|-|-|-|
| First visit, UI thread | 21.0 ms | 1.40 ms | 1.35 ms | 1.29 ms |
| Unchanged frame | 0.29 ms | 0.25 ms | 0.25 ms | 0.25 ms |
| Switch between two | 42.6 ms | 0.80 ms | 0.79 ms | 0.79 ms |
| Burst, no gap: UI thread | 255 ms | 24.8 ms | 15.0 ms | 14.8 ms |
| Burst, no gap: last body whole after | n/a | 38.7 ms | 92.5 ms | 30.4 ms |
| Burst, 30 ms gap: UI thread | 294 ms | 16.9 ms | 16.8 ms | 15.9 ms |
| Burst, 30 ms gap: last body whole after | n/a | 22.7 ms | 21.4 ms | 21.3 ms |

- Any background design cuts the UI thread's time for a burst from 255-294 ms to about 15 ms.

- Thread per job: 10 concurrent threads add 10 ms of UI-thread time to a no-gap burst, and the last body is whole 8 ms later than with the current design.

- Worker, 4 wanted: the shown body waits behind earlier bodies' jobs, 92.5 ms.

- Current: the shown body waits for at most one begun job and its own, 30.4 ms.

- With a 30 ms gap each job ends before the next selection, so designs 2-4 are within 1.5 ms of each other.

Blocking has no "whole after" figure. Its one-entry cache colour-patches a new body from the previous body's lines, which colours nearly all of it on the selection frame. A redundant whole recolour then runs on the UI thread 300 ms later. The bench's 277 ms is that timer, not a colouring.

Limits: single runs, one machine, one grammar, one body shape. Thread per job depends on the core count. Each selection gets exactly one frame.

## Incremental parsing

`tree-sitter-highlight` parses from scratch on every call: it passes no old tree to `parse_with_options` (`tree-sitter-highlight-0.25.10/src/lib.rs:530`). Measured with `tree-sitter` directly, on the same body:

| Step | Time |
|-|-|
| Whole highlight, as now | 21 ms |
| Fresh parse | 12 ms |
| Highlight query, whole tree | 6.7 ms |
| Highlight query, 50 visible lines | 0.07 ms |
| Re-parse after one character typed in a name or comment | 0.25 ms |
| Re-parse after an unclosed quote | 6.7 ms |
| Re-parse after `def` becomes `qdef` | 12 ms |

The `qdef` edit's changed ranges cover all 4,999 lines below it: error recovery re-parses everything.

### Decision: not now

A Helix-style design keeps a tree per body, applies each change with `Tree::edit`, and queries the visible lines. Two places to re-parse:

- **On the UI thread.** A keystroke costs 0.3 ms when the edit keeps the syntax valid, and 6.7-12 ms when it breaks it. Typing breaks the syntax often. `patch` costs 1.8 ms a key (measured on another machine, not re-run). A first visit still needs the 12 ms parse, so the visible-first window and the worker stay. Not a clear win, for a large rewrite: injections, `@language` regions and `plan`'s masking move to the lower-level API.

- **On the worker.** The UI thread queries the visible lines against the latest tree: about 0.07 ms a frame. The worker re-parses in 0.25 ms after a typical pause, 12 ms at worst, against 21 ms for a whole recolour now. Colours follow a whole-document tree, so an unclosed string no longer shows wrong colours beyond `patch`'s 20-line window for about 320 ms. The cost: the UI thread may query a tree one edit behind the text, so ranges must be shifted by the pending edits; plus the rewrite above.

The worker variant is the one that could pay. It is deferred because no measured problem remains for it to solve: a first frame takes 1.3 ms, a key about 1.8 ms, and the whole colouring is off the UI thread. `TODO.md` keeps it under Low.

Reasons to revisit:

- A profile of continuous typing in a long body shows the 21 ms recolours cost real CPU, for example on battery.

- Users notice wrong colours far from an edit, such as below an unclosed string.

- A grammar or body shape not measured here behaves worse. Every number above is Python, one body shape, and one edit rather than a typing sequence.

The cheapest deciding measurement: replay a recorded typing session into a long body, and compare the worker's total CPU now with an estimate for the worker variant.

## Reproducing

Designs 2 and 3 were never committed. To compare, copy the tree once per design, swap the `Colouring` section of `crates/leoapp/src/highlight.rs`, and give each copy its own `CARGO_TARGET_DIR`. Cargo names a workspace build by its path relative to the workspace root, so copies sharing a target directory reuse one build when their sources are older than it (`rsync -a` keeps modification times).
