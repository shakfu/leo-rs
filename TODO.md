# TODO

## Normalise builtin types across the grammars

Only 3 of the 12 grammars tag `@type.builtin` at all: java, rust, typescript. tree-sitter-c puts its primitives on `(primitive_type) @type` and `(sized_type_specifier) @type`, and go, python and javascript do the same for theirs. So `Class::BuiltinType` rarely fires, and a C `int` draws as a plain type while a Rust `u8` draws as a builtin one.

`treesit` already corrects one grammar this way. It appends `(integer_literal) @constant.numeric` to the Rust query, because a later pattern wins. The same shape applies here: append `(primitive_type) @type.builtin` and `(sized_type_specifier) @type.builtin` to C's query. C++ needs nothing of its own, since its query is already C's with C++'s appended.

Unmeasured: whether go, python and javascript earn the same treatment. A builtin type may only read as distinct in a language that has few of them.

## `@first` on the rename route

Renaming `@auto` to `@file` and writing with `w` moves a leading shebang to line 3, below the sentinel header. `:import-at-file` adds `@first`; the rename does not. Add `@first` by hand first.

## Nothing frees a vnode, and the undo stack has no cap

Deleting a node leaves its vnode in the arena, which is what makes undoing a delete a relink rather than a rebuild (`undo.rs`). The stack itself is unbounded. Neither matters for an editing session of ordinary length; together they mean a long-lived process editing a large outline has no steady state. A cap has to drop beads and their vnodes together, or undo starts relinking nodes that are no longer there.

## `app.rs` is 2,500 lines

59 methods on one `App` impl. Every other file in the workspace is under 1,100, including the parts of the TUI already split out (`editor/`, `minibuffer`, `search`, `substitute`, `theme`). The dispatcher, the mode handlers, the minibuffer glue and the command-line runner are separable, and the method names already say which is which. Worth doing when something else takes you into that file, not on its own.

## Say which leo-editor the `@auto` figures came from

The README's status table gives 998 of 1,000 `@auto` trees identical, without naming the checkout it was measured against; `docs/dev/comparison.md` pins its own figures to `3acfadd8d0`. A run at `b6e06060ad` no longer reproduces the table, because Leo's reader now raises on a file with nothing in it -- `demo/cases/empty_auto` pins that one. Re-measure, and name the commit.

---

`docs/dev/tui-design.md` section 19.8 holds what else is not done in the body colouring.
