# leo-rs

A Rust implementation of [Leo](https://github.com/leo-editor/leo-editor)'s
model layer (`leolib`) and a terminal front end that consumes it (`leotui`).

Leo's outline model was separated from its Qt front end in `leo/leolib`; this
port keeps that boundary. `leolib` reads and writes `.leo` files and the
external files they refer to, and knows nothing about how any of it is shown.
`leotui` is one front end over that crate. Nothing in `leolib` depends on it.

## Status

Verified against `leo/core/LeoPyRef.leo` from the Leo repository:

| check | result |
|---|---|
| nodes read from the `.leo` file | 536, identical gnx/headline/body to Python `leolib` |
| nodes read with all external files | 11,581, identical to Python `leolib` |
| `.leo` file rewritten | byte-identical to the file read |
| external files tangled | 381 of 381 byte-identical to the files on disk |
| `@auto` trees, 1,000 files across 8 languages | 998 identical to Leo's importers; the 2 differences are a deliberate fix |
| `@auto` files written back | 1,008 of 1,010 byte-identical; the 2 exceptions fail in Leo too |

Run those checks with `make test-corpus`. The `@auto` tree comparison needs a
Python Leo: see `docs/dev/compare-importers.py`.

## Layout

```
crates/leolib     the model. No view, ever.
crates/leotui     the terminal front end.
```

`leolib` has one runtime dependency for XML parsing (`quick-xml`), one for
regular expressions (`regex`), and `once_cell`. `leotui` adds `ratatui` and
`crossterm`.

## Using leolib

```rust
let mut outline = leolib::open_outline("myfile.leo", true)?;
for p in outline.all_unique_positions() {
    println!("{}", p.h(&outline));
}
let root = outline.root_position().unwrap();
outline.set_body(&root, "edited with no window in sight\n");
leolib::save(&mut outline, "")?;
leolib::write_external_files(&mut outline, true);
```

`leolib::Document` adds an undo history and the structural commands
(insert, delete, clone, copy, paste, move, mark) on top of an `Outline`.

## Using leotui

```
cargo run -p leotui -- FILE.leo
cargo run -p leotui -- FILE.leo --dump      # one frame to stdout, no terminal
```

| key | |
|---|---|
| `j` `k` arrows | move |
| `space` | fold or unfold |
| `right` `left` | unfold and descend, fold and ascend |
| `e` | edit the headline |
| `i` | edit the body (`^S` commits, `ESC` cancels) |
| `o` `D` | insert a node, delete a node |
| `u` `r` | undo, redo |
| `K` `J` `<` `>` | move the node up, down, left, right |
| `m` `c` `y` `P` | mark, clone, copy, paste |
| `s` `w` | write the `.leo` file, write the external files |
| `q` | quit |

Flags in the left column: `>` selected, `*` marked, `C` cloned, `~` dirty.

## `@auto`

An `@auto` file is the user's own source, with no sentinels in it. Its
structure comes from the language, through a port of Leo's importers.

| | |
|---|---|
| block languages | c, c++, c#, coffeescript, cython, dart, java, javascript, lua, pascal, perl, php, pug, python, rust, scheme, lisp/clojure, tcl, typescript |
| section languages | ini, xml, html |
| line-oriented | org, otl, markdown, treepad |

The file is regenerated from the tree alone, so an importer that dropped a
line would overwrite the user's source. Every import is therefore checked: the
tree is written back and compared with the file before it is kept, and a tree
that fails leaves the whole file in the node's body with an error. Leo does
not check this.

Two things an import can change even when it succeeds, both as in Leo:
leading tabs become blanks to match `@tabwidth`, and an XML or HTML file gets
adjacent tags split onto separate lines. `ReadResult::warnings` names the
files it happened to, because the next write changes them on disk.

`@auto-rst` is not ported: its reader and writer are a separate mechanism in
Leo, not an importer.

## What else is not ported

- **`@shadow`.** Deprecated in Leo.
- **Unknown attributes are opaque.** Leo pickles them. They round-trip as the
  hex strings the file spells, and are written back unchanged.
- **`.leojs`** (the JSON outline format).

See `docs/dev/porting-notes.md` for the places this port deliberately differs
from Leo, and why.

## Building

```
make build      # cargo build --workspace
make test       # cargo test --workspace
make test-corpus CORPUS=/path/to/LeoPyRef.leo
make lint       # rustfmt --check and clippy -D warnings
```
