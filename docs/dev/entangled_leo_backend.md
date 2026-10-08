# Literate markdown with entangled

Revised 2026-10-07; replaces the sketch of 2026-09-12. Since 2026-10-08 the code is in `crates/leo-entangled` (the kind, its tests and data, and the leoapp plugin), on `crates/leo-markdown`'s scanner and writer; the paths below are where it was built (`docs/dev/plugins.md`). Nothing here is scheduled. A line says when something was measured or run; the rest is judgement.

## Goal

Code examples in markdown go stale when nothing tests them against the API. Edit such a document in Leo as an outline whose code fences are nodes, and have the examples written to files a test runner runs. [entangled](https://github.com/shakfu/entangled-rs) already defines the fence syntax and the tangling; leo-rs adds the outline.

This is a third way for leo-rs to hold markdown, beside `@auto-md` (structure only) and `@edit` (flat text).

## Decisions

- **The markdown must render on GitHub.** A fence always holds its code. No design may leave fences empty in the file.
- **Full entangled compatibility**, with one exception. A document tangles the same under the `entangled` CLI and under leo-rs, including references, namespaces and annotations.
- **One fence per name** (2026-10-07). entangled joins fences that share a name into one block; leo-rs refuses such a file, as Leo refuses a section defined twice. A name must say which node it is. entangled-rs 0.4.0 refuses them too by default; its `split_blocks = true` restores the joining, which leo-rs does not offer.
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

### A new kind, `@entangled`

```text
┌───────────────────────────────────────────┬───────────────────┬─────────────────────────────┐
│            What the kind does             │     Direction     │      entangled's word       │
├───────────────────────────────────────────┼───────────────────┼─────────────────────────────┤
│ makes fences into nodes, writes the       │ markdown ↔        │ none (this is what leo-rs   │
│ markdown back                             │ outline           │ adds)                       │
├───────────────────────────────────────────┼───────────────────┼─────────────────────────────┤
│ writes file= targets on save              │ markdown → code   │ tangle                      │
│                                           │ file              │                             │
├───────────────────────────────────────────┼───────────────────┼─────────────────────────────┤
│ fills include= fences from their files    │ code file →       │ stitch                      │
│                                           │ markdown          │                             │
└───────────────────────────────────────────┴───────────────────┴─────────────────────────────┘
```

A headline `@entangled <path>` names the markdown file, as `@clean` does. It has its own scanner, which follows CommonMark: `#` headings need a space and stop at six, unlike `@auto-md`'s, because the file must render on GitHub. `@auto-md` stays as Leo has it.

One word, not an `@auto-` variant, because it does more than import: it writes `file=` targets too. Leo's own kinds are single words (`@clean`, `@edit`), and so is leo-rs's proposed `@wiki` (`docs/dev/wiki.md`). Python Leo matches a kind by the whole word (`findAtFileName`, `leoNodes.py:2359`), so it reads `@entangled` as a plain node and never touches the file.

`@wiki` is the other markdown kind, and the two differ:

| | `@wiki <name>` | `@entangled <path>` |
|-|-|-|
| source of truth | the outline | the markdown file |
| direction | export only: the outline writes `<name>.md` | both: read on open, written on save |
| argument | a name, also the link namespace | a file path |

### The tree

Headings are nodes, and a *named* fence's code is a child node reached by a section reference, as in the `@clean` spike. Prose never becomes a node:

````text
## Adding                          heading node; its body is the document's text
Two numbers:

```python #add file=hello.py       the fence lines stay in the heading's body
<< add >>                          the code is replaced by a reference...
```

That is all.

  << add >>                        ...to a child node holding only the code
    print(2 + 3)
````

- **Named fences only.** A fence is named when entangled would name it, in the style entangled reads from the file's extension: `#name` or `file=` in the info string for `.md` (entangled-rs and Pandoc forms), `label=` or `file=` in a braced knitr header for `.Rmd`, `#| label:` or `#| file:` lines opening the block for `.qmd`. A fence named in another style's way stays text, as entangled skips it. An unnamed fence stays as text in its heading's body, so a writer chooses which examples become nodes.
- **The fence node's headline is `<< name >>`**: the `#name`, the label, or for a `file=` block with no name, its file path, as entangled names it. Names are unique in a document: a file in which two fences share one is not read as a tree (the whole file stays in the node, with an error), a rename to a name in use is refused, and the writer refuses two fences with one name.
- **The root's body** is the text before the first heading: front matter, prose, fences.
- **The writer** walks the tree and concatenates bodies, putting each child's code back in place of the reference that follows a fence-opening line. Only a reference directly after a fence-opening line is expanded, so a `<< name >>` in prose or an entangled `<<ref>>` inside code is left alone. An unedited file is written byte for byte.
- **A heading keeps its source form** (`##` or underlined, trailing spaces) in an in-memory attribute, with its level and its depth in the tree when read, and is written as it was while its text and depth are unchanged. A heading node moved to another depth has its level moved by as much, its subheadings with it, kept to 1 to 6: demoting `## Usage` under `## Install` writes `### Usage`, and promoting it back restores the line as it was. An edited or moved heading is written fresh in its own style; an underlined one moved past level 2 becomes `###`. A heading added in leo-rs is ATX, one level below its parent.
- **A fence's language** (`python`, from `python`, `.python` or `{python`) is kept in an in-memory attribute that `Outline::language_at` reads first. The fence node is then Python to the colouring and the language servers, so every example gets completion and diagnostics. `@language` cannot be used: the directive would be written into the markdown.
- **An indented fence** (in a list item) has its indent taken off the code and put back on write, when every code line carries it.
- **The read checks itself**: the tree is written and compared with the file before it is kept. If they differ, the whole file goes into the root's body with an error, as an `@auto` import that loses text does.

The attributes live only in memory: an `@entangled` node's children are rebuilt from the file on every open and never stored in the `.leo` file, as for `@auto`. gnx is allocated at parse time and is not durable.

### Renaming a fence

Editing a fence node's headline renames the block. leo-rs rewrites the `#name` in the fence line and its own reference, as one undo step; a name another block in the document has is refused. It also rewrites entangled's references, `<<name>>` and `<<doc.md#name>>`, in every `@entangled` document in the outline, and reports that documents outside the outline cannot be checked; `:entangled-check` finds them. Leo itself never rewrites section references on a rename; this follows `:lsp-rename`.

Editing the info string in the heading's body changes `file=` or the language without touching any reference.

### Fence semantics

entangled's, plus one extension that entangled ignores.

| fence | on read | on save |
|-|-|-|
| ```` ```python #add ```` | a node headlined `<< add >>` | the markdown |
| ```` ```python #main file=hello.py ```` | a node; a report if `hello.py` changed on disk since leo-rs last wrote it | the markdown, and `hello.py` as entangled tangles it |
| ```` ```python #add include=lib.py#add ```` | a read-only node filled from `lib.py`, or from its `ANCHOR: add` region | the markdown, the fence filled from the file as it is then |
| ```` ```python ```` (unnamed) | text in the heading's body | the markdown |
| several ```` #add ```` fences | refused: the whole file in the node, with an error | the file as read |

- `file=` is entangled's tangle: `<<references>>` expanded, the project-wide namespace, annotations and `entangled.toml` all apply. leo-rs calls entangled for it rather than reimplementing it. leolib's write guards still apply: no overwriting a file never read, or one changed on disk.
- `include=` is the read direction, for code tested on its own: the file is the source. `include=path` takes the whole file and `include=path#name` the lines between `ANCHOR: name` and `ANCHOR_END: name` comments, without anchor lines, as mdBook takes them; the path starts from the markdown file's directory. The fence keeps the code, so GitHub shows it. On read the node is filled from the file after the round-trip check, and a fence whose file changed leaves the `@entangled` node unsaved, so the next save brings the markdown up to date; on write the fence is filled from the file as it is then. The node is read-only: leoapp refuses an edit and names the file to edit. An `include=` fence is a node even without `#name`, named by its target. In `.Rmd` and `.qmd` files `include` is knitr's and Quarto's own yes-or-no option, so it is left alone there. A missing file or anchor is a read warning, and the fence keeps its text.
- An empty fence means nothing special. "Empty reads, filled writes" was rejected: GitHub would show blank examples; clearing a fence to rewrite it would restore the old code from disk on the next open; and entangled reads an empty `file=` fence as an empty file.

### Where entangled sits

First through the CLI, with no new dependency: `:entangled-tangle` and `:entangled-check` run the `entangled` binary in the outline's directory and report its output on the status line and in `:messages`. Tangling then follows `entangled.toml`, references, namespaces and annotations exactly, because it is entangled. leo-rs's changed-on-disk check notices the files it writes.

Later, if tangling on save or `check` results as body diagnostics are wanted:

```text
crates/leolib         @entangled parse and exact writer. No new dependency.
crates/leo-entangled  tangle and check in-process. Depends on leolib and entangled-rs, default-features = false.
crates/leoapp         optional dependency on leo-entangled, behind a feature
```

leolib keeps its three runtime dependencies either way. `include=` needs no entangled: leolib reads the named file itself. Tests run outside leo-rs.

### The Python test harness

The README holds examples only, with no `file=`, so readers see plain code. A second document, kept out of the docs, holds the harness and refers across documents:

````markdown
```python file=tests/test_readme.py
def test_add():
    <<README.md#add>>
```
````

`entangled tangle`, or `:entangled-tangle`, writes `tests/test_readme.py`; pytest runs it. An example that cannot run alone is simply not referenced. entangled indents an expansion to its reference's column, as Leo's section references do: checked 2026-10-07 with entangled 0.2.1, and the example indents its blank lines too, which Python ignores.

`demo/entangled/` is a working copy of this: a README documenting a small library, a `tests.md` harness tangled to `test_readme.py`, and `demo.leo` holding both. `corpus.rs` and `make_corpus.py` skip that directory: every other `.leo` file under `demo/` must match Python Leo, and Leo reads `@entangled` as a plain node.

## Plan

1. **Clone conflicts, in leolib.** Done 2026-10-07: a clone two files disagree on is reported on read, and both texts kept under `Recovered Nodes` (`docs/dev/porting-notes.md`). Python Leo keeps the last file's text without a word.
2. **Phase 1: the kind, in leolib.** Done 2026-10-07 (`crates/leolib/src/entangled.rs`, with the CommonMark scanner and writer in `markdown.rs` since the `@auto-cells` work; tests in `crates/leolib/tests/entangled.rs`), with leolsp serving each fence node as a document in its language. Recognising `@entangled`, the scanner, the tree above, the exact writer and its read-time check, fence languages. Tests: round trips over entangled-rs's `examples/` and edge cases (underlined headings, `~~~` and four-backtick fences, indented fences, front matter, CRLF). Python Leo knows nothing of the kind, so the Leo corpus cannot check it.
3. **Phase 1b: renaming, in leolib and leoapp**, as above. Done 2026-10-07: `entangled::plan_rename` works out every edit before any is made, `Document::rename_entangled_block` applies them as one undo step, and the headline edit and MCP's `set_headline` go through it. A block named only by `file=` is refused. Checked on `demo/entangled/`: entangled 0.2.1 tangles and checks the renamed documents, and the tests pass.
4. **Open a real document.** An entangled document from entangled-rs's `examples/`: does the outline read well?
5. **Phase 2: `:entangled-tangle` and `:entangled-check`**, through the CLI. Done 2026-10-07 (`crates/leoapp/src/app/entangled.rs`): run on a thread, unsaved `@entangled` files written first, output in `:messages`, the program set by `entangled`. Checked against entangled-rs's `literate-crypto` example: an edit in a fence node reached the tangled `ciphers.py`.
6. **Phase 3: `include=`**, in leolib. Done 2026-10-07: whole files and `ANCHOR` regions, read-only nodes, filled on read and on write.
7. **Phase 4: the Python harness.** Done 2026-10-07: `demo/entangled/`, its tests passing under entangled 0.2.1, a stale example making its test fail, and the indentation checked.

entangled-rs 0.3.0 has the reduced dependencies, so linking it in-process is now possible; the CLI is still used. A plan for in-process tangling and checks, an `entangled-core` crate with few dependencies, is deferred (2026-10-07): entangled-rs `docs/core-split.md`.

## Open questions

- Done: the named-fence rule is tested against entangled's own reader. `fence_nodes_are_the_blocks_entangleds_reader_finds` (`crates/leolib/tests/entangled.rs`) runs `entangled::readers::parse_markdown` from entangled-rs 0.4.0, a dev-dependency without default features, over entangled-rs's examples and one document per style, two of them mixing styles: the fence nodes are entangled's blocks, in order. entangled-rs 0.3.0 also fixed its Pandoc reader, which could not read its own `examples/pandoc-style` before.
