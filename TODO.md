# TODO

## Critical

## High

### Reader differences

- [ ] **Leo's `descendentVnodeUnknownAttributes` blob is not stable across a read.** Its pickled dict comes back in another key order, so opening a `.leo` file with two uAs on one node and saving it rewrites the file with no edit. `demo/cases/uas` puts one uA per node to stay inside leo-editor's "rewritten unchanged" test. Leo's bug: this port rebuilds the blob from the tree, in the key order the tree gives, so it does not reproduce it.

- [ ] **Leo reads a section reference back with its delimiters regex-escaped** when `@section-delims` set them (`leoAtFile.py:4016` assigns `re.escape`'d delims to `section_delim1`), so `{ imports }` becomes `\{ imports \}` in the body it hands back. This port keeps what the file spells. Report upstream; `corpus.rs`'s `KNOWN` holds it meanwhile.

- [ ] **`external::read_files` drops the `@clean` mod-time cache before every read.** Leo drops it only in `refresh-from-disk` (`commanderFileCommands.py:463`); the open path honours it (`readOneAtCleanNode`). Nothing observable follows: the cache is session-scoped and empty at open, and `leotui` reaches `read_files` alone, so the #4385 skip never fires here at all.

- [ ] `@auto` on an extension with no importer (`.txt`, `.json`, `.yaml`, `.toml`, `.sh`, `.css`, `.go`, `.rst` and others) is reported unread. Leo puts the whole file in the body with `@language` set (`leoImport.py:673`).

- [ ] `@jupytext` is read and written as a sentinel `@file`.

- [ ] `@pagewidth` is scanned and never read: `Outline::get_page_width` has no caller. Leo reflows a doc part to it.

- [ ] No corpus case covers `@verbatim` alone, `@auto-rst`, `@auto-otl` or `@auto-vim-outline`. `sentinel_lookalikes` exercises `@verbatim` in three kinds.


- [ ] `w` has Leo's name and a different meaning. `w` runs `write-at-file-nodes`, which writes the dirty files. Leo's `write-at-file-nodes` writes every `@<file>` node under the selection, dirty or not (`leoFileCommands.py:708`); dirty-only is `write-dirty-at-file-nodes`. Rename the command, and add Leo's as a second one.

### leotui

- [ ] `:set wrap` does not wrap: `ui.rs` cuts each run to the pane width first.

- [ ] The body cursor is placed by character index, so it drifts on tabs and wide characters. There is no horizontal scroll, so typing past the pane's edge is invisible.

- [ ] The body cursor returns to row 0 on every node switch.

- [ ] Status messages vanish on the next key, and nothing keeps them (`:messages`). `ReadResult::warnings` are never shown.

- [ ] `:e` rebuilds `App` and drops the theme, colour depth, text register and last search.

- [ ] A failed `/pattern` then Enter prints nothing. Escape leaves open the folds the preview unfolded.

- [ ] The outline's colours are fixed and ignore the theme, `NO_COLOR` and light backgrounds.

- [ ] `leotui new.leo` fails with "not found" instead of starting that outline.

- [ ] Ensure to implement Commands a Leo user reaches for first. In rough order: goto-global-line and its reverse, `move-marked-nodes`. `move-marked-nodes` is not undoable in Leo, which recommends `clone-marked-nodes` and a paste instead. `ideas.md` covers the route to goto-global-line through `:!` and a quickfix list.

## Medium

### leolib API

- [ ] `Document::outline` and `Document::undoer` are public, so an edit through `outline` skips undo. The `g<` bug was this.

- [ ] `Outline::position_exists` checks only a position's last step. `position_is_linked` is the real test, and nothing says so.

- [ ] 258 public items had no doc comment at `825d3b8`.

- [ ] `Outline` has public fields whose values depend on each other: `gnx_dict`, `expanded`, `mod_time_cache`, `read_paths`, `file_stamps`, `dropped_descendent_uas`. A caller that edits `read_paths` can defeat the overwrite guard; one that edits `gnx_dict` can break the gnx index. Make them `pub(crate)` behind accessors.

- [ ] `atfile_read` is `pub` only because `tests/corpus.rs` calls `read_into_root`; `atfile_write` is `pub(crate)`. Hide it with `#[doc(hidden)]`, or expose it behind a test-only feature.

### Project

- [ ] No benchmark is committed. `comparison.md`'s load times came from `hyperfine` runs that no script records, so they cannot be re-measured or checked in CI. Add a `criterion` bench over leo-editor's `LeoPyRef.leo`.

- [ ] No fuzz target. The sentinel reader, the pickle reader and the importers are parsers fed untrusted files. Add `cargo-fuzz` targets for `read_into_root` and the pickle reader first.

- [ ] No `SECURITY.md` or `CONTRIBUTING.md`. The README says no write guard stops an outline creating a new file in an existing directory, and that a front end handling untrusted outlines must check `Outline::full_path` itself. `SECURITY.md` should state that threat model: a `.leo` file is as dangerous as a Makefile.

- [ ] `rust-version = "1.90"` limits who can build the crates. Keep it only as a deliberate choice, and say why in `Cargo.toml`.

### Normalise builtin types across the grammars

- [ ] Only 3 of the 12 grammars tag `@type.builtin` at all: java, rust, typescript. tree-sitter-c puts its primitives on `(primitive_type) @type` and `(sized_type_specifier) @type`, and go, python and javascript do the same for theirs. So `Class::BuiltinType` rarely fires, and a C `int` draws as a plain type while a Rust `u8` draws as a builtin one.

- [ ] `treesit` already corrects one grammar this way. It appends `(integer_literal) @constant.numeric` to the Rust query, because a later pattern wins. The same shape applies here: append `(primitive_type) @type.builtin` and `(sized_type_specifier) @type.builtin` to C's query. C++ needs nothing of its own, since its query is already C's with C++'s appended.

- [ ] Unmeasured: whether go, python and javascript earn the same treatment. A builtin type may only read as distinct in a language that has few of them.

- [ ] `@first` on the rename route. Renaming `@auto` to `@file` and writing with `w` moves a leading shebang to line 3, below the sentinel header. `:import-at-file` adds `@first`; the rename does not. Add `@first` by hand first.

- [ ] Nothing frees a vnode, and the undo stack has no cap. Deleting a node leaves its vnode in the arena, which is what makes undoing a delete a relink rather than a rebuild (`undo.rs`). The stack itself is unbounded. Neither matters for an editing session of ordinary length; together they mean a long-lived process editing a large outline has no steady state. A cap has to drop beads and their vnodes together, or undo starts relinking nodes that are no longer there.
