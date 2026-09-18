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

- [ ] In body focus, `Ctrl-s`, `Ctrl-f`, `Ctrl-b`, `Ctrl-d`, `Ctrl-u`, `PageUp`, `PageDown` and `F1` answer "no such command", though the README lists them for both panes. `editor/parse.rs` knows only `Ctrl-r`, `Ctrl-z`, `Ctrl-c` and `Ctrl-w`.

- [ ] Ctrl and Alt chords type their letter in INSERT and in every one-line input. `Ctrl-w` and `Ctrl-u` do nothing there.

- [ ] `Ctrl-c` in INSERT discards the whole session, with no undo bead.

- [ ] `:set wrap` does not wrap: `ui.rs` cuts each run to the pane width first.

- [ ] The body cursor is placed by character index, so it drifts on tabs and wide characters. There is no horizontal scroll, so typing past the pane's edge is invisible.

- [ ] The body cursor returns to row 0 on every node switch.

- [ ] Status messages vanish on the next key, and nothing keeps them (`:messages`). `ReadResult::warnings` are never shown.

- [ ] `:e` rebuilds `App` and drops the theme, colour depth, text register and last search.

- [ ] Counts overflow, which panics in a debug build, and a failing repeat keeps looping: `99999999K` runs for 1.9s.

- [ ] A failed `/pattern` then Enter prints nothing. Escape leaves open the folds the preview unfolded.

- [ ] The outline's colours are fixed and ignore the theme, `NO_COLOR` and light backgrounds.

- [ ] `leotui new.leo` fails with "not found" instead of starting that outline.

- [ ] Ensure to implement Commands a Leo user reaches for first. In rough order: goto-global-line and its reverse, clone-find-all, hoist, jump to a `<< section >>` definition, go-back and go-forward, extract, the marked-node commands, sort. `tui-design.md` section 6.1 lists hoist, sort and the marked-node commands as v1; none exist. `ideas.md` covers the route to goto-global-line through `:!` and a quickfix list.

## Medium

### leolib API

- [ ] `Document::outline` and `Document::undoer` are public, so an edit through `outline` skips undo. The `g<` bug was this.

- [ ] `Outline::position_exists` checks only a position's last step. `position_is_linked` is the real test, and nothing says so.

- [ ] 258 public items had no doc comment at `825d3b8`, and `Error` is not `#[non_exhaustive]`.

### Normalise builtin types across the grammars

- [ ] Only 3 of the 12 grammars tag `@type.builtin` at all: java, rust, typescript. tree-sitter-c puts its primitives on `(primitive_type) @type` and `(sized_type_specifier) @type`, and go, python and javascript do the same for theirs. So `Class::BuiltinType` rarely fires, and a C `int` draws as a plain type while a Rust `u8` draws as a builtin one.

- [ ] `treesit` already corrects one grammar this way. It appends `(integer_literal) @constant.numeric` to the Rust query, because a later pattern wins. The same shape applies here: append `(primitive_type) @type.builtin` and `(sized_type_specifier) @type.builtin` to C's query. C++ needs nothing of its own, since its query is already C's with C++'s appended.

- [ ] Unmeasured: whether go, python and javascript earn the same treatment. A builtin type may only read as distinct in a language that has few of them.

- `@first` on the rename route. Renaming `@auto` to `@file` and writing with `w` moves a leading shebang to line 3, below the sentinel header. `:import-at-file` adds `@first`; the rename does not. Add `@first` by hand first.


- [ ] Nothing frees a vnode, and the undo stack has no cap. Deleting a node leaves its vnode in the arena, which is what makes undoing a delete a relink rather than a rebuild (`undo.rs`). The stack itself is unbounded. Neither matters for an editing session of ordinary length; together they mean a long-lived process editing a large outline has no steady state. A cap has to drop beads and their vnodes together, or undo starts relinking nodes that are no longer there.

### Low

- [ ] refactor `app.rs` at 3,200 lines


