# Changelog

Earlier changes are recorded in the git history and in `docs/dev/tui-design.md`.

## Unreleased

### Fixed

- **An external file that is not UTF-8 is no longer rewritten as UTF-8.** The reader decoded every external file with `from_utf8_lossy`, so a latin-1 file arrived with U+FFFD in place of each high byte, and the writer ignored the encoding it was handed. Editing any node in such a tree and saving replaced the user's source -- `name = 'caf\xe9'` became `name = 'caf\xef\xbf\xbd'` -- and neither the read nor the write reported anything.

  Leo decodes with the file's own encoding and encodes with it again, from Python's `codecs`. Rust's standard library gives only UTF-8, so matching Leo means a new dependency; refusing does not. Such a file is now reported in `ReadResult::errors` and left unread, which leaves `may_overwrite` to refuse the write. `@nosent` is never read and `@clean` is exempt from `may_overwrite`, so the write side checks too, and the file is kept out of `WriteResult::refused`: approving that prompt would write UTF-8 over the bytes. `atfile_read::read_into_root` returns `Result<()>` rather than `bool`, to carry the reason.

- **A `.leo` file that is not UTF-8 no longer opens.** It was decoded the same lossy way, where the loss is worse: the replacement lands in a headline or a body, and the next save writes it over the outline itself. Opening now fails with `Error::NotUtf8`. Leo reads those bytes with an XML parser, which honours the encoding in the prolog. `write_leo_file` refuses a `leo_file_encoding` it cannot produce, since the prolog copies that name while the bytes are always UTF-8.

### Changed

- **One error type, `leolib::Error`.** The crate answered with three: `Box<dyn Error>` from `open_outline` and `Document::open`, `LeoFileError` from the `.leo` reader, and `String` everywhere else. A caller could not tell a missing file from a failed importer from a refused overwrite without matching on message text. `LeoFileError` is gone; its variants are `Error::NotALeoFile` and `Error::BadXml`. `external::FileReport` carries the error itself rather than a rendered string, so a front end can offer a prompt for `RefusedOverwrite` and nothing at all for `UnsupportedEncoding`. Warnings, which are not errors, move to `external::FileNote`.

- **`Outline::scan_from` walks instead of listing.** `next_marked`, `prev_marked` and `next_clone` built `all_positions()` and searched it: 665us per call on an 11,598-position outline, per keystroke. Stepping with `thread_next`/`thread_back` and stopping where it started takes 27ns when the match is a row away, 304us when there is none at all. `Outline::position_is_linked` is new, and rejects a position the walk could not come back to.

- `corpus.rs`'s tangle test skips files the read reported unread. They have nothing in the outline to reproduce.

### Added

- **GitHub Actions.** `make check` on push and pull request, `make corpus` against a pinned leo-editor commit, and `cargo audit` weekly. Nothing ran the checks the Makefile had.

- **Corpus case `unreadable`**: an `@file` whose file has no sentinels, which both implementations report unread, beside an `@clean` file that reads. No case had an unread file, so nothing checked that Python and Rust agree on which files are unread.

## 0.2.1

The crates.io 0.2.0 was built from `f4adad3`, not the `0.2.0` tag. It already contains every change below except the `quick-xml` and `ratatui` upgrades and `make audit`.

### Security

