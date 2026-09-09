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

Run those checks with `make test-corpus`.

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

## What is not ported

- **`@auto`.** Its files carry no sentinels, so their structure comes from one
  of Leo's 34 language importers. Reading one is refused rather than guessed:
  a wrong guess would silently rewrite the tree.
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
