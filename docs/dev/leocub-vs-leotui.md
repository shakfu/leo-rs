# leo-cub and leo-rs

A comparison of [leo-cub](https://github.com/vivainio/leo-cub) with leo-rs (`leolib` and `leotui`), written on 2026-09-21. Its author asked whether leo-rs could reuse any of its code. Versions: leo-cub `f5d5876`, leo-rs `0047f53`.

It rests on reading leo-cub's module docs, public items and selected functions. leo-cub was not built or run against `LeoPyRef.leo`, so each conformance claim below is inferred from its design.

## Summary

- No leo-cub module can move into `leolib` as it is.
- Worth taking now: a few TUI commands.
- Deferred: a non-interactive CLI with JSON operation batches, until leotui is a usable editor and conformance with leo-editor is good.
- Rhai is the recorded pathway if scripting or plugins are ever wanted. Neither is planned.
- The reverse direction may be the larger gain: leo-cub building on `leolib`.

## Why the code does not carry over

1. **Different data model.** leo-cub keys nodes and positions by strings (`NodeId(String)`, `PositionId(String)`, `src/model.rs`). `leolib` uses a vnode arena and `Position` paths. Every leo-cub module is written against its own model, so a port is a rewrite.

2. **Different goals.** `leolib` is checked against Python Leo: byte-identical `.leo` and external-file writes, and the corpus in `demo/`. leo-cub aims to be "automation-safe" and departs from Leo on purpose in its core:

   - `src/clean.rs` updates `@clean` files with its own LCS diff. `atclean.rs` uses `seqmatch.rs`, a port of Python's `difflib.SequenceMatcher`. The two can pair lines differently, so leo-cub's `@clean` results can differ from Leo's.
   - `src/auto.rs` builds `@auto` trees with tree-sitter. The trees are read-only and never written back. `leolib`'s importers match Leo's on 998 of 1,000 files and write back. `leolib` covers every language leo-cub does except Go, which `TODO.md` lists as having no importer.
   - `src/relative.rs` reads `@f`, a new sentinel format ("cub-1-thin", from leo-editor issue #4928). Python Leo cannot read it.
   - `@auto-dir` expands a directory or glob into one `@auto` tree. It is not Leo syntax.
   - New GNXs have the form `cub.<secs>.<nanos>`, not Leo's `id.YYYYMMDDHHMMSS.n`.

3. **Overlap.** `leolib` already has the XML read and write, thin `@file` reading and writing, `@clean`, `@path` resolution and the importers, each verified against Leo.

## What is worth taking now

| leo-cub piece | value for leo-rs | form of reuse |
|-|-|-|
| TUI: `n`/`N` cycle a node's clone occurrences, next marked node, mark all found, `Ctrl-P` incremental headline find | `TODO.md` asks for the marked-node commands | ideas only; `src/tui.rs` is one 8,654-line file tied to its model |

## CLI: deferred

A non-interactive CLI waits until leotui is a usable editor and conformance with leo-editor is good. When it comes, these leo-cub pieces are the reference:

| leo-cub piece | value for leo-rs | form of reuse |
|-|-|-|
| `inspect --format json/json-tree`, `validate`, `render`, `diff`, `add` (`src/inspect.rs`, `src/tree.rs`) | leo-rs has no non-interactive tool besides `leotui --dump` | reimplement over `leolib`, as a `leocli` crate or leotui subcommands |
| `cub apply`: atomic JSON operation batches with `expected` text preconditions (`src/operation.rs`) | other tools edit an outline without a TUI; maps onto `Document` and its undo | copy the design and JSON schema; the code is bound to leo-cub's model |
| headline paths with `\/` escaping (`escape_headline_path_component`) | addresses a node without its GNX | near-direct copy |
| agent skill (`skills/leo-cub/SKILL.md`, `cub install-skills`) | documents the CLI for AI agents | adapt the text |

## Scripting and plugins: deferred

leo-rs adds neither now. The policy follows Helix: strong built-in defaults first, and extension only when users ask for it. `ideas.md` records the constraint.

If users do ask, Rhai is the pathway, and leo-cub (`src/rhai_run.rs`, `docs/reference/rhai-api.md`) is a working reference. Reasons, from the [Rhai book](https://rhai.rs/book/about/features.html):

- A script has no file or process access unless the host registers it. leo-cub registers `sh` and `rhai-fs`; leo-rs need not.
- `Engine::set_max_operations` and related limits stop a runaway script.
- It is pure Rust, sits behind a Cargo feature, and needs no interpreter on `PATH`.

Rules for that day, so plugins keep leo-rs's files readable by Python Leo:

- Plugins edit only through `Document` commands, so every write goes through `leolib`'s verified writers and is undoable. `Document::outline` must stop being public first (`TODO.md`).
- Plugins live in the config directory, enabled in `config.toml`, not in the outline. A `.leo` file then carries no code.
- Plugins register commands and hooks, named after Leo's `@g.command` and `g.registerHandler` hooks where one fits.
- `leolib` stays free of scripting.

## Leave out

- `@auto-dir` and `@f`. Neither is Leo syntax, and a file using them breaks in Python Leo.
- `@action` and `@import` nodes. They store code in the outline, so opening a `.leo` file from someone else can offer that file's code to run. `@action` is also not Leo's name: Leo's are `@button`, `@command` and `@script`, with Python bodies.
- syntect highlighting (`src/syntax.rs`). leotui uses tree-sitter.

## License

leo-cub's `LICENSE` file is MIT; its `Cargo.toml` says `MIT OR Apache-2.0` with no Apache licence file in the repository. Both are compatible with leo-rs's MIT. Copied code keeps the "leo-cub contributors" copyright notice.

## Another option: leo-cub builds on `leolib`

leo-cub's `xml.rs`, `derived.rs`, `sync.rs` and `clean.rs` redo what `leolib` has verified against Leo. `leolib` could be the Leo-conformant core, and leo-cub its automation layer: `apply`, `inspect`, Rhai, `@auto-dir`. leo-cub would gain conformance, and each project would do one job.

## Open questions

- Is leo-cub to stay compatible with Python Leo, or become its own format (`@f`)? The second rules out a shared core.
- Would its author accept a `leolib` dependency, given the API gaps in `TODO.md`: public `Document::outline`, `Error` not `#[non_exhaustive]`?
- Which part did its author mean as reusable: the core, or the automation layer?
