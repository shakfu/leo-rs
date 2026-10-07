# Literate markdown with entangled

Revised 2026-10-07; replaces the sketch of 2026-09-12. Nothing here is scheduled. A line says when something was measured or run; the rest is judgement.

## Goal

Code examples in markdown go stale when nothing tests them against the API. Edit such a document in Leo as an outline whose code fences are nodes, and have the examples written to files a test runner runs. [entangled](https://github.com/shakfu/entangled-rs) already defines the fence syntax and the tangling; leo-rs adds the outline.

This is a third way for leo-rs to hold markdown, beside `@auto-md` (structure only) and `@edit` (flat text).

## Decisions

- **The markdown must render on GitHub.** A fence always holds its code. No design may leave fences empty in the file.
- **Full entangled compatibility.** A document tangles the same under the `entangled` CLI and under leo-rs, including references, continuation blocks, namespaces and annotations.
- **Python first**, for the test harness.

## What was checked

### entangled-rs

- Published as `entangled-rs` 0.2.1; the library target is `entangled`. The crate named `entangled` on crates.io is unrelated.
- `ParsedDocument` (`readers/markdown.rs:18`) holds code blocks, frontmatter and a path: no headings, no prose, no order beyond `IndexMap` insertion. leo-rs must parse the document's structure itself.
- `stitch_files` (`interface/document.rs:320`) splices line ranges into the markdown it read. It skips any block containing a `<<reference>>`: only leaf blocks are stitched.
- `ReferenceId{name, count}` counts same-named blocks by position, so it cannot serve as a gnx.
- An unknown fence attribute is parsed as a generic `key=value` and only looked up by name (`model/properties/mod.rs:231`), so entangled ignores `include=`. Read from the code, not run.
- References may cross documents: `<<other.md#name>>` (0.2.1).
- Dependencies: 94 crates as published. `tokio`, `notify` and `serde_yaml` were unused, and weave's HTML backend now sits behind an `html` feature. With those changes and `default-features = false`, 48 crates, 30 of them new to `leoapp`. The changes are in the local checkout, unreleased; removing `EntangledError::YamlParse` makes the release 0.3.0.

### leo-rs

- The markdown `@auto` writer regenerates the file (`write_markdown`, `importers/lines.rs:347`): `#` headings whatever the source used, `!Declarations` and placeholders dropped. `import_string` skips the round-trip check for line importers (`importers.rs:504`).
- `@auto` imports match Leo's on 998 of 1,000 files (README, Status). Changing `@auto-md`'s parser would break that, and give a different tree from Python Leo for the same file.
- `@persistence` is not ported (`delta.md:477`), so a clone inside an `@auto` tree does not survive reopening.

### A spike with `@clean` and clones

Run 2026-10-07 with leotui, no code changes. `@clean README.md` held prose and a fence whose body was `<< example add >>`; the same node, cloned, sat inside a test function under `@file tests/test_readme.py`.

- Both files were written correctly, the example indented inside the test function, and the test passed under Python.
- An external edit to the example in `README.md` reached the node when only `README.md` was present.
- With both files present, the `@file` read came second and its stale copy overwrote the edit, with no report. leolib has no check for a clone whose body differs between files.

The structure works, but it needs clones and the `.leo` file to carry it. Fence attributes carry it in the markdown itself, which is the design below.

## Design

### A new kind, `@auto-lit`

Name provisional. It shares `@auto-md`'s line scanner and adds fences; `@auto-md` stays as Leo has it.

- A heading is a node, nested by level, as now.
- Under a heading, its children follow document order: each fenced block, and each prose run between fences.
- A fence node's body is the code alone. The info string and fence lines are kept verbatim, so the file can be written back exactly.
- The writer concatenates every node's lines in document order. An unedited node writes the lines it was read from, so a file read and written unedited is byte-identical, and the corpus checks that.

gnx is allocated at parse time and is not durable, as for any `@auto` tree.

### Fence semantics

entangled's, plus one extension that entangled ignores.

| fence | on read | on save |
|-|-|-|
| ```` ```python #add ```` | a node headlined `add` | the markdown |
| ```` ```python #main file=hello.py ```` | a node; a report if `hello.py` changed on disk since leo-rs last wrote it | the markdown, and `hello.py` as entangled tangles it |
| ```` ```python #main include=hello.py ```` | a node filled from `hello.py` | the markdown with the code in it; `hello.py` if the node was edited |
| several ```` #add ```` blocks | sibling nodes with one headline | each in place; entangled concatenates them |

- `file=` is entangled's tangle: `<<references>>` expanded, the project-wide namespace, annotations and `entangled.toml` all apply. leo-rs calls entangled for it rather than reimplementing it. leolib's write guards still apply: no overwriting a file never read, or one changed on disk.
- `include=` is the read direction. The fence keeps the code, so GitHub shows it. When both the node and the file changed since the last sync, report a conflict rather than pick one; leolib's file stamps already record the last read or write.
- An empty fence means nothing special. "Empty reads, filled writes" was rejected: GitHub would show blank examples; clearing a fence to rewrite it would restore the old code from disk on the next open; and entangled reads an empty `file=` fence as an empty file.

### Where entangled sits

```text
crates/leolib         @auto-lit parse and exact writer. No new dependency.
crates/leo-entangled  tangle, check, include= sync. Depends on leolib and entangled-rs, default-features = false.
crates/leoapp         optional dependency on leo-entangled, behind a feature
```

leolib keeps its three runtime dependencies. Commands in leoapp: `:entangled-tangle` and `:entangled-check`, run on the outline's project, with results on the status line and in `:messages`. Tests run outside leo-rs.

### The Python test harness

The README holds examples only, with no `file=`, so readers see plain code. A second document, kept out of the docs, holds the harness and refers across documents:

````markdown
```python file=tests/test_readme.py
def test_add():
    <<README.md#add>>
```
````

`entangled tangle`, or `:entangled-tangle`, writes `tests/test_readme.py`; pytest runs it. An example that cannot run alone is simply not referenced. To verify: entangled indents an expansion to its reference's column, as Leo's section references do.

## Plan

1. **Clone conflicts, in leolib.** Done 2026-10-07: a clone two files disagree on is reported on read, and both texts kept under `Recovered Nodes` (`docs/dev/porting-notes.md`). Python Leo keeps the last file's text without a word.
2. **`@auto-lit` parse and exact writer, in leolib.** Fences and prose runs as nodes, byte-identical round trip, corpus cases.
3. **Open a real document.** An entangled document from entangled-rs's `examples/`, with step 2 only: does the outline read well? Cheap, and it gates step 4.
4. **leo-entangled.** `file=` through entangled's tangle, `:entangled-check`, then `include=`.
5. **The Python harness.** The convention above, a demo in `demo/`, and the indentation check.

Outside leo-rs, before step 4: release entangled-rs 0.3.0 with the reduced dependencies.

## Open questions

- The kind's name, and a fence node's headline: `add`, `#add`, or the info string.
- How a heading keeps its source form (`#` or underlined) for the exact writer: in the body's first line, or in a uA.
- Whether `include=` is wanted at all once `file=` and the harness cover testing.
- How continuation blocks read as n siblings with one headline; number them if not.
