# leo-rs and leo-editor

A comparison of leo-rs (`leolib` and `leotui`) with the Qt-based leo-editor,
measured on 2026-09-10. Versions: leo-rs `a791e7d`, leo-editor `3acfadd8d0`,
Python 3.14.7, macOS.

## Measured

| | leo-rs | leo-editor |
|---|---|---|
| code | 23,639 lines of Rust, 4,375 of them generated tables | 94,125 lines of Python in `core` and `commands`, plus 98,340 in `plugins` |
| tests | 300 | 967 unit tests |
| commands | 85 | 934 distinct names |
| scripting (`@button`, `@command`, `execute-script`) | none | central |
| settings | `config.toml`, one key | `@settings` trees, `myLeoSettings.leo` |
| plugins | none | 98k lines |
| load `LeoPyRef.leo` and its external files | 0.78s | 0.48s, of which 0.11s is Python and bridge startup |

How each row was measured:

- **Code:** `wc -l` over `crates/*/src`, and over the Python in `leo/core`,
  `leo/commands` and `leo/plugins`. The generated tables are `keywords.rs` and
  `langdata.rs`.
- **Tests:** `#[test]` attributes in `crates/`; `def test_` under
  `leo/unittests`.
- **Commands:** entries in `COMMANDS`; distinct names in leo-editor's `@cmd`,
  `@g.command` and `@g.commander_command` decorators.
- **Load:** three warm runs each, which agreed to within 0.01s.
  - leo-rs: `leotui LeoPyRef.leo --dump`, release build.
  - leo-editor: `leoBridge.controller(gui='nullGui', loadPlugins=False,
    readSettings=False).openLeoFile(...)`.

The load gap is in the external files. With `--no-external`, leotui opens the
outline in under 10ms, so almost all of the 0.78s goes to reading them.
leo-editor's `leoCache.py` is not used by its file-reading code, so both
programs read the same files. It has not been profiled. Both timings are
headless; a Qt launch with plugins is slower, and was not measured.

## Where leo-rs is stronger

- **The model has no view.** `leolib` round-trips the Leo corpus exactly, and
  the corpus test checks it. That makes it usable where Qt is not: CI checks
  on `.leo` projects, other tools, SSH sessions, containers.
- **Safer writes.** It refuses to overwrite a file the outline has not read,
  and writes through a temporary file and a rename. leo-editor backs the file
  up and writes in place (`porting-notes.md`). An `@auto` import is checked to
  round-trip before the tree is kept.
- **Terminal-native editing.** Modal vim editing in the body, one binary,
  tree-sitter highlighting, Helix themes.

## Where it is weaker

- **No scripting.** leo-editor's distinguishing feature is Python run against
  the outline, through `c`, `g`, `p` and `@button`. Without it, leo-rs is an
  outliner that reads and writes Leo's formats. This is judgement, not
  measurement.
- **Breadth.** 85 commands against 934, no plugins, no import command.
- **Speed.** It loads external files in about twice the time of the Python it
  ports.

## Open question: what leo-rs is for

Each answer sets different priorities:

1. **A daily Leo in a terminal.** Scripting is the gap that matters. Leo
   scripts assume leo-editor's Python API, so running them means embedding
   Python, with pyo3 for example, and emulating that API over `leolib`'s
   model. That work is large and has no natural end.
2. **A library for Leo files.** Load speed and a stable `leolib` API matter
   most, and leotui is a demonstration of the library.
3. **A different product on Leo's formats.** An outliner with vim editing and
   tree-sitter, which stops chasing parity and spends its effort on what
   leo-editor does badly in a terminal.

Options 2 and 3 describe leo-rs as Leo's data model without Leo's commands. On
that reading, the next work is the load speed and the `leolib` API, not more
TUI commands.

## Assessment

This section is judgement, not measurement. Options 2 and 3 are achievable.
Option 1 is not without scripting, and scripting compatibility is likely a
matter of months. The load-speed gap is worth profiling under any of the
three.
