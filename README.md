# leo-rs

A minimal Rust implementation of [Leo](https://github.com/leo-editor/leo-editor)'s model layer (`leolib`), with two frontends using it: `leotui` in the terminal and `leogui` on the desktop.

Leo's outline model was re-implemented in rust as `leolib`. `leolib` reads and writes `.leo` files and the external files they refer to, and knows nothing about how any of it is shown. Nothing in `leolib` depends on a frontend. The frontends share `leoapp`, which holds the editor's state, commands and keys, so leotui and leogui take the same keys, commands, settings and themes.

![leogui, the desktop front end.](https://raw.githubusercontent.com/shakfu/leo-rs/main/docs/media/gui.png)

![leotui, the terminal front end.](https://raw.githubusercontent.com/shakfu/leo-rs/main/docs/media/tui.png)

## Installing

```sh
cargo install leotui --locked                  # leotui, from crates.io
cargo install --path crates/leotui --locked    # leotui, from a checkout
cargo install --path crates/leogui --locked    # leogui and leogui-glow, from a checkout
cargo add leolib                               # the library, in your own crate
```

`--locked` builds with the `Cargo.lock` shipped in the package. A checkout build uses the workspace's `lto = true`; the crates.io build does not, as cargo drops workspace profiles from a published package.

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

In INSERT and the `:` and `/` lines, `Ctrl-w` deletes the word before the cursor and `Ctrl-u` the text before it. Other Ctrl and Alt chords type nothing. While a headline is edited, a chord the outline binds keeps the headline as typed and acts on the node instead, as in Leo: `Ctrl-r` indents a new node before it has a name, `Ctrl-i` starts the next, and `Ctrl-u` moves the node up. A new outline's `newHeadline` starts selected, so typing replaces it.

### Cheatsheet

The keys are the same in leogui.

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
| `Ctrl-o` | back, in either pane, as vim's jump back: after following a link, to where it was |

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
| `gr` | clone every node using it under a `Found` node |
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
| `:set semantic` `:set nosemantic` | colour by the language server's semantic tokens as well, or by tree-sitter alone |
| `:set list` `:set guides` | mark spaces and tabs; draw indent guides (leogui) |
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

`:set` takes several options at once, as vim does: `:set search=all|headlines split=N wrap number syntax semantic list guides colors=true|256|16|none`. `name:value` works as `name=value`, and `:set name?` or `:set` alone shows values. `:set split=N` sets the outline's width in percent, and saves it as `split-ratio` in `~/.config/leo-rs/settings.toml`.

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

### Colouring and themes

The body is coloured by the language declared at the node: an `@language` directive in the node or an ancestor, or the nearest `@<file>` node's extension. A node with neither is left plain, so prose is never coloured as code. `@language` lines inside a body move it from that line on, so one node can hold Python and then C; `@nocolor`, `@color` and `@killcolor` work as they do in Leo. `:set nosyntax` turns it off.

Twelve languages -- C, C++, CSS, Go, HTML, Java, JavaScript, JSON, Python, Rust, shell, TypeScript -- are parsed with tree-sitter, which tells a function from a field from a type. Every other language Leo knows a comment delimiter for runs a line scanner instead: comments, strings, numbers, and keywords from Leo's colorizer modes for 33 of them.

Colours come from a Helix theme, read from `~/.config/leo-rs/themes` or `~/.config/helix/themes`. Nothing is vendored, so the themes are whichever ones you already have. The default is `sonokai`, and without a file of that name leotui uses the terminal's sixteen colours. leogui adds a light theme and a picker: see [Using leogui](#using-leogui).

`:theme` names the current one. `:theme NAME` changes it, and the themes on disk are listed above the command line as you type. Tab and the arrow keys move through the list, applying each as they land on it, so the outline shows the theme before Enter accepts it. Escape puts back the one you started with. Enter saves the choice to `~/.config/leo-rs/settings.toml`, rewriting only its `theme` line, and the next launch starts there. `--theme NAME` picks a theme for one launch without saving it.

Truecolor is used where the terminal reports it, and reduced to the 256-colour cube or the terminal's sixteen where it does not; `:set colors=true|256|16` overrides the guess. A non-empty `NO_COLOR` turns colour off, as `:set colors=none` does; the selected row and the status line are then shown reversed.

The outline's selected row, marked and `@<file>` nodes and pane borders take the theme's `ui.menu.selected`, `ui.selection`, `warning`, `ui.text.directory`, `ui.text.focus` and `ui.window` scopes, so a light theme draws them for a light background.

In leotui, flags in the left column: `>` selected, `*` marked, `C` cloned, `~` dirty. `@<file>` nodes are green. The design, and what is still to come, is in `docs/dev/tui-design.md`.

## Using leogui

```sh
leogui FILE.leo [MORE.leo...]      # a tab per outline
leogui                             # reopen the outlines open at the last quit
leogui --no-session                # neither restore nor save the session
leogui FILE.leo --press F1 --screenshot out.ppm   # press keys, save one frame, exit
```

or during development

```sh
make gui FILE=FILE.leo              # a release build; egui's debug build draws slowly
make gui-glow FILE=FILE.leo         # the same, drawn with OpenGL instead of wgpu
cargo run -p leogui -- FILE.leo
```

`--no-external` and `--theme NAME` work as in leotui. `leogui-glow` is the same program drawn with OpenGL. On Linux it reached its first frame in a third of the time, with 80 MB less memory (`docs/dev/gui-roadmap.md`, Performance 7); wgpu stays the default because OpenGL is deprecated on macOS.

leogui is the same editor in a window: the same keys, modes, commands, settings and themes, drawn in a monospace grid. The outline and body also take clicks and the wheel in NORMAL. A yes/no dialog answers to `y` or `n` alone.

On macOS, Cmd is Leo's Ctrl, as in Leo: a Cmd chord the outline binds runs from either pane, so Cmd-R indents the node even in the body, and any other Cmd chord is Ctrl. Control keeps leotui's keys, vim's in the body. `qt-mac-dont-swap-ctrl-and-meta = true` in the settings leaves Cmd unbound.

What the window adds to leotui:

| | |
|-|-|
| Several outlines | A tab each. File > Open..., Save As... (the system's dialogs) and Open Recent (the last ten). Quitting asks about each outline with unsaved work. |
| Session | Started with no outline named, leogui reopens the outlines open at the last quit, with their selections, tabs, panel and window size. |
| Go to node | Cmd-P: every headline, fuzzy-matched. |
| Command palette | Cmd-Shift-P: every command, by name. |
| Find panel | Cmd-Shift-F: find and replace, regex, whole word and case, over the outline, a subtree or the marked nodes; Find All, Replace All and Clone Find All. |
| External files | An `@<file>` row is badged unread, changed on disk, never read, or unwritten. A bar above the body offers Reload or Keep for a file changed on disk. |
| Clones and hoists | A cloned row shows its clone count, and its context menu lists the clones. A hoisted node is named above the outline, with a De-hoist button. |
| Several nodes | Cmd-click (Ctrl-click off macOS) adds a row, Shift-click the rows from the current one. Delete and Mark then act on every chosen row, and dragging one moves them all, each as one undo step; any other command acts on the current row alone and unchooses the rest. |
| The body | The bracket at the cursor and its match are shaded (`ui.cursor.match`). An `@pagewidth` directive draws a ruler at its column (`ui.virtual.ruler`). Indent guides mark each indent level (`ui.virtual.indent-guide`, `:set noguides`); `:set list` marks spaces and tabs (`ui.virtual.whitespace`). |
| Drag and drop | Dropping a `.leo` file opens it; dropping any other file imports it as `@auto`. |
| Language servers | Completion under the cursor, Cmd-. for code actions, and the Body menu for hover, definition, rename and problems. The status bar's LSP dot opens View > Language Servers. See [Language servers](#language-servers). |
| Bottom panel | View > Problems (the body's diagnostics), Log (the status messages), Find, and Language Servers (each server's state and log). |
| Rendered view | View > Rendered View: the selected node rendered beside the body, as Leo's `viewrendered` shows it. Markdown (`@language md` or a `@md` headline) with its tables, coloured code and images; an `@image` node's picture (the path on the body's first line). reStructuredText is shown as text. |
| Settings dialog | File > Settings... (Cmd-,). See [Settings](#settings). |

View > Appearance picks dark, light, or the system's choice, saved as `appearance = "dark" | "light" | "system"`. The dark theme is `theme` (default `sonokai`) and the light one `theme-light` (default `onelight`); `:theme` sets whichever is showing. View > Theme... lists your Helix themes as dark or light, and previews each under the pointer. The window's parts take the Helix scopes for them, such as `ui.statusline.insert` for the INSERT badge, `ui.menu.selected` for a list's selected item, and `diagnostic.warning` for a warning's underline.

## Settings

Both front ends read `~/.config/leo-rs/settings.toml`. leogui keeps its `session` and `recent-outlines` beside it, and both look for themes in `themes/` there. On first start, an older `~/.config/leotui/` directory is renamed to `leo-rs/`, and an older `config.toml` to `settings.toml`. leogui edits it in File > Settings... (Cmd-,), which writes only the keys that changed and keeps comments and keys it does not know. The keys:

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

## MCP

With `mcp = true`, the running leotui or leogui serves its open outline to MCP clients at `http://127.0.0.1:PORT/mcp` (port 7341 by default), over MCP's streamable HTTP transport. A client must send `Authorization: Bearer TOKEN`; the token is made the first time MCP is turned on, and the Settings dialog shows the command that connects Claude Code:

```sh
claude mcp add --transport http leo http://127.0.0.1:7341/mcp --header "Authorization: Bearer TOKEN"
```

A client can read until the settings say more: `outline`, `read_node`, `search` and `selection` always; `set_headline`, `set_body`, `insert_node`, `delete_node`, `move_node` and `select_node` with `mcp-edit = true`; `save` with `mcp-save = true` as well. Nodes are named by gnx. Each edit is one undo step the user can take back with `u`, and the status line says what the client did; an edit waits while the user is typing. Only 127.0.0.1 is listened on, and a request whose `Host` or `Origin` is not local is refused, so a web page cannot reach the outline.

## Language servers

No server starts unless the settings name one. A server runs code from the project around the outline, and opening a `.leo` file should not choose that code.

### Settings

| key | value |
|-|-|
| `lsp` | `false` to start no server, whatever the `lsp-LANGUAGE` keys say. Default `true`. |
| `lsp-LANGUAGE` | The command that starts the server for Leo's language `LANGUAGE`: a program and its arguments, split on blanks, with no shell quoting. An empty value is ignored, with a warning. |

```toml
lsp-python = "pylsp"
lsp-c = "clangd"
lsp-cplusplus = "clangd"
lsp-rust = "rust-analyzer"
```

`LANGUAGE` is Leo's name, as `@language` spells it: `cplusplus` for C++, `shell` for shell scripts. In leogui, File > Settings... (Cmd-,) > Language servers edits the same keys: a checkbox for `lsp`, and a row per language.

A server's workspace is the outline's directory, or the current directory for an unsaved outline.

### Which server a node gets

A node's language comes from, in order: `@language` in the node, then in its ancestors, then the extension of the nearest `@<file>` headline, then the outline's default (Python unless the outline sets another). The server is the one named for that language.

A server sees each external file as leolib writes it, so it works across the nodes of an `@file`, `@clean`, `@nosent` or code `@auto` tree. A node in no file is a document of its own. The servers hear an edit when INSERT commits it.

### Choosing a server

A server does what it offers, no more. `ruff server` gives diagnostics and fixes, but no hover, definitions or completion. For those in Python, use one of:

| server | install | setting |
|-|-|-|
| python-lsp-server | `pip install python-lsp-server` | `lsp-python = "pylsp"` |
| jedi-language-server | `pip install jedi-language-server` | `lsp-python = "jedi-language-server"` |
| basedpyright | `pip install basedpyright` | `lsp-python = "basedpyright-langserver --stdio"` |

A `rust-analyzer` installed by rustup is a stub until `rustup component add rust-analyzer`. Check a server's own documentation for what it offers.

### What the servers do here

| feature | keys and commands | leogui |
|-|-|-|
| Diagnostics | underlined in the body; the cursor line's on the status line. `]d` `[d` move between them; `:lsp-diagnostics` lists the body's | Body > Next Problem, Previous Problem; View > Problems |
| Hover | `K` | Body > Hover |
| Go to definition | `Ctrl-]` | Body > Go to Definition |
| Semantic colouring | automatic, where the server offers semantic tokens: parameters, variables, macros, namespaces and the like, which tree-sitter cannot tell apart, take their theme colours; `:set nosemantic` turns it off. Not while a change is typed | the same |
| Find references | `gr`: the nodes using the symbol, cloned under `Found N:references to NAME`, as `clone-find-all` gathers matches; references in files the outline does not hold are counted | Body > Find References |
| Signature help | in INSERT, after `(` or `,`: the call's signature, its parameter in brackets, on the status line; `:lsp-signature-help` | a popup above the cursor; Body > Signature Help |
| Format document | `:lsp-format`, with the node's `@tabwidth` | Body > Format Document |
| Rename | `:lsp-rename NAME`: every node at once, as one undo | Body > Rename Symbol... |
| Completion | in INSERT, Tab after a word character or a dot, or Ctrl-n anywhere | the same |
| Code actions | `:lsp-code-action`, then Up/Down or `j`/`k` and Enter, or a digit; `:lsp-code-action N` applies the Nth | Cmd-. or Body > Code Actions... |
| Status and log | `:lsp-status`: each server's state and the last 500 lines it logged | the status bar's LSP dot, or View > Language Servers |

Completion: typing narrows the list, Up/Down or Ctrl-n/Ctrl-p select, Tab or Enter takes one, and Escape closes the list and stays in INSERT. Elsewhere Tab indents. With no server for the body, Tab after a word indents, and Tab after a dot says which setting is missing. At most 200 items are shown. Snippets are not asked for, and an item's additional edits, such as an auto-import, are not applied.

A code action or a format that replaces the whole file is applied to just the lines it changes. An edit that would touch a sentinel line, a file the outline does not hold, or text changed since the request is refused whole.

### When nothing happens

- Tab indents, or says "no language server for LANGUAGE": add `lsp-LANGUAGE` to the settings.

- "the LANGUAGE server is not serving this body": `:lsp-status` shows whether it started, and what it logged if it did not.

- "language server: ..." on the status line: the server refused the request, often because it does not offer that feature.

## Plugins

The workspace holds kinds Leo does not have, as plugins outside leolib: `@entangled` (literate markdown for [entangled](https://github.com/shakfu/entangled-rs)), `@qmd` and `@rmd` (Quarto and R Markdown with their cells as nodes), and `@wiki` (markdown pages linked by `[[...]]`, exported to one file). leotui and leogui register `@qmd`, `@rmd` and `@wiki` from the release after 0.7.0. `@entangled` is in no release: its crate is unpublished, and `make ... ENTANGLED=1` builds and tests it. leolib keeps the extension API they use (`leolib::ext`); with no kind registered, it reads an outline as Leo does. What they do is in `docs/plugins.md`; the design is in `docs/dev/plugins.md`.

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

## Status

Verified against `leo/core/LeoPyRef.leo` from the Leo repository, at leo-editor `e3b3841f64`. The two `@auto` rows come from an earlier checkout and have not been re-measured since.

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
crates/leolib         the model: Leo's, with an API for kinds Leo lacks. No view, ever.
crates/leolsp         language servers, with their positions mapped to nodes.
crates/leoapp         a front end's state and commands, with no renderer.
crates/leo-markdown   @qmd and @rmd, and their markdown scanner and writer.
crates/leo-entangled  @entangled, and its :entangled-* commands. Unpublished.
crates/leo-wiki       @wiki: markdown pages linked by [[wikilinks]], exported to one file.
crates/leo-plugins    registers the plugins in leotui and leogui.
crates/leotui         the terminal front end: leoapp drawn with ratatui.
crates/leogui         the desktop front end: leoapp drawn with egui.
crates/leomcp         an MCP server on localhost, serving the open outline.
```

`leolib` has one runtime dependency for XML parsing (`quick-xml`), one for regular expressions (`regex`), and `once_cell`. `leolsp` adds `lsp-types` and `serde_json`, `leomcp` only `serde_json`, and `leoapp` the tree-sitter grammars. `leotui` adds `ratatui`, `crossterm` and `clap`; `leogui` adds `eframe`, `rfd` and `clap`, and `egui_commonmark`, `egui_extras` and `image` for the rendered view.

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

- **Encodings other than UTF-8.** Leo decodes an external file with the encoding its `@encoding` directive or `@+leo` header names, and encodes it with the same one on the way out. This port reads and writes UTF-8 only, by design, so a file in any other encoding is reported unread and is never written: writing it would replace its bytes with UTF-8 and lose every character the two encodings spell differently. The node keeps whatever the `.leo` file said. A `.leo` file that is not UTF-8 is refused outright, since there is no part of it to keep.

- **`.leojs`** (the JSON outline format).

See `docs/dev/porting-notes.md` for the places this port deliberately differs from Leo, and why.

## Building

```text
make build      # cargo build --workspace, less leo-entangled; ENTANGLED=1 adds it
make release    # the same, optimised
make run FILE=FILE.leo    # leotui
make gui FILE=FILE.leo    # leogui, release build
make test       # cargo test, likewise
make bench      # leolib's load times; LEO_EDITOR=... adds leo-editor's own outline
make corpus LEO_EDITOR=/path/to/leo-editor   # demo/'s expected files against Python Leo
make lint       # rustfmt --check and clippy -D warnings
make check      # lint, then test
make audit      # Cargo.lock against the RustSec advisories (cargo-audit)
cargo bench -p leoapp    # drawing and colouring a 5,000-line body
cargo bench -p leolsp    # keeping language servers in step with the outline
```

`.github/workflows/ci.yml` runs `make check` and `make corpus` on every push; the corpus job pins the leo-editor commit its expected files came from. `make audit` is not in CI, as it fetches the RustSec database.
