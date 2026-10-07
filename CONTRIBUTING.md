# Contributing

## Before a change is done

- `make check` passes: `cargo fmt --check`, clippy with `-D warnings`, and every test. leolib warns on a public item with no doc comment, so clippy fails on one.

- A change to how leolib reads or writes passes `make corpus LEO_EDITOR=~/projects/leo-editor`, which checks `demo/`'s expected files against Python Leo. CI pins the leo-editor commit in `.github/workflows/ci.yml`.

- A failing test is a question about the code first. Do not weaken a test unless the code cannot meet the stronger one.

## Behaving like Leo

leolib is a port. Where it reads or writes a file, the answer is what Leo's Python gives, and the corpus holds both to the same files:

- A new case is a builder in `scripts/make_corpus.py`; `--create` rebuilds `demo/cases/`, and a run without `--check` writes the expected files.

- A deliberate difference goes in `KNOWN` in `crates/leolib/tests/corpus.rs`, with the reason, and in `docs/dev/porting-notes.md`.

## Writing it down

- `CHANGELOG.md`, under Unreleased: what changed, then why this option over another where there was one.

- `TODO.md` holds what is agreed; `docs/dev/delta.md` records what Leo has and leo-rs does not.

- A new leotui key appears in the README's tables; `crates/leotui/tests/readme.rs` checks it.

## Other checks

- `make bench` times loading and saving `LeoPyRef.leo`. With `LEO_EDITOR` set, it also times leo-editor's own outline with its external files.

- `fuzz/` holds `cargo-fuzz` targets, run on nightly: `cargo +nightly fuzz run read_into_root`.

- `make audit` checks dependencies against the RustSec database.
