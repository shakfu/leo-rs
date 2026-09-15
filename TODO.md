# TODO

Most entries below come from the review of 2026-09-15: a conformance run of both implementations against leo-editor `e3b3841f64`, and a usability pass driven with `--dump --press` and tmux. Each was reproduced unless it says otherwise.

## Can lose or corrupt text

- **`@clean` mtimes are whole seconds.** `atclean.rs` skips the read when the cached second is at or after the file's. An outside edit in the same second as a write is never read, and the next write reverts it. Leo compares float mtimes. `util::FileStamp` already holds a `SystemTime`.
- **Opaque uAs are written unescaped.** `leofile.rs` unescapes attribute values on read and writes them raw. A `"` or `<` in a uA makes the saved `.leo` file unreadable by both implementations.
- **The descendent-uA blob goes stale.** `descendentVnodeUnknownAttributes` is written back verbatim, but `Document`'s inserts, deletes and moves change the subtree it indexes. Leo then restores uAs onto the wrong nodes. Regenerate it as `fc.putDescendentVnodeUas` does.
- **`@path` in an `@clean`, `@auto`, `@edit`, `@nosent` or `@asis` body is ignored.** `Outline::get_path_from_node` skips every `@<file>` kind; Leo skips only `@file` and `@thin` (`leoOutline.py:594`). `@clean` and `@nosent` are exempt from `may_overwrite`, so the write can land on an unrelated file.
- **Unedited CRLF files are rewritten as LF.** `replace_file` compares bytes, where Leo's `compareIgnoringLineEndings` ignores `\r`. A write-all rewrites every CRLF file. A CRLF `.leo` file also keeps `\r` in its bodies, which an XML parser would normalise.
- **An `@auto` tree that writes empty truncates its file.** Leo reports "not written" (`leoAtFile.py:1686`).
- **`@edit`:** an empty file on disk replaces the body with `@nocolor`, and a node with children is written. Leo keeps the body, and refuses the write (`leoAtFile.py:1802`).
- **A hard link is broken by the atomic rename.** Writing in place when the link count is above 1 keeps it, at the cost of atomicity. Inferred, not reproduced.

## Encoding names

- `@encoding cp1252` is not in `outline::is_valid_encoding`'s list, so `get_encoding` falls back to utf-8 and the file is written as UTF-8 without a report. Refuse it, as `@encoding latin-1` is refused.
- `HEADER_PATTERN` in `atfile_read.rs` keeps the comma of `-encoding=utf8,.`. The writer emits that header for any encoding spelled other than `utf-8`, so an `@encoding utf8` file does not read back. Leo strips the comma (`leoAtFile.py:1069`). The failure is safe: the node stays empty and the write is refused.

## Reader differences

- A clone shared by two `@file` trees gains duplicate parent links. Case 3 in `atfile_read.rs` clears an existing clone's children without removing it from their parent lists, and `is_cloned` then answers true for nodes that are not clones.
- `@auto` on an extension with no importer (`.txt`, `.json`, `.yaml`, `.toml`, `.sh`, `.css`, `.go`, `.rst` and others) is reported unread. Leo puts the whole file in the body with `@language` set (`leoImport.py:673`).
- `@jupytext` is read and written as a sentinel `@file`.
- Reader warnings are collected in `atfile_read.rs` and never returned.
- An unparseable `@tabwidth` or `@lineending` hides the ancestor's value, where Leo's pattern falls through to it. Read from code.

## `w` has Leo's name and a different meaning

`w` runs `write-at-file-nodes`, which writes the dirty files. Leo's `write-at-file-nodes` writes every `@<file>` node under the selection, dirty or not (`leoFileCommands.py:708`); dirty-only is `write-dirty-at-file-nodes`. Rename the command, and add Leo's as a second one.

## leotui

