# Incremental colouring: design

Design only. Source: the `TODO.md` item "Incremental colouring, as Helix does it" (Medium, leoapp). Background and measurements: `docs/dev/colouring.md`.

Terms:

- **Incremental parse**: `Parser::parse(text, Some(&old_tree))`, where `old_tree` has had `Tree::edit` applied. tree-sitter reuses the unchanged subtrees ([Parser::parse](https://docs.rs/tree-sitter/0.25.10/tree_sitter/struct.Parser.html#method.parse), [Tree::edit](https://docs.rs/tree-sitter/0.25.10/tree_sitter/struct.Tree.html#method.edit)).
- **`InputEdit`**: one change as byte offsets plus row/column points: start, old end, new end ([InputEdit](https://docs.rs/tree-sitter/0.25.10/tree_sitter/struct.InputEdit.html)).
- **Range query**: a highlight query limited by `QueryCursor::set_byte_range` to nodes that intersect a byte range ([set_byte_range](https://docs.rs/tree-sitter/0.25.10/tree_sitter/struct.QueryCursor.html#method.set_byte_range)).
- **Changed ranges**: `Tree::changed_ranges(old, new)`, the ranges whose syntactic structure differs between two trees ([changed_ranges](https://docs.rs/tree-sitter/0.25.10/tree_sitter/struct.Tree.html#method.changed_ranges)).

## 1. Problem

`tree-sitter-highlight` parses from scratch on every call. It passes no old tree to `parse_with_options` (`tree-sitter-highlight-0.25.10/src/lib.rs:530`). It also queries the whole tree. So the worker colours a long body whole after every typing pause.

Measured 2026-10-07, Apple M1, release build, one 5,000-line Python body:

| Step | Time |
|-|-|
| Whole `highlight::highlight` | 21 ms |
| Fresh parse | 12 ms |
| Highlight query, whole tree | 6.7 ms |
| Highlight query, 50 visible lines | 0.07 ms |
| Incremental re-parse, one character in a name or comment | 0.25 ms |
| Incremental re-parse, unclosed quote | 6.7 ms |
| Incremental re-parse, `def` to `qdef` | 12 ms |
| `patch`, one key (other machine, not re-run) | 1.8 ms |
| First visit, UI thread (`highlight_window`) | 1.29 ms |

Derived, not measured: the 21 ms minus parse and query leaves 2.3 ms for joining lines, walking events and `push_run`.

What the current design costs the user:

1. CPU: one 21 ms worker job per typing pause, whatever the edit size.
2. Wrong colours beyond `patch`'s window. An unclosed string colours only `CONTEXT` (20) lines either side. The rest is wrong until `SETTLE` (300 ms) plus the 21 ms job: about 320 ms.

No measurement shows either costs a user anything yet. `colouring.md` ("Decision: not now") says so. That decision still holds unless step 0 below says otherwise.

## 2. Current design

- `Colouring::of` (`crates/leoapp/src/highlight.rs`) is called once per frame from `App::body_view` (`crates/leoapp/src/view.rs:387`). Both leotui and leogui reach it through `body_view`.
- The cache key is the body's full `Vec<String>` and language, compared every frame. `KEEP` = 4 bodies, keyed by gnx.
- Under `WHOLE_UNDER` (500) lines, `highlight` runs on the UI thread on every change.
- Over 500 lines, a first visit colours `visible` plus `CONTEXT` lines (`highlight_window`). An edit runs `patch`: a line-level prefix/suffix diff, recolouring the changed lines plus `CONTEXT` either side.
- After `SETTLE`, `start_if_due` sends a `Job` to one shared `worker` thread. `App::poll` (`crates/leoapp/src/app/lsp.rs:61`) calls `Colouring::poll`, which collects the result. Only the shown body's job is `WANTED`.
- `plan` claims directives and section references, and blanks those lines to spaces of equal byte width. It cuts the body into `Region`s at `@language`, `@nocolor`, `@color`, `@killcolor`.
- Each region is parsed on its own: `treesit::highlight` on `masked[region]`, or the line scanner `scan` when no grammar exists.
- Injections: none. `treesit::configure` passes `""` as the injection query and `highlight` passes `|_| None`. HTML's `<script>` is not coloured as JavaScript today.
- Body text reaches `Colouring` as whole lines. The editor (`crates/leoapp/src/editor/`) returns a new `Vec<String>` per `Change`. No byte-level edit exists anywhere. Undo, LSP edits, MCP and file reloads change bodies through `Document::set_body` too.

## 3. How Helix does it

From [`helix-core/src/syntax.rs`](https://github.com/helix-editor/helix/blob/master/helix-core/src/syntax.rs), [`helix-view/src/document.rs`](https://github.com/helix-editor/helix/blob/master/helix-view/src/document.rs) and tree-house's [`highlighter/src/parse.rs`](https://github.com/helix-editor/tree-house/blob/master/highlighter/src/parse.rs). Read through fetched summaries, not line by line.

- `Document::apply_impl` calls `Syntax::update` synchronously, on the main thread, for every transaction.
- `generate_edits` turns a `ChangeSet` into `InputEdit`s. It fills byte offsets only; all points are `Point::ZERO`.
- tree-house applies the edits in reverse order with `Tree::edit` per layer. It re-parses with the old tree via `parse_with_timeout`. `PARSE_TIMEOUT` is 500 ms; on error Helix sets `syntax = None` for that buffer.
- Each injection is a layer parsed with `set_included_ranges`.
- Highlighting takes a byte range, so only the viewport is queried.

Helix has the edits for free: its `ChangeSet` is the edit. leo-rs does not.

## 4. Options

All of (a)-(c) drop `tree-sitter-highlight`: it accepts no old tree and no byte range. They need a query engine on `tree_sitter::QueryCursor` that reproduces its capture precedence. For one node, a later pattern replaces an earlier one (`tree-sitter-highlight-0.25.10/src/lib.rs:1036`). `treesit.rs`'s appended patterns (`PYTHON_BUILTIN_TYPES`, `RUST_NUMBERS`) rely on that. Injection support would have to be written by hand later.

All of (a)-(c) need an `InputEdit` per region. Derive it from text, not from the editor: diff the old and new masked text of each region, byte prefix and suffix, backed off to a char boundary. This covers every `Document::set_body` caller, undo and masking changes. A line turning into `@others` is then an ordinary edit. Cost: one `memcmp` pass over the region, about 150 KB here; inferred to be well under 0.1 ms. A multi-site change (`:s///g`) yields one wide edit: correct, but less reuse.

### (a) Full incremental on the UI thread, as the TODO sketches

- Changes: `Entry` keeps one `Tree` per region. `of` derives the edit, calls `Tree::edit`, re-parses, and range-queries `visible`. Removes `patch`, `SETTLE`, the worker and `Pending`.
- Cost per key, UI thread: 0.25 + 0.07 = 0.3 ms typical; 6.8 ms unclosed quote; 12.1 ms `qdef`. Versus 1.8 ms now. Sum inferred from the table.
- First visit: 12 ms fresh parse on the UI thread, versus 1.29 ms now. Avoiding that keeps the worker, so (a) removes less than claimed.
- Complexity: high. Rewrites the engine, the cache and the timing model.
- Risk: frame drops. 12 ms is 72% of a 16.7 ms frame at 60 Hz. Typing breaks syntax often (`colouring.md`). Helix accepts this on its main thread with a 500 ms cap.
- Regions: one tree per region. Adding or removing a directive changes the region list: parse those regions fresh.

### (b) Incremental parse on the worker, range query on the UI thread

- Changes: the worker keeps the trees and re-parses per job. The UI thread clones the latest tree (`ts_tree_copy`, a reference count), applies `Tree::edit` for edits since that tree, and range-queries `visible`. Lines changed since the tree are coloured by `patch`'s window.
- Cost per frame, UI thread: 0.07 ms query plus clone and edit (unmeasured, inferred small) plus `patch` on a changed frame (1.8 ms). Worker: 0.25-12 ms per job, versus 21 ms.
- Colours below an unclosed string are right after one job: at most 12 ms after it starts, not 320 ms.
- Complexity: highest. Two threads share trees (`Tree` is `Send + Sync`, `tree-sitter-0.25.10/binding_rust/lib.rs:3933`). Version tracking. A stale tree queried against new text mis-places captures inside the edit; `patch` must overwrite them.
- Risk: colour glitches near edits from tree/text skew. Hard to test deterministically.
- Regions: as (a), but region-list changes must also reach the UI-side clone.

### (c) Whole-body output, incremental parse on the worker

- Changes: the worker re-parses each region with its previous tree. It still returns spans for every line. `Colouring::of`, `patch`, `plan` and the UI thread are unchanged. `Entry` owns `Arc<Mutex<Trees>>`; each `Job` carries a clone; trees drop with the entry. Only the worker locks it.
- Cost per job: parse 0.25 / 6.7 / 12 ms, plus 6.7 ms query, plus 2.3 ms output: 9.3 / 15.7 / 21 ms. Inferred by addition.
- (c+): re-query only the edited bytes plus changed ranges, line-aligned, and splice into the previous spans. Typical job: about 0.3-0.5 ms (inferred). An unclosed quote changes everything below: back to about 15 ms.
- Once a job costs under 1 ms, `SETTLE` can drop from 300 ms to one frame. Then far colours are right within one job of each change.
- Complexity: medium. New engine plus a tree cache. No change to the UI thread or the timing model.
- Risk: incremental and fresh parses can differ after error recovery (inferred; tree-sitter does not promise identical trees). A property test catches it.
- Regions: key trees by region index and language. If the region list differs in count or language, parse all regions fresh.

### (d) Do nothing

- Changes: none. Range-limiting alone is not available: `tree-sitter-highlight` has no byte-range API.
- Cost: as now. 1.8 ms per key on the UI thread; 21 ms per pause on the worker.
- Variant (d'): cut `SETTLE` from 300 ms to, say, 50 ms. One constant. Far colours are right about 70 ms after a pause, for up to 6 x the worker CPU while typing. This targets wrong colours, not CPU.

### Summary

| | UI thread per key | Worker per job | Far colours right after | Removes |
|-|-|-|-|-|
| now | 1.8 ms | 21 ms | ~320 ms | - |
| (a) | 0.3-12.1 ms | none (or first visit) | same frame | `patch`, `SETTLE` |
| (b) | ~1.9 ms | 0.25-12 ms | <= 12 ms after job start | 21 ms jobs |
| (c) | 1.8 ms | 9.3-21 ms | ~320 ms | 12 ms parse |
| (c+) + short `SETTLE` | 1.8 ms | ~0.4-15 ms | one frame + job | most worker CPU |
| (d') | 1.8 ms | 21 ms | ~70 ms | nothing |

All rows except "now" are inferred from the table in section 1.

## 5. Recommendation

Do step 0 first. If it shows no cost, stop: (d).

Otherwise build (c), then (c+), then shorten `SETTLE`. Defer (b). Reject (a).

Reasons:

- (c) keeps the UI thread at its measured 1.29 ms first visit and 1.8 ms key. (a) raises the worst key to 12 ms.
- (c+) with a short `SETTLE` fixes both costs in section 1. It gets most of (b)'s benefit without trees shared across threads.
- Every step lands alone. Each keeps the existing `Colouring` tests as written.
- (b) adds tree/text skew handling to save the 1.8 ms `patch`. No measurement says 1.8 ms is a problem.

Rejected framing: "port Helix's model", where the editor emits `InputEdit`s and the tree follows them. leo-rs has no edit stream. Bodies change through `Change`, undo, `set_body` from MCP and LSP, and file reloads. Plumbing byte edits through all of them is large and brittle. Diffing at the colouring cache costs one pass over text it already compares every frame. Cost: a multi-site change becomes one wide edit.

Second framing to weigh: this is a latency problem, not a CPU problem. If so, (d') answers it with one constant, and step 0 should measure perceived wrong-colour time, not CPU.

## 6. Implementation plan

Each step lands separately. `make test` passes after each. Kept throughout: the `Colouring` tests in `highlight.rs` (`an_edit_to_a_long_body_recolours_near_it_as_the_whole_would` through `only_the_shown_body_can_be_due`), the engine tests (`python_comments_strings_and_keywords` through `the_scanners_one_builtin_list_lands_on_builtin_functions`), `a_long_body_is_coloured_whole_by_a_later_poll` in `app/tests.rs`, and the `poll` tests in `app/lsp.rs`.

0. **Measure.** Add a bench to `crates/leoapp/benches/body.rs`: replay a recorded typing sequence (about 200 keys, with pauses) into the 5,000-line body. Report total worker CPU, UI time per key, and time with wrong far colours. Add a second grammar (Rust or C) and one `@edit`-sized real file. No code outside `benches/`.

1. **Own query engine.** Replace `tree_sitter_highlight` in `treesit.rs` with `Parser` plus `QueryCursor::captures`, same `CAPTURES` table, same precedence. Still a fresh parse. Tests: a temporary equivalence test against `tree-sitter-highlight` over every grammar's sample bodies and the existing test bodies; delete it with the dependency. Bench: whole `highlight` must stay at or under 21 ms.

2. **Edit derivation.** A pure function: old text, new text to `InputEdit`, char-boundary safe, with points. Tests: unit tests for insert, delete, replace, multi-byte, empty, identical. A fuzz target under `fuzz/` checks that applying the edit to old text gives new text.

3. **Trees on the worker, (c).** `Entry` owns `Arc<Mutex<Trees>>`. `Job` carries it. The worker diffs each region, edits, re-parses with the old tree. Tests: a property test that incremental colouring equals fresh `highlight` over random edits to sample bodies, including unclosed quotes and `def`/`qdef`. A test that adding `@language` mid-body drops the trees. Bench: worker job after one key.

4. **Changed-range re-query, (c+).** The worker re-queries edited bytes plus `changed_ranges`, line-aligned, and splices. Tests: the step 3 property test, now on spliced output. Bench: job time for the three edit kinds in section 1.

5. **Cancel superseded parses.** Use `parse_with_options`'s progress callback to stop when the job turns `UNWANTED`. Keeps the old tree. Test: a superseded job returns no spans and the next job still re-parses incrementally.

6. **Shorten `SETTLE`.** Set it from step 0's numbers. Tests: existing timing tests use `SETTLE` by name; check none assumes 300 ms. Bench: the step 0 replay, before and after.

7. **Optional, (b).** Only if step 6's replay still shows a UI-thread cost from `patch`. Needs its own design note.

Then update `docs/dev/colouring.md` and the `TODO.md` item. Add a CHANGELOG entry per user-visible step (6; 1 only if colours change).

## 7. Open questions

1. Is there a user-visible problem today? `colouring.md` says no; `TODO.md` lists this as Medium, `colouring.md` as Low. Which is current?
2. Which matters more: worker CPU (battery) or wrong far colours for 320 ms? That picks (c+) versus (d').
3. If the step 3 property test finds incremental and fresh parses that differ, accept the difference or fall back to a fresh parse for that job?
4. Memory: up to 4 bodies' trees stay resident. Is that acceptable unmeasured, or measure first?
5. Dropping `tree-sitter-highlight` makes injections (HTML `<script>`, Markdown fences) a hand-written feature. Are injections wanted at all?
6. Should step 0 cover grammars other than Python? Every number here is one grammar, one body shape and single edits.
