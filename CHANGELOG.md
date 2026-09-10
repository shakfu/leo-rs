# Changelog

Earlier changes are recorded in the git history and in
`docs/dev/tui-design.md`.

## Unreleased

### Added

- **Tree-sitter highlighting** for C, C++, CSS, Go, HTML, Java, JavaScript,
  JSON, Python, Rust, shell and TypeScript. A parse tree tells a function from
  a type from a field, which the keyword tables cannot. Tree-sitter over
  syntect: its capture names are the scopes Helix themes use. Every other
  language keeps the line scanner. Leo directives and whole-line section
  references are blanked before parsing; left in, they made the parser misread
  the lines after them. The release binary grows from 2.8MB to 12MB.

- **Colours from Helix themes.** Theme files are read from
  `~/.config/leotui/themes`, then `~/.config/helix/themes`, then
  `$HELIX_RUNTIME/themes`, and none is vendored. Helix keys a theme by
  tree-sitter capture name, so a theme applies to the highlighter's classes
  as it is. The default is `sonokai`; without it, the terminal's 16 colours
  are used.

- **Truecolor, with a fallback.** Colours are reduced to the 256-colour cube or
  to the terminal's 16 when `COLORTERM` and `TERM` call for it, and
  `:set colors=true|256|16` overrides the guess. The nearest colour is measured
  in CIELAB. In RGB, grey is nearest to anything unsaturated, and sonokai's
  keywords came out grey.

- **`:theme`** names the current theme, and `:theme NAME` changes it. Matching
  names are listed above the command line. Tab and the arrow keys move through
  them, applying each as they land on it, and Escape puts back the theme you
  started with.

- **`~/.config/leotui/config.toml`**, holding `theme`. Accepting `:theme NAME`
  saves it there, rewriting only that line; `--theme NAME` sets a theme for one
  launch without saving. The file is TOML, read by a hand parser: the format
  has three shapes, against five crates for `toml` and `serde`.

- **`leolib::open_outline_with_report`**, which returns the external-file read
  errors that `open_outline` drops. `Document::open` keeps them in
  `read_report`.

### Changed

- `function.builtin`, `type.builtin` and `constant.builtin` are separate
  classes. Of the 31 installed Helix themes that name both of the first two,
  26 give them different colours. tree-sitter-rust tags numeric literals
  `constant.builtin`; they are tagged again as numbers, so a Rust `1` has the
  colour of a Python `1`.

- The body's colouring is kept between redraws and recomputed only when the
  text or the language changes. Parsing a 5,000-line `@edit` body took 23ms
  per frame.

- `demo/fsm.py` is Python 3, and is the `@file` example in `demo/demo.leo`.

### Fixed

- **An `@file` node over a file it failed to read could overwrite that file.**
  Reading recorded the path as read before checking that the read had worked,
  which opened the overwrite guard. `open_outline` also discarded the error, so
  the node only looked empty. After an edit, `w` would have replaced the file
  with sentinels around that edit. A path now counts as read only once its node
  holds the file, and the first read error shows on the status line at startup
  and on `:e`.

- Rust string literals were not coloured, and `//` inside one started a comment
  that ran to the end of the line. The highlighter took its string delimiters
  from the importers, and the Rust importer's list is empty on purpose. The
  highlighter now has its own table, which also covers nested block comments
  (Rust, Scala, D, Dart, Haskell, OCaml, Scheme) and languages that double a
  quote instead of escaping it (SQL, Pascal, Fortran, Ada, VBScript). A
  backslash inside a block comment no longer escapes its close.

- `@others` and `@all` are recognised when indented, as Leo's `directiveKind4`
  allows. An indented `@others` was coloured as a Python decorator.

- `@language` inside a `@nocolor` block no longer ends the block.
