# TODO

## Normalise builtin types across the grammars

Only 3 of the 12 grammars tag `@type.builtin` at all: java, rust, typescript.
tree-sitter-c puts its primitives on `(primitive_type) @type` and
`(sized_type_specifier) @type`, and go, python and javascript do the same for
theirs. So `Class::BuiltinType` rarely fires, and a C `int` draws as a plain
type while a Rust `u8` draws as a builtin one.

`treesit` already corrects one grammar this way. It appends
`(integer_literal) @constant.numeric` to the Rust query, because a later
pattern wins. The same shape applies here: append `(primitive_type)
@type.builtin` and `(sized_type_specifier) @type.builtin` to C's query. C++
needs nothing of its own, since its query is already C's with C++'s appended.

Unmeasured: whether go, python and javascript earn the same treatment. A
builtin type may only read as distinct in a language that has few of them.

## An import-to-`@file` command

Leo's `import-file` turns a plain file into an `@auto` node
(`importAnyFile` in `leo/commands/commanderFileCommands.py`, with
`treeType='@auto'`). Only a file that already has sentinels becomes `@file`.
Converting by hand means three steps: rename the headline from `@auto` to
`@file`, save, and approve the prompt to overwrite the file with sentinels.
`recursive-import` accepts `@file` as a kind (`leoImport.py:1985`), but no
command does it for one file.

leotui has no import command, and the rename does not work here either.
`may_overwrite` keys on gnx, path and headline. After the rename, `w` reports
"refusing to overwrite a file this outline has not read", with no way to
approve.

The command imports a file as `@file` in one step:

- Build the tree with the existing `@auto` importer
  (`importers::import_string`, 23 languages).
- Ask before writing, because the write adds sentinels to a file leotui did
  not create. A `y/n` prompt, as `MiniKind::ConfirmQuit` already has.
- Keep a leading shebang or encoding line on line 1 with `@first`. Leo's
  rename route put `demo/fsm.py`'s shebang on line 3, below the sentinel
  header.
- For a file that already has sentinels, open it as `@file` directly, as
  Leo's `importDerivedFiles` does.

---

`docs/dev/tui-design.md` section 19.8 holds what else is not done in the body
colouring.
