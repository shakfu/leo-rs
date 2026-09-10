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

---

`docs/dev/tui-design.md` section 19.8 holds what else is not done in the body
colouring.