- **`quick-xml` 0.37 to 0.42**, for [RUSTSEC-2026-0194](https://rustsec.org/advisories/RUSTSEC-2026-0194): the duplicate-attribute check ran in quadratic time, so a `.leo` file with many attributes on one element could stall the load. [RUSTSEC-2026-0195](https://rustsec.org/advisories/RUSTSEC-2026-0195) is fixed by the same version; it is in `NsReader`, which `leolib` does not use. The reader returns the same text as before: entity references are unescaped as 0.37 did, and `\r\n` in bodies and whitespace in attribute values are kept, not normalized.

- **`ratatui` 0.29 to 0.30**, with `crossterm` 0.28 to 0.29 to match its backend. 0.29 pulled in `lru` 0.12, unsound under [RUSTSEC-2026-0253](https://rustsec.org/advisories/RUSTSEC-2026-0253) and [RUSTSEC-2026-0002](https://rustsec.org/advisories/RUSTSEC-2026-0002), and the unmaintained `paste`. No leotui code changed.

### Added

- **`make audit`** runs `cargo audit` against `Cargo.lock`. It is kept out of `make check`, as it fetches the advisory database.


- **`Ctrl-w <` and `Ctrl-w >`**, vim's window-width keys, narrow and widen the pane that has focus. macOS takes `Ctrl-Left` and `Ctrl-Right` for Mission Control, so those never reached leotui there.

- **`:bufdo %s/pattern/replacement/[flags]`**, vim's `:bufdo` with each node's body a buffer: `:s` in every body, as one undo step. A clone's body is changed once. Headlines are left alone, as a buffer's name is in vim.

- An install section in the README.

### Changed

- `rust-version` is 1.90, up from 1.75. The dependencies already needed it: `tree-sitter-language` 1.90, `ratatui` 0.30 1.88, `quick-xml` 0.42 1.86. One workspace value over per-crate values, though `leolib` alone needs only 1.86.

- Both crates take `repository` and `readme` from `[workspace.package]`, so crates.io shows the README and links the repository. The README's screenshot link is absolute, as the image is in neither package.

- `tests/readme.rs` is excluded from the `leotui` package. It reads `../../README.md`, which an unpacked crate does not have.

### Fixed

- `cargo publish` refused `leotui`: its `leolib` dependency had a path and no version. The version is set in `[workspace.dependencies]`, beside the workspace version, so a bump edits one file.

- The 0.2.0 entry gives the outline pane's default width as 30%; 0.2.0 shipped with 35%.

- `demo/fsm.py`'s shebang is on line 1, through `@first`. It sat on line 3, below the sentinel header, so the file could not run as `./fsm.py`.

## 0.2.0

### Added

- **Tree-sitter highlighting** for C, C++, CSS, Go, HTML, Java, JavaScript, JSON, Python, Rust, shell and TypeScript. A parse tree tells a function from a type from a field, which the keyword tables cannot. Tree-sitter over syntect: its capture names are the scopes Helix themes use. Every other language keeps the line scanner. Leo directives and whole-line section references are blanked before parsing; left in, they made the parser misread the lines after them. The release binary grows from 2.8MB to 12MB.

- **Colours from Helix themes.** Theme files are compatible with helix theme files and are read from `~/.config/leotui/themes`, then `~/.config/helix/themes`, then `$HELIX_RUNTIME/themes`, and none are vendored. Helix keys a theme by tree-sitter capture name, so a theme applies to the highlighter's classes as it is. The default is `sonokai`; without it, the terminal's 16 colours are used.

- **Truecolor, with a fallback.** Colours are reduced to the 256-colour cube or to the terminal's 16 when `COLORTERM` and `TERM` call for it, and `:set colors=true|256|16` overrides the guess. The nearest colour is measured in CIELAB. In RGB, grey is nearest to anything unsaturated, and sonokai's keywords came out grey.

- **`:theme`** names the current theme, and `:theme NAME` changes it. Matching names are listed above the command line. Tab and the arrow keys move through them, applying each as they land on it, and Escape puts back the theme you started with.

- **`~/.config/leotui/config.toml`**, holding `theme` and `split-ratio`. Accepting `:theme NAME` saves it there, rewriting only that line; `--theme NAME` sets a theme for one launch without saving. `split-ratio = N` is the outline pane's width in percent, 15 to 85, and `:set split=N` saves it. The file is TOML, read by a hand parser: the format has three shapes, against five crates for `toml` and `serde`.

- **`w` asks before overwriting a file this outline has not read.** It used to refuse with no way to approve, so a node renamed from `@auto` to `@file` could never be written. `y` writes the file; any other answer leaves it untouched. `WriteResult::refused` lists the refused nodes for other callers.

- **`:import-at-file PATH`** imports a file as an `@file` tree in one step; Leo's `import-file` makes an `@auto` node, and converting it takes three. A file with sentinels is read as it is. Any other file is split by its `@auto` importer, or kept whole in one body when the tree would not write it back unchanged. A leading `#!` or coding line gets `@first`, so it stays on line 1, and a missing final newline is added, as the `@file` writer adds one anyway. The sentinels are written only after the overwrite prompt; `n` keeps the node, and `w` asks again. A file that is not UTF-8 is refused, since sentinels would corrupt a binary.

- **`:[range]s/pattern/replacement/[flags]`**, vim's substitute, on the current node's body as one undo step. The pattern is a `regex` crate regex rather than vim's dialect, with smartcase as `/` has; an empty pattern reuses the last search. Ranges are `%`, `.`, `$`, `N` and `N,M`; flags are `g`, `i`, `I` and `n`. `c` is refused: confirming each match needs a prompt loop the command line does not have.

- **Search highlighting**, vim's `hlsearch`: every match in the outline and in the visible body. `:noh` or `:nohlsearch` clears it until the next search or `n`.

- **`leolib::open_outline_with_report`**, which returns the external-file read errors that `open_outline` drops. `Document::open` keeps them in `read_report`.

- **`leolib::external::write_files`**, which writes the given `@<file>` nodes, and **`Document::import_at_file`**, the library side of `:import-at-file`.

### Changed

- `function.builtin`, `type.builtin` and `constant.builtin` are separate classes. Of the 31 installed Helix themes that name both of the first two, 26 give them different colours. tree-sitter-rust tags numeric literals `constant.builtin`; they are tagged again as numbers, so a Rust `1` has the colour of a Python `1`.

- **The `@file` reader caches its compiled sentinel regexes**, one set per comment-delimiter pair, shared by every file. It compiled all 12 twice per file, and compiling was 80% of the load. Loading `LeoPyRef.leo` and its external files now takes 88ms, down from 779ms; leo-editor takes 484ms, 317ms of it in `openLeoFile`. Leo also compiles them per file, but Python's `re` caches compiled patterns and the `regex` crate does not.

- The outline pane defaults to 30% of the width, down from 45%.

- **`/` and `?` search every headline and body**, in outline order, whichever pane has focus. From the outline they searched only headlines, and from the body only that node's text. A body match puts the cursor on it, and `n` and `N` step from match to match, wrapping with vim's "search hit BOTTOM" message. The pattern is a `regex` crate regex with smartcase, as `:s` uses, so `:s//x/` reuses it as typed; a capital after a backslash, as in `\S`, does not make it case sensitive. `:set search=headlines` keeps the old outline search.

- `:set` takes several options at once, as vim does, and reads `name:value` as `name=value`. `:set name?`, a number option named alone, or `:set` by itself shows values. The first option it does not understand stops the rest.

- The body's colouring is kept between redraws and recomputed only when the text or the language changes. Parsing a 5,000-line `@edit` body took 23ms per frame.

- `demo/fsm.py` is Python 3, and is the `@file` example in `demo/demo.leo`.

### Fixed

- **An `@file` node over a file it failed to read could overwrite that file.** Reading recorded the path as read before checking that the read had worked, which opened the overwrite guard. `open_outline` also discarded the error, so the node only looked empty. After an edit, `w` would have replaced the file with sentinels around that edit. A path now counts as read only once its node holds the file, and the first read error shows on the status line at startup and on `:e`.

- Rust string literals were not coloured, and `//` inside one started a comment that ran to the end of the line. The highlighter took its string delimiters from the importers, and the Rust importer's list is empty on purpose. The highlighter now has its own table, which also covers nested block comments (Rust, Scala, D, Dart, Haskell, OCaml, Scheme) and languages that double a quote instead of escaping it (SQL, Pascal, Fortran, Ada, VBScript). A backslash inside a block comment no longer escapes its close.

- `@others` and `@all` are recognised when indented, as Leo's `directiveKind4` allows. An indented `@others` was coloured as a Python decorator.

- `@language` inside a `@nocolor` block no longer ends the block.

## [0.1.0]

initial release