- In body focus, `Ctrl-s`, `Ctrl-f`, `Ctrl-b`, `Ctrl-d`, `Ctrl-u`, `PageUp`, `PageDown` and `F1` answer "no such command", though the README lists them for both panes. `editor/parse.rs` knows only `Ctrl-r`, `Ctrl-z`, `Ctrl-c` and `Ctrl-w`.
- Ctrl and Alt chords type their letter in INSERT and in every one-line input. `Ctrl-w` and `Ctrl-u` do nothing there.
- `Ctrl-c` in INSERT discards the whole session, with no undo bead.
- `:set wrap` does not wrap: `ui.rs` cuts each run to the pane width first.
- The body cursor is placed by character index, so it drifts on tabs and wide characters. There is no horizontal scroll, so typing past the pane's edge is invisible.
- The body cursor returns to row 0 on every node switch.
- Status messages vanish on the next key, and nothing keeps them (`:messages`). `ReadResult::warnings` are never shown.
- `:e` rebuilds `App` and drops the theme, colour depth, text register and last search.
- Counts overflow, which panics in a debug build, and a failing repeat keeps looping: `99999999K` runs for 1.9s.
- A failed `/pattern` then Enter prints nothing. Escape leaves open the folds the preview unfolded.
- The outline's colours are fixed and ignore the theme, `NO_COLOR` and light backgrounds.
- `leotui new.leo` fails with "not found" instead of starting that outline.

## Commands a Leo user reaches for first

In rough order: goto-global-line and its reverse, clone-find-all, hoist, jump to a `<< section >>` definition, go-back and go-forward, extract, the marked-node commands, sort. `tui-design.md` section 6.1 lists hoist, sort and the marked-node commands as v1; none exist. `ideas.md` covers the route to goto-global-line through `:!` and a quickfix list.

## leolib API

- `Document::outline` and `Document::undoer` are public, so an edit through `outline` skips undo. The `g<` bug was this.
- `Outline::position_exists` checks only a position's last step. `position_is_linked` is the real test, and nothing says so.
- 258 public items had no doc comment at `825d3b8`, and `Error` is not `#[non_exhaustive]`.

## Normalise builtin types across the grammars

Only 3 of the 12 grammars tag `@type.builtin` at all: java, rust, typescript. tree-sitter-c puts its primitives on `(primitive_type) @type` and `(sized_type_specifier) @type`, and go, python and javascript do the same for theirs. So `Class::BuiltinType` rarely fires, and a C `int` draws as a plain type while a Rust `u8` draws as a builtin one.

`treesit` already corrects one grammar this way. It appends `(integer_literal) @constant.numeric` to the Rust query, because a later pattern wins. The same shape applies here: append `(primitive_type) @type.builtin` and `(sized_type_specifier) @type.builtin` to C's query. C++ needs nothing of its own, since its query is already C's with C++'s appended.

Unmeasured: whether go, python and javascript earn the same treatment. A builtin type may only read as distinct in a language that has few of them.

## `@first` on the rename route

Renaming `@auto` to `@file` and writing with `w` moves a leading shebang to line 3, below the sentinel header. `:import-at-file` adds `@first`; the rename does not. Add `@first` by hand first.

## Nothing frees a vnode, and the undo stack has no cap

Deleting a node leaves its vnode in the arena, which is what makes undoing a delete a relink rather than a rebuild (`undo.rs`). The stack itself is unbounded. Neither matters for an editing session of ordinary length; together they mean a long-lived process editing a large outline has no steady state. A cap has to drop beads and their vnodes together, or undo starts relinking nodes that are no longer there.

## `app.rs` is 3,200 lines

1,864 lines before its tests and 1,309 of tests, with 77 functions before the tests. Every other hand-written file in the workspace is under 1,400, including the parts of the TUI already split out (`editor/`, `minibuffer`, `search`, `substitute`, `theme`). The dispatcher, the mode handlers, the minibuffer glue and the command-line runner are separable, and the method names already say which is which. Worth doing when something else takes you into that file, not on its own.

## Re-measure the README's `@auto` rows

The README's status table pins its first four rows to leo-editor `e3b3841f64`. Its two `@auto` rows, 998 of 1,000 trees and 1,008 of 1,010 files, name no checkout; `docs/dev/comparison.md` pins its own figures to `3acfadd8d0`. A run at `b6e06060ad` no longer reproduces the tree row, because Leo's reader now raises on a file with nothing in it -- `demo/cases/empty_auto` pins that one. Re-measure both with `docs/dev/compare-importers.py`, and name the commit.

---

`docs/dev/tui-design.md` section 19.8 holds what else is not done in the body colouring.
