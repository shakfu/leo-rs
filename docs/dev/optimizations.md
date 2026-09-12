# Load-time optimizations

Measured on 2026-09-10 on macOS, release build, against leo-editor `3acfadd8d0`'s `leo/core/LeoPyRef.leo`: 362 `@file`, 12 `@clean` and 7 `@edit` nodes. Whole-process times are `hyperfine`, warm, from process start to exit.

Compiling the sentinel regexes once per delimiter pair took the load from 779ms to 99ms; see the CHANGELOG. This file covers what is left.

## Where the load goes

`sample` over 40 loads in one process: 95ms per load, 1,537 samples.

| cost | share |
|-|-|
| regex matching | 13% |
| regex cache pools (`Pool::get_slow`) | 8.5% |
| reading files (`read_file_to_string`) | 13%, of which `fs::read` is 9% |
| `util::split_lines` | 9% |
| hashing `String` keys with SipHash | 9% |

Shares are exclusive samples, except `Pool::get_slow`, which is inclusive.

## 1. Share the compiled patterns

The cache cloned each `Regex` out, and a clone gets a new, empty cache pool (`regex-automata` 0.4.18, `src/meta/regex.rs:1916`). Every file therefore rebuilt each regex's search cache on its first match. The cache now hands out an `Arc<Patterns>`, so all files share one set of pools. `@section-delims` still changes `ref_pat` on a private copy, through `Arc::make_mut`.

Result: 99.0ms to 87.9ms whole-process, 11%; 15 runs each, σ 0.5ms.

## 2. Match sentinels by hand

Not done. `Scanner::scan_lines` rejects a line that does not start with the sentinel prefix, so the regexes run only on sentinel lines. There they are tried in turn, up to 12 per line. Branching on the word after `@` (`+node:`, `+others`, `@c`, ...) would replace most of them, for up to 13% of the load.

The risk is parity. `\s` and `\b` are Unicode-aware in both Python's `re` and the `regex` crate, and a hand matcher must behave the same. The corpus tangle test would catch a misread file. Worth doing if load speed becomes a goal.

## 3. Borrow lines instead of allocating them

Not done, and the saving is an estimate. `util::split_lines` returns a `Vec<String>`, one allocation per line of every file, and `read_into_root` scans that. Scanning `&str` slices of the file's text would remove those allocations. `split_lines` is 9% of the load; `strip_indent` and string joins allocate more.

## Not pursued: tree-sitter

- **For reading `@file`.** Sentinels are comments, so a parse of the host language does not find them any faster. tree-sitter-python took 562ms to parse leo-editor's 293 Python files (6.9MB). A line scan of the same text took 6.5ms, and the whole load takes 99ms. The reader also works for any language with comment delimiters, where leotui has 12 grammars, and leolib has no tree-sitter dependency.

- **For the `@auto` importers.** About 7 of the 23 importer languages have a grammar here. A tree-sitter importer would split files differently from Leo's, so an outline would differ from leo-editor's for the same file. `LeoPyRef.leo` has no `@auto` nodes, so importers are not in this load at all. The case for it is import quality, not speed.

# Traversal costs

Measured on 2026-09-12, release build, on `leo/core/LeoPyRef.leo` with its external files read: 11,598 positions.

## Finding the next marked node

`Outline::scan_from`, which `next_marked`, `prev_marked` and `next_clone` are written on, built `all_positions()` and searched it. That is 665us per call whatever the answer, and it runs per keystroke.

It now steps with `thread_next`/`thread_back` and stops when it returns to where it started: 27ns when the match is a row away, 304us when there is no match at all and the walk covers the outline. A position whose ancestors no longer link is rejected first, since the walk would never come back to it.

## Cost is quadratic in depth

`Position` carries its ancestor stack, so `self_and_parents` allocates depth positions of depth entries each. `Outline::get_path` calls it per node, `full_path` calls `get_path`, and `find_files_to_read` calls `full_path` per node, as Leo's `findFilesToRead` does.

A single chain of N nodes, opened and written through `examples/writecheck`:

| depth | time |
|-|-|
| 500 | 0.06s |
| 1,000 | 0.64s |
| 2,000 | 7.05s |
| 20,000 | over 300s, killed |

Reading alone is milliseconds at 20,000, so the cost is in the path scanners, not the reader.

Not done, and not a problem in practice: real outlines are shallow. `LeoPyRef.leo` is 11,581 nodes at depth 10 and loads in 99ms. Memoizing `get_path` per vnode for the length of one read is the smaller fix; it needs interior mutability or a `&mut self` scan. Skipping the path for nodes whose headline is not a directive is cheaper still, but it changes which clone subtrees are walked twice, and with them the contents of `ReadResult::ignored`.
