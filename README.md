# leo-rs

A Rust implementation of [Leo](https://github.com/leo-editor/leo-editor)'s model layer (`leolib`) and a terminal front end that consumes it (`leotui`).

Leo's outline model was separated from its Qt front end in `leo/leolib`; this port keeps that boundary. `leolib` reads and writes `.leo` files and the external files they refer to, and knows nothing about how any of it is shown. `leotui` is one front end over that crate. Nothing in `leolib` depends on it.

![Screenshot the tui.](https://raw.githubusercontent.com/shakfu/leo-rs/main/docs/media/tui.png)

## Status

Verified against `leo/core/LeoPyRef.leo` from the Leo repository, at leo-editor `e3b3841f64`. The two `@auto` rows come from an earlier checkout; `TODO.md` records re-measuring them.

| check | result |
|---|---|
| nodes read from the `.leo` file | 538, identical gnx/headline/body to Python `leolib` |
| nodes read with all external files | 11,596, identical to Python `leolib` |
| `.leo` file rewritten | byte-identical to the file read |
| external files written | 383 of 383 byte-identical to the files on disk |
| `@auto` trees, 1,000 files across 8 languages | 998 identical to Leo's importers; the 2 differences are a deliberate fix |
| `@auto` files written back | 1,008 of 1,010 byte-identical; the 2 exceptions fail in Leo too |

Those figures come from runs against a leo-editor checkout. What `cargo test` checks every time is the conformance corpus in `demo/`: each outline there has an expected file written by Python Leo (`scripts/make_corpus.py`), and leo-editor checks Python Leo against a copy of the same files. One case per feature, indexed in `demo/README.md`. The `@auto` tree comparison needs a Python Leo: see `docs/dev/compare-importers.py`.

## Layout

```text
crates/leolib     the model. No view, ever.
crates/leolsp     language servers, with their positions mapped to nodes.
crates/leoapp     a front end's state and commands, with no renderer.
crates/leotui     the terminal front end: leoapp drawn with ratatui.
crates/leoegui    the desktop front end: leoapp drawn with egui.
```

`leolib` has one runtime dependency for XML parsing (`quick-xml`), one for regular expressions (`regex`), and `once_cell`. `leolsp` adds `lsp-types` and `serde_json`, `leoapp` the tree-sitter grammars. `leotui` adds `ratatui`, `crossterm` and `clap`; `leoegui` adds `eframe` and `clap`.

## Installing

```sh
cargo install leotui --locked      # from crates.io
cargo install --path crates/leotui --locked   # from a checkout
cargo add leolib                   # the library, in your own crate
```

`--locked` builds with the `Cargo.lock` shipped in the package. A checkout build uses the workspace's `lto = true`; the crates.io build does not, as cargo drops workspace profiles from a published package.

## Using leolib

```rust
let mut outline = leolib::open_outline("myfile.leo", true)?;
for p in outline.all_unique_positions() {
    println!("{}", p.h(&outline));
}
let root = outline.root_position().unwrap();
outline.set_body(&root, "edited with no window in sight\n");
let saved = leolib::save_all(&mut outline, "");   // the .leo file, then the dirty external files
saved.leo?;
```

`leolib::save` writes only the `.leo` file and `leolib::write_external_files` only the external files; `save_all` does both, in that order, and reports each in `SaveResult`.

`leolib::Document` adds an undo history and the structural commands (insert, delete, clone, copy, paste, move, mark) on top of an `Outline`.

Every fallible call answers with `leolib::Error`, whose variants are the distinctions a caller acts on: `NotFound`, `NotUtf8`, `UnsupportedEncoding`, `RefusedOverwrite`, `ChangedOnDisk`, `Import`, `Write`. Reading and writing the external files reports per file rather than failing the outline, in `ReadResult` and `WriteResult`.

### A `.leo` file names the paths it writes

An `@<file>` headline and an `@path` directive can name any path: an absolute one, or one that climbs out with `..`, with `~` expanded. So opening an outline and writing its external files can write anywhere the user can write, and a `.leo` file from someone else is as dangerous as a Makefile from someone else. Leo behaves the same way.

Three guards narrow this without closing it:

- `Outline::may_overwrite` refuses a file the outline has not read.

- A write refuses a file changed on disk since the outline read or wrote it (`Error::ChangedOnDisk`).

- A write refuses a directory that does not exist, unless `Config::create_nonexistent_directories` is set. Leo's default is the same.

None of them stops a new file in an existing directory. A front end handling untrusted outlines should check `Outline::full_path` against a directory of its own choosing before writing.

## Using leotui

```sh
leotui FILE.leo
leotui FILE.leo --dump              # one frame, no terminal
leotui FILE.leo --dump --press F1   # press keys, then dump
leotui --keys                       # the binding table
leotui --version
```

or during development

```sh
cargo run -p leotui -- FILE.leo
cargo run -p leotui -- FILE.leo --dump              # one frame, no terminal
cargo run -p leotui -- FILE.leo --dump --press F1   # press keys, then dump
cargo run -p leotui -- --keys                       # the binding table
```

leotui is modal. The pane decides what a key means -- Leo's own `!tree`/`!body` rule -- and `:` reaches every command by name, as Leo's minibuffer does. `F1` shows the bindings in the app; `leotui --keys` prints the same table.

| mode | how you get there | how you leave |
|---|---|---|
| `NORMAL` | the default | |
| `INSERT` | `i` `a` `I` `A` `o` `O` `s` `S` `c` `C` in the body | `Escape` commits |
| `VISUAL` | `v` `V` in the body | an operator, or `Escape` |
| `HEADLINE` | `e` in the outline | `Enter` commits, `Escape` abandons |
| `COMMAND` | `:` | `Enter` runs it, `Escape` abandons |
| `SEARCH` | `/` `?` | `Enter` keeps the match, `Escape` goes back |
| `HELP` | `F1` | `q` |

`Ctrl-c` in any mode keeps what you typed, closes what is open, and asks to quit if anything is unsaved. In a yes/no prompt it answers no.

In INSERT and every one-line input, `Ctrl-w` deletes the word before the cursor and `Ctrl-u` the text before it. Other Ctrl and Alt chords type nothing. While a headline is edited, a chord the outline binds keeps the headline as typed and acts on the node, as in Leo: `Ctrl-r` indents a new node before it has a name, `Ctrl-i` starts the next, and `Ctrl-u` moves the node up.

leoegui is the same editor in a window: the same keys, commands, settings and themes, drawn in a monospace grid. On macOS, Cmd is Leo's Ctrl, as in Leo: a Cmd chord the outline binds runs from either pane, so Cmd-R indents the node even in the body, and any other Cmd chord is Ctrl. Control keeps leotui's keys, vim's in the body. Leo's `qt-mac-dont-swap-ctrl-and-meta = true` in the settings leaves Cmd unbound. The outline and body take clicks and the wheel in NORMAL. View > Appearance picks dark, light, or the system's choice, saved as `appearance = "dark" | "light" | "system"`; the dark theme is `theme` and the light one `theme-light` (default `onelight`), and `:theme` sets whichever is showing. View > Theme... lists your Helix themes as dark or light and previews each under the pointer; the window's parts take the Helix scopes for them, such as `ui.statusline.insert` for the INSERT badge and `diagnostic.warning` for a warning's underline. A yes/no dialog answers to `y` or `n` alone.

```sh
make gui FILE=FILE.leo                                   # a release build
cargo run -p leoegui -- FILE.leo
cargo run -p leoegui -- FILE.leo --press F1 --screenshot out.ppm   # one frame, then exit
```

### Cheatsheet

<!-- keys:begin -->

**Outline: moving**

| | |
|---|---|
| `j` `k` `Down` `Up` `Alt-Down` `Alt-Up` | next, previous visible node |
| `h` `Left` `Alt-Left` | fold this node, or step out to the parent |
| `l` `Right` `Enter` `Alt-Right` | unfold this node, or step in to the first child |
| `gg` `Alt-Home` | first node |
| `G` `Alt-End` | last visible node |
| `gp` | parent |
| `{` `}` | previous, next sibling |
| `[m` `]m` | previous, next marked node |
| `]c` `Alt-n` | next clone of this node |
| `H` `L` | back, forward through the nodes selected |

**Outline: folding**

| | |
|---|---|
| `Space` `za` | fold or unfold this node |
| `zh` `zH` | hoist this node, dehoist (`:clear-all-hoists` undoes every hoist) |
| `zo` `Alt-]` | unfold this node |
| `zc` `Alt-[` | fold this node |
| `zR` | unfold every node |
| `zM` `Alt--` | fold every node |
| `zr` `zm` | unfold one level further, fold one level back |
| `zx` | fold everything except the path to this node |
| `z1` `z2` `z3` `z4` `z5` `z6` `z7` `z8` `z9` | unfold to that level |

**Outline: moving a node**

| | |
|---|---|
| `>>` `Shift-Right` | **indent**: make this node a child of the one above |
| `<<` `Shift-Left` | **deindent**: move this node out one level |
| `J` `Shift-Down` | move this node down |
| `K` `Shift-Up` | move this node up |
| `Ctrl-r` `Ctrl-l` `Ctrl-u` `Ctrl-d` | Leo's indent, deindent, up, down |
| `Alt-Shift-Right` `Alt-Shift-Left` `Alt-Shift-Up` `Alt-Shift-Down` | the same, from either pane |
| `g>` | demote: make the *following siblings* children of this node |
| `g<` | promote: make this node's *children* its siblings |

`>>` moves the node you are on. `g>` and `g<` move other nodes around it. Leo's `Ctrl-r`, `Ctrl-u` and `Ctrl-d` move nodes in the outline only: the body keeps vim's redo and half pages.

**Outline: creating and removing**

| | |
|---|---|
| `o` `Insert` `Shift-Insert` | insert a node after this one |
| `O` | insert a node before this one |
| `a` `Ctrl-Insert` | insert a node as the first child |
| `e` | edit the headline |
| `i` | edit the body |
| `dd` | cut this node to the clipboard |
| `Delete` `Backspace` | delete this node |
| `yy` `p` | copy, paste after this node |
| `Ctrl-Shift-c` `Ctrl-Shift-x` `Ctrl-Shift-v` | Leo's copy, cut, paste a node, from either pane |
| `Ctrl-Shift-d` | extract the selected lines into a child |
| `` ` `` | clone this node |
| `m` `M` | mark or unmark this node, clear every mark |
| `Alt-a` | sort this node and its siblings (`:sort-children` sorts its children) |

`:cfa pattern` (`:clone-find-all`) clones every node matching `pattern` under a new last top-level node, `Found N:pattern`, as Leo's `clone-find-all`. `:cff` (`:clone-find-all-flattened`) also searches below each match. The pattern is matched as `/` matches it; with none, the last search is used.

`:extract` moves the VISUAL lines, or the cursor's line, into a new first child, as Leo's `extract`: a `<< section >>` first line names the child and stays behind, a definition line names it, or else the first line does.

`:mark-subheads`, `:mark-node-and-parents`, `:unmark-node-and-parents`, `:clone-marked-nodes`, `:copy-marked-nodes`, `:move-marked-nodes` and `:delete-marked-nodes` are Leo's commands of those names. Each is one undo step; Leo cannot undo `move-marked-nodes`.

**Body: a vim buffer**

| | |
|---|---|
| `h j k l` | left, down, up, right |
| `w W b B e E ge` | by word: forwards, back, to the end |
| `0 ^ $ gg G { } %` | line start and end, file, paragraph, matching bracket |
| `f F t T ; ,` | to a character on the line, and repeat |
| `H M L` | top, middle, bottom of the pane |
| `d c y > < gu gU g~` | delete, change, yank, indent, unindent, case |
| `iw aw i" a( ip` | word, quoted, bracketed, paragraph |
| `x X r s S D C Y J ~` | delete, replace, substitute, join, case |
| `i a I A o O` | enter INSERT at the usual vim place |
| `v V` | select charwise, linewise |
| `p P` | put the text register after, before |
| `.` | repeat the last change |
| `gd` | go to the node defining the `<< section >>` on this line |
| `K` | what the language server says of the symbol under the cursor |
| `Ctrl-]` | go to its definition |
| `]d` `[d` | next, previous diagnostic |

`:reformat-paragraph` wraps the paragraph at the cursor to `@pagewidth`, as Leo's command of that name, and moves to the next paragraph.

Operators take a count, a motion and a text object: `2d3w`, `ciw`, `da"`, `>>`. One change is one undo, so `A`, two hundred characters and `Escape` is one `u`.

**Both panes**

| | |
|---|---|
| `Tab` `Shift-Tab` | move between the outline and the body |
| `Escape` | in the body, go back to the outline |
| `:` | the command line |
| `/` `?` | search headlines and bodies, forwards, backwards |
| `n` `N` | next match, previous match |
| `u` `Ctrl-z` | undo |
| `Ctrl-r` | redo, in the body; in the outline, Leo's indent |
| `Ctrl-s` | write the `.leo` file, then the changed external files |
| `w` | write the changed external files |
| `Ctrl-f` `Ctrl-b` `PageDown` `PageUp` | a screen down, up |
| `Ctrl-d` `Ctrl-u` | half a screen down, up, in the body |
| `Shift-PageDown` `Shift-PageUp` | half a screen down, up, in the outline, as Leo's |
| `Alt-d` `Alt-t` `Ctrl-t` | Leo's: focus the body, the outline, the other pane |
| `Ctrl-g` | Leo's keyboard-quit: Escape for whatever is being typed |
| `Ctrl-w <` `Ctrl-w >` | narrow, widen the pane that has focus |
| `Ctrl-Left` `Ctrl-Right` | give the body, the outline more room |
| `:set syntax` `:set nosyntax` | colour the body, or leave it plain |
| `Alt-g` | go to line N of the external file (`:goto-global-line N`) |
| `F1` | help |
| `q` | quit |

**The `:` command line**

Every Leo command name, with Tab completion and Up/Down history, plus the vim spellings: `:w` `:w path` `:saveas path` `:q` `:q!` `:wq` `:x` `:e path` `:e!` `:h cmd`. `:N` selects the Nth visible row.

`Ctrl-s` and `:w` write the `.leo` file, then every dirty external file, as Leo's `save` does. A file that cannot be written, such as one with an orphan node, does not stop the others. It stays dirty, and the next save tries it again. If the `.leo` file is not saved, no external file is written. `:write-outline-only` writes the `.leo` file alone, and `w` the external files alone.

`:write-at-file-nodes` writes every `@<file>` node at or under the selection, dirty or not, as Leo's command of that name; `w` is `:write-dirty-at-file-nodes`.

`:w path` writes a copy and keeps editing this outline. `:saveas path` moves the outline there, which also moves where relative `@file` paths are written. Both refuse an existing file until given `!`.

`q`, `:q` and `:e` ask or refuse while anything is unsaved, including an `@file` tree a save could not write. `:e!` opens the `.leo` file again, discarding every change. A path with no file there, given to `:e` or on the command line, starts a new outline that the first save creates.

`:refresh-from-disk` reads the `@<file>` node at or above the selection from disk again; `:read-at-file-nodes` reads every one at or under it. `:read-at-file-nodes` skips an `@clean` file unchanged since it was last read or written; `:refresh-from-disk` reads it anyway. Both ask before discarding unwritten edits, and both clear the undo history, as in Leo. When the terminal regains focus, the status line names any external file changed on disk, and a write asks before overwriting it.

`:set` takes several options at once, as vim does: `:set search=all|headlines split=N wrap number syntax colors=true|256|16|none`. `name:value` works as `name=value`, and `:set name?` or `:set` alone shows values. `:set split=N` sets the outline's width in percent, and saves it as `split-ratio` in `~/.config/leotui/settings.toml`.

`/` searches every headline and body in outline order, whichever pane has focus, and lands on the match: a headline in the outline, body text under the body's cursor. The pattern is a Rust `regex`, with smartcase. Matches stay highlighted until `:noh`, and `:set search=headlines` leaves bodies out.

`:import-at-file path` imports a file as an `@file` tree, and asks before writing sentinels into it.

`:goto-global-line N` selects the node that writes line N of the selection's `@file`, `@clean`, `@edit` or `@asis` file, with the cursor on that line. `:show-file-line` is the reverse, for the cursor's line.

`:messages` lists the status messages shown so far, including every external file a read reported.

`:[range]s/pattern/replacement/[flags]` substitutes in the current node's body, as one undo step. The pattern is a Rust `regex`, with smartcase as in `/`; an empty pattern reuses the last search. The replacement takes `&`, `\1`-`\9` and `\r`. Ranges are `%`, `.`, `$`, `N` and `N,M`; flags are `g`, `i`, `I` and `n`. `:bufdo %s/pattern/replacement/[flags]` does the same in every node's body, as one undo step.

**Leo's own chords**

These need a terminal speaking the kitty keyboard protocol (kitty, foot, wezterm, ghostty, alacritty, iTerm2). A legacy terminal cannot send them -- `Ctrl-I` *is* Tab -- so each has a portable binding above. `--no-kitty-keys` turns the protocol off.

| | |
|---|---|
| `Ctrl-i` | insert a node |
| `Ctrl-m` | mark |
| `Ctrl-[` `Ctrl-]` `Ctrl-{` `Ctrl-}` | promote, demote |
| ``Ctrl-` `` | clone |
| `Ctrl-Shift-z` | redo |
| `Ctrl-h` | edit the headline |

<!-- keys:end -->

The body is coloured by the language declared at the node: an `@language` directive in the node or an ancestor, or the nearest `@<file>` node's extension. A node with neither is left plain, so prose is never coloured as code. `@language` lines inside a body move it from that line on, so one node can hold Python and then C; `@nocolor`, `@color` and `@killcolor` work as they do in Leo. `:set nosyntax` turns it off.

Twelve languages -- C, C++, CSS, Go, HTML, Java, JavaScript, JSON, Python, Rust, shell, TypeScript -- are parsed with tree-sitter, which tells a function from a field from a type. Every other language Leo knows a comment delimiter for runs a line scanner instead: comments, strings, numbers, and keywords from Leo's colorizer modes for 33 of them.

Colours come from a Helix theme, read from `~/.config/leotui/themes` or `~/.config/helix/themes`. Nothing is vendored, so the themes are whichever ones you already have. The default is `sonokai`, and without a file of that name leotui uses the terminal's sixteen colours.

`:theme` names the current one. `:theme NAME` changes it, and the themes on disk are listed above the command line as you type. Tab and the arrow keys move through the list, applying each as they land on it, so the outline shows the theme before Enter accepts it. Escape puts back the one you started with. Enter saves the choice to `~/.config/leotui/settings.toml`, rewriting only its `theme` line, and the next launch starts there. `--theme NAME` picks a theme for one launch without saving it.

Truecolor is used where the terminal reports it, and reduced to the 256-colour cube or the terminal's sixteen where it does not; `:set colors=true|256|16` overrides the guess. A non-empty `NO_COLOR` turns colour off, as `:set colors=none` does; the selected row and the status line are then shown reversed.

The outline's selected row, marked and `@<file>` nodes and pane borders take the theme's `ui.menu.selected`, `ui.selection`, `warning`, `ui.text.directory`, `ui.text.focus` and `ui.window` scopes, so a light theme draws them for a light background.

Flags in the left column: `>` selected, `*` marked, `C` cloned, `~` dirty. `@<file>` nodes are green. The design, and what is still to come, is in `docs/dev/tui-design.md`.

### Settings

Both front ends read `~/.config/leotui/settings.toml` (an older `config.toml` there is renamed to it). leoegui edits it in File > Settings... (Cmd-,), which writes only the keys that changed and keeps comments and keys it does not know. The keys:

| key | what it does |
|-|-|
| `theme` `theme-light` `appearance` | the dark and light themes, and which is shown: `dark`, `light` or `system` |
| `number` `wrap` `syntax` | what `:set number`, `wrap` and `syntax` start as |
| `split-ratio` | the outline's share of the width, in percent |
| `lsp` | `false` to start no language server |
| `lsp-LANGUAGE` | the command of the server for Leo's language `LANGUAGE` |
| `mcp` `mcp-edit` `mcp-save` | the MCP server, and whether its clients may edit and save |
| `mcp-port` `mcp-token` | where it listens on 127.0.0.1, and the token a client sends |
| `qt-mac-dont-swap-ctrl-and-meta` | Leo's: on macOS, Cmd is Meta rather than Leo's Ctrl |

### MCP

With `mcp = true`, the running leotui or leoegui serves its open outline to MCP clients at `http://127.0.0.1:PORT/mcp` (port 7341 by default), over MCP's streamable HTTP transport. A client must send `Authorization: Bearer TOKEN`; the token is made the first time MCP is turned on, and the Settings dialog shows the command that connects Claude Code:

```sh
claude mcp add --transport http leo http://127.0.0.1:7341/mcp --header "Authorization: Bearer TOKEN"
```

A client can read until the settings say more: `outline`, `read_node`, `search` and `selection` always; `set_headline`, `set_body`, `insert_node`, `delete_node`, `move_node` and `select_node` with `mcp-edit = true`; `save` with `mcp-save = true` as well. Nodes are named by gnx. Each edit is one undo step the user can take back with `u`, and the status line says what the client did; an edit waits while the user is typing. Only 127.0.0.1 is listened on, and a request whose `Host` or `Origin` is not local is refused, so a web page cannot reach the outline.

### Language servers

A server starts only for a language the settings name, one line each, and `lsp = false` turns them all off:

```toml
lsp-python = "pylsp"
lsp-c = "clangd"
lsp-rust = "rust-analyzer"
```

None is started otherwise: a server runs code from the project around the outline, and opening a `.leo` file should not choose that code.

A server sees each external file as leolib writes it, so diagnostics, hover, definitions and renames work across the nodes of an `@file`, `@clean`, `@nosent` or code `@auto` tree. A node in no file is a document of its own. Diagnostics are underlined in the body, and the one on the cursor's line is on the status line; the servers hear an edit when INSERT commits it. `:lsp-diagnostics` lists the body's, and `:lsp-rename NAME` renames the symbol under the cursor in every node at once, as one undo. A rename that would touch a sentinel line, a file the outline does not hold, or text changed since the request is refused whole.

## `@auto`

An `@auto` file is the user's own source, with no sentinels in it. Its structure comes from the language, through a port of Leo's importers.

| | |
|---|---|
| block languages | c, c++, c#, coffeescript, cython, dart, java, javascript, lua, pascal, perl, php, pug, python, rust, scheme, lisp/clojure, tcl, typescript |
| section languages | ini, xml, html |
| line-oriented | org, otl, markdown, treepad |

The file is regenerated from the tree alone, so an importer that dropped a line would overwrite the user's source. Every import is therefore checked: the tree is written back and compared with the file before it is kept, and a tree that fails leaves the whole file in the node's body with an error. Leo does not check this.

Two things an import can change even when it succeeds, both as in Leo: leading tabs become blanks to match `@tabwidth`, and an XML or HTML file gets adjacent tags split onto separate lines. `ReadResult::warnings` names the files it happened to, because the next write changes them on disk.

An extension with no importer is read whole into the node's body, as in Leo. `@auto-rst`, and `@auto` on `.rst`, are not ported: Leo reads them with an importer this port lacks and writes them with a separate mechanism, so they are reported unread.

## What else is not ported

- **`@shadow`.** Deprecated in Leo.

- **`@jupytext`.** Leo converts the notebook with the jupytext package. Such a node is refused on read and write.

- **Unknown attributes are opaque.** Leo pickles them. They round-trip as the hex strings the file spells, and are written back unchanged.

- **Encodings other than UTF-8.** Leo decodes an external file with the encoding its `@encoding` directive or `@+leo` header names, and encodes it with the same one on the way out. This port reads and writes UTF-8 only, so a file in any other encoding is reported unread and is never written: writing it would replace its bytes with UTF-8 and lose every character the two encodings spell differently. The node keeps whatever the `.leo` file said. A `.leo` file that is not UTF-8 is refused outright, since there is no part of it to keep.

- **`.leojs`** (the JSON outline format).

See `docs/dev/porting-notes.md` for the places this port deliberately differs from Leo, and why.

## Building

```text
make build      # cargo build --workspace
make test       # cargo test --workspace
make corpus LEO_EDITOR=/path/to/leo-editor   # demo/'s expected files against Python Leo
make lint       # rustfmt --check and clippy -D warnings
make check      # lint, then test
make audit      # Cargo.lock against the RustSec advisories (cargo-audit)
```

`.github/workflows/ci.yml` runs `make check` and `make corpus` on every push; the corpus job pins the leo-editor commit its expected files came from. `make audit` is not in CI, as it fetches the RustSec database.
