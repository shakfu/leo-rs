# Changelog

Earlier changes are recorded in the git history and in `docs/dev/tui-design.md`.

## [0.8.0]

`@qmd` and `@rmd` (in `leo-markdown`) and `@wiki` (in `leo-wiki`) are registered in leotui and leogui through `leo-plugins`. `@entangled` is in the unpublished `leo-entangled` crate, and no binary registers it. What they do is in `docs/plugins.md`. leolib, leolsp and leoapp change their public APIs, hence the minor version.

### Added

- **The plugin crates.** `@entangled` is in `leo-entangled`, with its `:entangled-*` commands as a leoapp plugin (feature `leoapp`); `@qmd` and `@rmd` are in `leo-markdown`; `leo-plugins` registers `@qmd` and `@rmd`, and leotui and leogui call it at startup. `leo-markdown` and `leo-plugins` are published; `leo-entangled` is not, so `leo-plugins` leaves it out, as crates.io refuses an unpublished dependency even when optional. The workspace builds it only with `make ... ENTANGLED=1`. `Document::rename_entangled_block` is now `rename_block`.

- **`@qmd PATH` and `@rmd PATH`: Quarto and R Markdown documents with cells as nodes.** Headings, executable cells (`{python}`, `{r setup}`) and labelled fences become nodes; display fences stay in the prose, and the file is written back byte for byte. The kind fixes the label rules whatever the extension: `#| label:` under `@qmd`, the chunk header (`{r setup}`, `label=`) under `@rmd`. An unnamed cell is headlined `<< python cell 2 >>`; renaming it adds a label in its kind's syntax. New kinds rather than a change to `@auto`, so `@auto` and `@auto-md` stay Leo's importer and existing outlines read as before. They share `@entangled`'s scanner and writer, in the `leo-markdown` crate. A heading inside a fenced div (`::: {.callout-note}`, a tabset) stays in the body, in all three kinds, so the div's opening and closing lines stay in one node. An R fence is now in the `r` language: Leo's extension table reads `.r` as REBOL.

- **A labelled cell can be cloned into an `@clean` tree.** One node is then a fence in the markdown and code in the `.py` file, for code-first literate programming: an edit in either tree is saved to both files, and `<<load>>` in a cell resolves as Leo's `<< load >>` section. The `.leo` file keeps each labelled cell's gnx in a `str_leo-rs-cell-ids` attribute on the `@qmd`, `@rmd` or `@entangled` node, so the clone survives a reopen; the tree itself is still read from the markdown. If both files changed on disk, the `@clean` file's text wins and the markdown's goes to Recovered Nodes, as for any Leo clone. An unnamed cell is renumbered when cells are added, so it gets no saved gnx; label it first.

- **`@entangled PATH`: a markdown file whose named code fences are nodes.** A leo-rs kind for literate documents in [entangled](https://github.com/shakfu/entangled-rs)'s syntax, so the code examples in documentation can be tangled to files and tested. Headings become nodes; a fence entangled would name, in the document's style as entangled reads it from the extension (`#name` or `file=` in `.md`, knitr's `{lang, label=...}` in `.Rmd`, Quarto's `#| label:` in `.qmd`), leaves its fence lines in the heading's body around a `<< name >>` reference, and its code becomes a child node headlined `<< name >>`. Prose and unnamed fences stay text. The file is written back byte for byte, and a read that would not reproduce it leaves the whole file in the node with an error. A fence node takes its language from the info string, so its code is coloured and served by a language server as that language. The scanner follows CommonMark, unlike `@auto-md`'s. The design is in `docs/dev/entangled_leo_backend.md`.

- **An `@entangled` heading's level follows the tree.** Moving a heading node to another depth moves its level by as much, its subheadings with it: demoting `## Usage` under `## Install` writes `### Usage`, where the level stayed `##` and the move was lost on the next read. A heading not moved keeps its line as the file had it, so a file that skips a level is not rewritten; levels stay within 1 to 6, and an underlined heading moved past level 2 becomes `###`.

- **`include=` fills an `@entangled` fence from a tested file.** ```` ```python #add include=lib.py#add ```` takes the whole of `lib.py`, or with `#add` the lines between `ANCHOR: add` and `ANCHOR_END: add` comments, as mdBook does; the path starts from the markdown file's directory. The file is the source, so the code can be tested on its own: the fence node is read-only, an edit is refused with the file to edit named, and the fence is filled from the file on every read and every save. A fence whose file changed since the markdown was written leaves the `@entangled` node unsaved, so the next save updates the markdown; a missing file or anchor is a read warning. `.Rmd` and `.qmd` files keep `include` as knitr's and Quarto's own option.

- **An `@entangled` name is one block.** entangled joins fences that share a name into one block; leo-rs refuses such a file, as Leo refuses a section defined twice, so a fence node's headline always says which node it is. The file opens as one node holding all of it, with an error naming the block, and is written back unchanged. A rename to a name another block in the document has is refused, and so is a write that would give two fences one name.

- **Renaming an `@entangled` block.** Editing a fence node's headline, `<< count >>` to `<< tally >>` or just `tally`, renames the block as one undo step: the `#name`, `label=` or Quarto `#| label:` in its fence, its `<< >>` reference, and entangled's `<<count>>` and `<<README.md#count>>` in the code of every `@entangled` document in the outline. A bare `<<count>>` in another document is left alone when that document has a block of its own by that name. A name another block in the document has, a block named only by its `file=` or `include=` target, or a name with a space, is refused with nothing changed. Documents outside the outline are not checked; `:entangled-check` finds what they still name. MCP's `set_headline` renames the same way.

- **`:entangled-tangle` and `:entangled-check`** run entangled's own command in the outline's directory, so tangling follows `entangled.toml` exactly. They write the unsaved `@entangled` files first, and stop if one cannot be written, since entangled would read the old text. The command runs on a thread of its own; its output goes to `:messages` and its last line, or why it failed, to the status line. Arguments pass through: `:entangled-tangle --force`. leogui has them in the Body menu. The `entangled` setting names the program, for an app started without the shell's `PATH`.

- **`demo/entangled/`: a README whose examples are tested.** `tests.md` places each named example from `README.md` in a pytest function by reference; tangling writes `test_readme.py`, and an example that no longer holds fails its test. `demo.leo` holds the README, the tests and the library in one outline. The conformance corpus skips it: Python Leo reads `@entangled` as a plain node, so there is nothing to check it against.

- **`demo/qmd/`, `demo/rmd/` and `demo/wiki/`: an example for each plugin.** The `@qmd` and `@rmd` outlines clone labelled cells into an `@clean` file that `make test` runs. The corpus skips them, as it skips `demo/entangled/`; `leo-plugins/tests/demo.rs` checks that each opens without errors and writes back unchanged. It also runs the cloned code and renders each document, skipping a step when Python, Quarto with Jupyter in the uv `.venv`, R or rmarkdown is missing.

- **`@settings` trees set what leolib reads and writes.** The outline's `@settings` tree, and `~/.leo/myLeoSettings.leo` in leotui and leogui, give `tab-width`, `page-width`, `output-newline`, `target-language`, `default-derived-file-encoding`, `create-nonexistent-directories` and `force-newlines-in-at-nosent-bodies`; an outline with `@string output-newline = crlf` now writes CRLF files, as Leo does. Other settings are ignored. A value that is not valid is reported and the default kept. `@ignore`, `@ifplatform`, `@ifenv` and `@ifhostname` are honoured; `@if EXPRESSION` is Python, so its settings are skipped.

- **A session that ends without the user's say keeps its work.** On a hangup (a dropped ssh session), SIGTERM, a terminal that fails, or a panic, leotui writes every node to `NAME.recovered.leo` beside the outline, all under one `@ignore` node, so opening it reads no external file. The next open names the copy. Five seconds after a signal the process exits whatever it is doing.

- **The quit question offers to save.** `s` saves and quits, `y` quits without saving, `n` stays; a question answers to one key in leotui, as it did in leogui, whose dialog gains a Save button.

- **`:commands`** lists every command with its keys, including the many no key runs. An unknown command suggests the nearest name. Leo's `save-file`, `save-file-as`, `save-file-to`, `exit-leo`, `open-outline` and `help-for-command` run their leo-rs commands. Tab on a command name with several matches shows them, as vim's wildmenu does.

- **`@wiki`: markdown pages linked by `[[...]]`**, in the new `leo-wiki` crate. `gd` follows a link and `Ctrl-o` comes back; `[[` completes a page; renaming a page rewrites the links to it; `:export-wiki` writes one markdown file with GitHub anchors, refusing a broken or ambiguous link or a page deeper than six. An edit that breaks a wiki's rules (no clones, no `@` headlines, no directives in pages) is undone and named. `leolib::ext::TreeKind` is the extension it uses: a directive whose tree is stored in the `.leo` file, unlike a `FileKind`'s. `gd` also follows Leo's `gnx:` and `unl:` links everywhere. Design: `docs/dev/wiki.md`.

- **A `[[wiki link]]` is drawn and followed as a hyperlink.** In a `@wiki` page, both front ends draw it underlined in the theme's `markup.link.url` colour; the builtin theme's is light blue. `Enter` in NORMAL follows it, as `gd` does, since `gd` is a vim key few expect to follow a link. In leogui a click follows it, and the mouse shows a pointing hand over it. leogui's rendered view shows each link that names one page as a link to its node, and a click selects the node; the link is an egui_commonmark link hook, so hovering does not show its `unl:gnx://` URL. A link naming no page or several is shown as typed. Following uses the scan that drawing and export use, so a `[[link]]` in fenced code is text.

- **Semantic-token colouring.** Where a language server offers semantic tokens, parameters, variables, macros, namespaces and enum members take their theme colours over tree-sitter's; `:set nosemantic` turns it off. Tokens are asked for when the text the server has changes, and not shown while a change is typed.

- **Find references, signature help and format document**, from the language server. `gr` clones the nodes using the symbol under `Found N:references to NAME`, as `clone-find-all` gathers matches. A call's signature shows while its arguments are typed, its parameter in brackets on the status line and in a popup in leogui. `:lsp-format` formats the node's file and applies just the lines it changes; one touching a sentinel line refuses the whole.

- **leogui's body: matching brackets, a ruler and guides.** The bracket at the cursor and its match are shaded; an `@pagewidth` directive draws a ruler; indent guides mark each level (`:set noguides`); `:set list` marks spaces and tabs.

- **Several nodes at once in leogui.** Cmd- or Ctrl-click adds a row, Shift-click a run; Delete, Mark and a drag then act on every chosen row, as one undo step each (`Document::move_nodes`, `delete_nodes`).

### Changed

- **leoapp: plugins can mark and render links.** `AppPlugin` has `links`, the ranges the body draws as links, and `rendered`, a node's markdown as the rendered view shows it. `view::decorate` takes the links and marks their segments, and `BodyView` carries them. `App::follow_url` follows a Leo link outside the body, as the rendered view needs.

### Fixed

- **A new `@clean`, `@nosent` or `@asis` node no longer overwrites a file it never read.** Typing `@clean b.py` over an existing `b.py` and saving emptied it without a prompt: `may_overwrite` exempted `@clean` and `@nosent`, as Leo does. Now each asks first. An `@nosent` or `@asis` node the `.leo` file already had is still written without asking. Through MCP, a client with edit and save rights could otherwise write any file the user can.

- **An `@clean` file edited by another program is no longer overwritten unseen.** A file that matched its tree on open got no stamp, so the edit was never reported and the next save replaced it. With `--no-external`, a save wrote the `.leo` copy over the file.

- **An `@file` cut short before its `@-leo` line is a read error.** It read as success with every body empty, and the next save wrote the empty bodies over the code.

- **A `.leo` file stays readable whatever its gnxs and attributes hold.** Gnxs were written unescaped, so one holding `&` or `"` made the next open fail; the login name in new gnxs is now cleaned as Leo's `cleanLeoID` cleans it. A `tx` attribute on a `<v>` element was written back as a second `tx` on its `<t>`. A node listed inside itself is refused; it hung the reader.

- **Demoting a node whose next sibling is its clone, or moving one into its clone, is refused.** Either made the node its own descendant, and the outline endless. Deleting the only top-level node is refused too; it emptied the outline while the status line said it could not.

- **The Rust importer makes `impl`, `struct` and `trait` blocks nodes**, as Leo does. Its patterns end in `$`, which in Rust's regex did not match before the line's newline, as Python's does.

- **Messages that say what to do.** A failed save leads with the reason and the file (`cannot save a.leo: permission denied; in /work`), not the temp file's path. A broken `.leo` file names the element left open or the closing tag that does not match, with lines. leotui without a terminal says so; given a file that is not an outline, it names the import commands. Startup messages no longer hide each other: the first shows with a count, and `:messages` has all of them, wrapped. Settings warnings name the settings file, and an out-of-range `split-ratio` is reported. A new outline's `newHeadline` starts selected, so typing replaces it.

- **leogui no longer offers `.leojs` and `.db`** in its Open dialog, which it could not open.

- **A `.leo` file with CRLF line endings reads as Leo reads it.** A Windows checkout's file put `\r\n` into every body, where Leo, following XML, reads `\n`; a written `&#13;` is still a CR.

- **Gnxs take Leo's id first from `~/.leo/.leoID.txt`**, then the login name, as Leo does, so one person's nodes carry one id in both.

- **`@ifenv` and `@ifhostname` in `@settings` are tested as Leo tests them**; their settings were skipped. The pickle reader reads and writes floats and tuples, so a uA blob holding one survives a restructure.

- **The Python importer no longer panics on a backslash before a non-ASCII character** (`"\é"`).

- **`$NAME` and `${NAME}` in `@path` and file names are expanded**, as Leo's `os.path.expandvars` does. `%NAME%` on Windows is not.

- **Saving is atomic per writer and durable.** Each save writes a temp file of its own, fsynced before the rename, so two instances saving one file no longer share `FILE.leo-rs-tmp`. Errors name the file being saved, not the temp file.

- **A file that fails to read keeps its nodes' uAs.** The `descendentVnodeUnknownAttributes` blob was consumed even though the tree it names was not read, so the next save dropped its uAs for good.

- **A uA blob leo-rs rebuilt could fail to read back.** The pickle reader's cap counts opcodes and memo copies, and protocol 1 memoizes every tuple, so a value built cheaply with `TUPLE1` could cost size x depth once written: a uA of tuples nested about 90 deep read, and the `descendentVnodeUnknownAttributes` blob rebuilt from it did not. The blob stayed parked, unchanged, but its subtree's uAs were not restored. The reader now refuses a value unless its own pickle reads within the cap, which the `pickle` fuzz target found.

- **The MCP server reads nothing large before it checks the token**, closes a refused connection, caps request lines and headers, and serves at most 16 connections. A settings file holding `mcp-token` is written readable by its owner only.

- **leotui and leogui say when plugins failed to register**, on stderr; a release build was silent.

- **`@qmd` and `@rmd` fixes before their release.**
  - A body edited to end without a newline joined the next heading onto it; the headings were lost on reopen.
  - Only the CR of a CRLF is removed. A line ending in CR alone makes the read refuse, which keeps the file byte for byte; it lost every line break before. A dropped byte-order mark is reported.
  - A refused read kept the whole file in the node, but the next write could refuse that body too; it is now written as it stands. A refused read no longer erases the saved cell gnxs, which unlinked `@clean` clones.
  - A fence opener on a file's last line, with no newline, is text.
  - A quoted label is one name: `#| label: "b c"` renames in place, where a second label line was added, and knitr's `label='x y'` no longer reads as `x`. A label named `label` renames its value.
  - An indented cell with a line of just its indent reads; it was refused.
  - Edits that would change the tree on the next read are written another way or refused: an underlined heading renamed to `- todo` is written `## - todo`, and a cell whose code would close its fence, or a heading with no headline, is refused. `#` lines in an HTML comment spanning lines stay text.

## [0.7.0]

### Added

- **leogui's rendered view, Leo's `viewrendered`.** View > Rendered View opens a pane beside the body showing the selected node rendered: markdown (`@language md`, or a `@md` headline) with its tables, code blocks coloured by language and images, and an `@image` node's picture. Leo's directives are left out, and it follows the text as it is typed. Relative image paths start from the outline's directory, as Leo resolves them. reStructuredText has no Rust renderer, so it is shown as text; other nodes say there is nothing to render. The pane is remembered with the session. It adds `egui_commonmark`, `egui_extras` and `image` (PNG, JPEG, GIF, WebP) to leogui: 12 crates. What a node renders as is decided in `leoapp::rendered`, testable without a window.

- **`leolib::ext`: kinds Leo does not have can be registered per outline.** A crate implements the `FileKind` trait (read, write, save, rename, read-only, rendered markdown), and an outline reads, writes and saves that kind only if it is in its `Kinds` (`Outline::kinds`). `open_outline` and `Document::open` register none, so leolib reads an outline as Leo does; `open_outline_with_kinds` and `Document::open_with` take a set. A kind cannot take a directive Leo has. A per-outline set rather than a global one, so a test can open one outline as Leo and another with kinds in the same process. leoapp has the matching `plugins::AppPlugin` (commands, settings keys, menu items, background polling) and `plugins::register_kinds`; leogui adds a plugin's menu items. The 0.7.0 binaries register nothing, so nothing changes for users. Design: `docs/dev/plugins.md`.

## [0.6.1]

Tidies up after 0.6.0, whose desktop front end was published as `leoegui` and then withdrawn. `leogui` 0.6.0 on crates.io already had the rename and the Windows fix below; the other crates had neither.

### Added

- **Release archives for leogui.** Each GitHub release has a `leogui-VERSION-TARGET` archive per platform, holding `leogui` and `leogui-glow`, beside leotui's. The release's check job now installs the window libraries leogui needs to build on Linux, as `ci.yml` does; without them its `make check` would fail.

### Changed

- **leoegui is renamed leogui**: the crate, its binaries `leogui` and `leogui-glow`, and its release archives. The `leoegui` crate was removed from crates.io after its one version, 0.6.0; `cargo install leogui` replaces it.

### Fixed

- **A clone two external files disagree on is no longer overwritten silently.** A node cloned into two files took the text of the one read last, so an edit made in one file was lost when the other still held the old text: an example edited in `@clean README.md` and stale in `@file tests/test_readme.py`, say. The node still keeps the later file's text, as Leo does, but the read now reports the conflict on the status line and in `:messages`, and keeps both texts with a diff under a `Recovered Nodes` node, in Leo's layout for it. Leo itself loses the edit; see `docs/dev/porting-notes.md`. `ReadResult::conflicts` lists them.

- **leogui on Windows opens no console window.** Release builds of `leogui` and `leogui-glow` use the Windows GUI subsystem; debug builds keep the console, so `--help` and errors still print there.

## [0.6.0]

### Added

- **`leoegui-glow`: leoegui drawn with OpenGL**, to compare with `leoegui`'s wgpu (`make gui-glow`). On Linux a glow-only build reached its first frame in 0.09 s and 117 MB against wgpu's 0.28 s and 198 MB (`docs/dev/gui-roadmap.md`, Performance 7). wgpu stays the default because OpenGL is deprecated on macOS. leoegui is now a library with the two as thin binaries, each holding both renderers.

- **Completion from the language server, in both front ends.** In INSERT, Tab after a word character or a dot asks the server what could be typed there, and Ctrl-n asks anywhere; elsewhere Tab still indents. With no server for the body, Tab after a word indents, and Tab after a dot names the missing `lsp-LANGUAGE` setting, or points to `:lsp-status` when the server is named but not serving the body. The completions show under the cursor in leoegui and above the status line in leotui. Typing narrows them, Up/Down or Ctrl-n/Ctrl-p select, Tab or Enter types the selected one over the word, and Escape closes the list and stays in INSERT. The servers see committed text only, so the request first sends the working copy: it is put in the node's body for one render and taken out, so neither undo nor the outline's generation sees it. Snippets are not asked for, and an item's additional edits, such as an auto-import, are not applied.

- **leoegui: hoist banner, clone counts, and drop to import.** While a node is hoisted, the sidebar names it with a De-hoist button. A cloned row shows how many places its node appears, and its context menu lists them to jump to. Dropping a `.leo` file on the window opens it; any other file is imported as an `@auto` tree, which `:import-auto PATH` also does in both front ends.

- **Language-server status and log, in both front ends.** Each configured server's state, and the last 500 lines the servers sent to `window/logMessage`, `window/showMessage` and stderr, which was discarded before. `:lsp-status` shows them; in leoegui the status bar's LSP dot, lit while a server runs and red if one failed, opens them in View > Language Servers.

- **leoegui restores the session.** Started with no outline named, it reopens the outlines open at the last quit, each with its selected node, body cursor and tabs, and the bottom panel and window size, from `session` beside the settings. Quitting writes it; an unsaved outline, or one whose file is gone, is left out. Named outlines or `--press` skip the restore, so a scripted run opens only what it names. `--no-session` neither restores nor saves, so a one-off run leaves the session as it was.

- **leoegui opens several outlines, a tab each.** File > Open... and Save As... use the system's dialogs (`rfd`), File > Open Recent lists the last ten outlines (kept in `recent-outlines` beside the settings), and `leoegui a.leo b.leo` opens both. Quitting asks of each outline with unsaved work, and a no stops it. Each outline has its own undo, language servers and sidebar; options, histories, theme and settings are shared. The MCP server stays with one outline and moves to the next when that one closes, as a second server would need a second port. `:e` still replaces the outline shown.

- **leoegui's go to node, Cmd-P.** Every headline, ranked as the command palette ranks commands; a match in a node's ancestors ranks below one in its headline.

- **leoegui shows each external file's state.** An `@<file>` row is badged unread (its read failed), changed on disk, not read (it exists but was never read, so a write asks first), or unwritten. A bar above the body offers Reload or Keep for files changed on disk; Keep records the file's current stamp, so the next write overwrites it without asking.

- **Language-server code actions, in both front ends.** `:lsp-code-action` (leoegui: Cmd-. or Body > Code Actions...) lists the server's fixes for the cursor, sent with the diagnostics on its line, the server's preferred one selected; Up/Down or `j`/`k` and Enter, a digit, or `:lsp-code-action N` applies one as one undo. An action that is only a command runs on the server, and the edit it sends back (`workspace/applyEdit`) is applied the same way. Edits map to bodies as a rename's do, and a list offered before the outline changed is refused. An edit wider than one body, as ruff's fix-all replacing the whole file, is cut by a line diff into the lines it changes, each mapped to its node; it is refused whole only if a changed run crosses a sentinel or another node, or drops a line's `@others` indent. `leolib::seqmatch` is public for it.

- **leoegui's find panel, Leo's Find tab.** Ctrl-Shift-F opens it in the bottom panel: find and replace text, regex, whole word and ignore case, headlines and bodies, and a scope of outline, subtree or marked nodes. Find All lists every match to click to; Replace All is one undo; Clone Find All and its flattened form clone the matches in scope under a `Found` node. Its search becomes `/`'s, so `n` continues it. Case is set by the checkbox, not by smartcase.

- **MCP access to the open outline.** With `mcp = true` in the settings, the running app serves its outline to MCP clients on 127.0.0.1, over streamable HTTP, behind a bearer token and a check that `Host` and `Origin` are local. Clients read nodes by gnx, search, and see the selection; with `mcp-edit` they change headlines and bodies and insert, move, delete and select nodes, each an undo step, and with `mcp-save` they save. Off by default, and read-only once on. The tools are answered on the app's own thread, so an edit lands in the live outline, not in the file behind it. `:e` keeps it running.

- **A settings dialog, and a settings file both front ends share.** Settings move from `~/.config/leotui/config.toml` to `~/.config/leo-rs/settings.toml`, named for the project since both front ends read it: on first start the old directory is renamed whole, themes included, and the file inside it. leoegui's File > Settings... edits appearance, themes, editor defaults (`number`, `wrap`, `syntax`), keys, language servers and MCP, and writes only what changed. `lsp = false` starts no language server; the dialog turning it off stops the running ones.

- **leoegui, a desktop front end.** The same keys, commands, settings and themes as leotui, drawn by egui in a monospace grid, with clicks and the wheel in NORMAL. On macOS Cmd is Leo's Ctrl: a Cmd chord the outline binds runs from either pane, so `Cmd-R` indents the node from the body too, as in Leo, and any other Cmd chord is Ctrl. Control keeps leotui's meaning. Leo's `qt-mac-dont-swap-ctrl-and-meta = true` leaves Cmd unbound. `--press KEYS --screenshot out.ppm` saves one frame. View > Appearance switches between a dark theme (`theme`) and a light one (`theme-light`), or follows the system. View > Theme... lists the themes on disk as dark or light, previews the one under the pointer, and saves the one clicked for the current appearance. The window's parts take Helix's scopes for them (`ui.statusline.insert`, `ui.cursor.primary`, `ui.bufferline.active`, `ui.menu.selected`, ...), text colour included, and diagnostics take `diagnostic.*`'s underline colour and style, wavy for `curl`. Why egui over gpui is in `docs/dev/leogui.md`.

- **Language servers, in both front ends.** A server named in the settings (`lsp-python = "pylsp"`) sees each external file as leolib writes it, and its answers come back in body rows: diagnostics underlined and on the status line, `K` hover, `Ctrl-]` definition, `]d` `[d`, `:lsp-diagnostics`, and `:lsp-rename NAME` across every node of a file as one undo. No server starts unless named, because a server runs code from the project it is pointed at. An edit that would cross a sentinel line, reach a file the outline does not hold, or apply to text changed since the request is refused whole. `:e` restarts the servers in the new outline's directory. Keeping the servers in step is cheap: with 20 of leo-editor's Python files open, a selection move takes 0.01 ms and an edit 6.8 ms. A document is rendered again only when a fingerprint of its tree and ancestors changes, which also catches a node moved between files and a directive changed in an ancestor. Diagnostics map through each document's line offsets: 500 in a 6,300-line file take 8.7 us. `cargo bench -p leolsp` times both; `Lsp::with_connect` is public for its stand-in server.

- **`Outline::position_of_gnx`**: a node's position from its gnx, by walking up from leolib's gnx map. 16 tabs at 11,600 nodes are found in 1.5 us, where walking the outline took 1.2 ms.

- **`leolib::goto::line_map_of`**: an external file's text, each line mapped to its node, body row and `@others` indent. A file the sentinel writer does not write, such as `@edit` or a markdown `@auto`, has no map rather than a guessed one.

- **`goto-global-line` on `Alt-g`, and `show-file-line`.** `:goto-global-line N` selects the node that writes line N of the selection's `@file`, `@clean`, `@edit` or single `@asis` file, with the cursor on that line; `show-file-line` gives the line for the cursor. Both read one map in `leolib::goto`. Leo's `show-file-line` adds the row to the node's first line, which is wrong after an `@others` or a section reference; the map is not. Leo also counts an `@verbatim` sentinel as a body row, which puts later rows of that node off by one; here it counts as none.

- **`move-marked-nodes`, from `:`.** As Leo's, but one undo step; Leo's cannot be undone. `leolib::Document::move_marked`.

- **`reformat-paragraph`, from `:`.** Leo's command: the paragraph at the cursor is wrapped to `@pagewidth`, with hanging indents for list items, and the cursor moves to the next paragraph. `@pagewidth` had no reader before. `leolib::reformat` ports it with `util.wrap_lines`, and its tests are Leo's output on the same input.

- **`write-at-file-nodes` writes every `@<file>` node at or under the selection, dirty or not**, as Leo's does. `w` keeps writing the dirty files, under Leo's name for that, `write-dirty-at-file-nodes`.

- **`:messages`** lists every status message shown, up to 200. A read's warnings, such as an `@auto` file the importer reformatted, reach the status line and the log; they were never shown.

- **`leotui new.leo` starts an outline that the first save creates**, as vim does, where it failed with "not found". `:e` does the same.

- **A body cursor per node.** Leaving a node and coming back puts the cursor and scroll where they were, as Leo's `v.insertSpot`; they returned to the top.

- **`NO_COLOR` and theme colours for the outline.** The selected row, marked and `@<file>` nodes and pane borders take the theme's `ui.menu.selected`, `ui.selection`, `warning`, `ui.text.directory`, `ui.text.focus` and `ui.window`, so a light theme suits a light background. A theme without them keeps the old colours. A non-empty `NO_COLOR`, or `:set colors=none`, draws no colour, and shows the selected row and status line reversed.

- **`@type.builtin` for C, C++, Go and Python.** `int` and `unsigned long` in C, Go's 22 predeclared types, and builtin types in a Python annotation now draw as builtins, as Rust's `u8` did. JavaScript has no type syntax to mark.

- **A rename to `@file` adds `@first` to a leading `#!` or coding line**, in the same undo step, as `:import-at-file` does. The sentinel header pushed them to line 3.

- **`make bench`**: criterion times for opening and saving `LeoPyRef.leo`, and with `LEO_EDITOR` set, for leo-editor's outline with its external files. **`fuzz/`**: `cargo-fuzz` targets for the sentinel reader and the pickle reader. **`SECURITY.md`** and **`CONTRIBUTING.md`**.

### Changed

- **Long bodies colour faster, in both front ends.** The body pane keeps the colourings of the last four bodies shown, so switching back to one does not colour it again. A long body seen for the first time is coloured on screen first. The whole colouring, of a first visit or after typing pauses, is made on one worker thread shared by every outline, so no frame waits on it; `poll` puts it in. Only the shown body is coloured whole: the worker skips the job of a body left before it began, and keeps the result of one already begun for when the body is shown again. A colouring is checked against the body by comparing its lines, where every line was hashed each frame. On a 5,000-line Python body (`cargo bench -p leoapp`), the first frame went from 21 ms to 1.4 ms, and switching between two such bodies from 42 ms to 0.8 ms. The whole colouring, 21 ms, no longer blocks a frame. `docs/dev/colouring.md` compares the designs.

- **The outline takes Leo's own keys.** `Ctrl-r` `Ctrl-l` `Ctrl-u` `Ctrl-d` move the node right, left, up and down, as Leo's `move-outline-*`, where they were vim's redo and half pages; the body keeps vim's. Leo's Alt-arrows navigate and Alt-Shift-arrows move from either pane, `Ctrl-Shift-c` `-x` `-v` `-d` copy, cut, paste and extract a node, `Ctrl-{` `Ctrl-}` promote and demote, and `Alt-d` `Alt-t` `Ctrl-t` `Ctrl-g` are Leo's pane keys and keyboard-quit. Leo leaves go-back and go-forward unbound, so they move from Alt-arrows to `H` and `L` in the outline. This reverses `tui-design.md`'s rule that a Ctrl key means the same in both panes: Leo's outline keys win in the outline. While a headline is being typed, those chords keep the headline and act on the node, as in Leo's headline editor, where they were swallowed: a new node can be indented before it is named.

- **A large outline or body no longer slows every redraw.** The outline's rows are walked again only when its shape, its folds or the hoist change (`Outline::expansion` counts folds), and only the rows on screen are built: a frame of a 21,000-node outline went from 6.2 ms to 0.2 ms. A body over 500 lines recolours only near an edit while you type, and whole once typing pauses for 300 ms: a key in a 5,000-line body went from 35 ms to 1.8 ms.

- **New crate `leoapp`: leotui without its renderer.** The app state, vim editor, commands, bindings, search, colouring and theme moved out of leotui, which keeps only the ratatui drawing and the terminal loop. A GUI front end can reuse them; see `docs/dev/leogui.md`. `leoapp::keys` defines its own `KeyCode`, `KeyModifiers` and `KeyEvent`, so crossterm is a leotui dependency only. No behaviour changes.

- **An `@auto` file with no importer is read whole into the body**, as Leo's `scanUnknownFileType`: after `@language` for a known extension, `@nocolor` for `.txt`, nothing otherwise. It was reported unread. A file whose whole body would not write back unchanged, one holding an `@others` line say, is still reported and left unread. `.rst` stays unread, since Leo splits it with an importer this port lacks.

- **`@auto-md`, `@auto-markdown`, `@auto-org`, `@auto-org-mode`, `@auto-otl` and `@auto-vim-outline` are `@auto` nodes.** `AT_AUTO_NAMES` held only `@auto` and `@auto-rst`, so these headlines were plain nodes: never read, never written. Leo adds each importer's names when it loads the importer. `.rmd` now goes to the Markdown importer, as in Leo.

- **`@jupytext` is refused on read and write**, as `@shadow` is, until it is ported (`TODO.md`). It was read and written as a sentinel `@file`, which would put sentinel text into a notebook's JSON. Leo converts through the jupytext package.

- **`read-at-file-nodes` skips an `@clean` file unchanged since its last read or write**, as Leo's does (#4385). Only `refresh-from-disk` drops the cached mod time first (#4875); `external::refresh_files` and `Document::refresh_files` are that path.

- **`leolib::Document`'s `outline` and `undoer` are private.** An edit through the public `outline` field skipped the undo history; the `g<` bug was one. `outline()` reads; `outline_mut_untracked()` is the named way round, for folds and approvals. `begin_group`, `end_group` and `clear_undo` replace the undoer's own.

- **`Outline`'s interdependent fields are crate-private**: `gnx_dict`, `expanded`, `mod_time_cache`, `read_paths`, `import_warnings`, `file_stamps`, `dropped_descendent_uas`. Editing `read_paths` could defeat the overwrite guard. `expanded()`, `set_expanded()` and `changed_files()` cover what leotui used. `atfile_read` is crate-private too.

- **`Outline::position_exists` checks every step of the position**, as Leo's `c.positionExists`. It checked only the last, so a position under a deleted node passed. `position_is_linked`, which did the full check, is gone.

- **The undo stack keeps 1000 steps**, as vim's `undolevels`, and the vnodes only dropped steps named are freed and their slots reused. A long session on a large outline grew without bound. `Document::set_undo_limit` changes the cap.

- **Every public leolib item has a doc comment**, and `#![warn(missing_docs)]` keeps it so.

- **Each crate's package includes the MIT `LICENSE` file**, as the licence asks of copies; `license = "MIT"` named it without shipping it. The crates hold copies rather than symlinks, which a Windows checkout without symlink support turns into text files.

- **leolib's `rust-version` is 1.89, leotui's 1.90.** leotui's floor is `tree-sitter-language`'s; the library is not held to it.

### Fixed

- **An inheriting Helix theme now takes its own palette.** A variant that is `inherits` plus a `[palette]`, as `catppuccin_latte` and `rose_pine_dawn` are, drew its parent's scopes in the parent's colours: each file's scopes were resolved with that file's palette before the chain was merged. Palettes are merged first now, child over parent, as Helix does, so a light variant is light in leotui too.

- **A crafted unknown attribute could abort the reader.** A pickle `LONG_BINPUT` names a memo slot up to 2^32, and the memo was a vector resized to it: six bytes in a `.leo` file asked for 64 GB. The memo is a map now, as Python's dict is, and a blob may build at most 4096 values, counting each memo copy, since Python shares what this copies. The largest blob in a leo-editor checkout builds under 100. A random-mutation run found the bug.

- **`:set wrap` wraps.** Each line was cut to the pane before ratatui could wrap it.

- **The body cursor counts screen columns.** It was placed by character index, so it drifted after a tab or a wide character. Tabs expand to `@tabwidth` stops rather than four blanks, and without `wrap` the view scrolls sideways to keep the cursor on screen.

- **A `/` search that finds nothing says so on Enter.** Escape from a search folds again what its preview unfolded.

- **`:e` keeps the theme, colour depth, text register, last search and message log.** It rebuilt the app and kept only options and histories.


## [0.5.0]

### Added

- **`go-back` and `go-forward`, on `Alt-Left` and `Alt-Right` in both panes.** They walk Leo's node history: every selected node, each vnode once, as `leoHistory.NodeHistory` keeps it since #3800. Leo leaves both unbound. Alt-arrows over vim's `Ctrl-o`, which Leo binds to `open-outline`. Two edge cases differ from Leo; `docs/dev/porting-notes.md` has them.

- **`clone-find-all` (`:cfa`) and `clone-find-all-flattened` (`:cff`).** Each node that matches is cloned once under a new last top-level node, `Found N:pattern`, whose body starts with `@nosearch` so a later search skips it. `@nosearch` and `@ignore` trees are not searched, and, unflattened, neither is a match's subtree. The pattern is matched as `/` matches it, honouring `:set search=headlines`; with no pattern the last search is used, and a new one becomes the last search. `leolib::Document::clone_find_all` takes the match as a predicate, so the search rules stay the front end's.

- **`extract`, from `:`.** The VISUAL lines, or the cursor's line, become the current node's first child, as one undo step. The child's headline is a section reference on the first line, which stays in the body; else the name a Python, JavaScript, CoffeeScript or Clojure definition line defines; else the first line, which leaves the body. Leo's code and its docstring disagree on that last case; this follows the code. Leo's `Shift-Ctrl-D` reaches a terminal as `Ctrl-d`, so there is no key. `leolib::Document::extract` does the work, and `util::remove_leading_whitespace` ports `g.removeLeadingWhitespace`.

- **`sort-siblings` on `Alt-a`, as in Leo, and `sort-children` from `:`.** Headlines are compared ignoring case, and equal ones keep their order, as Leo's `sortSiblings` does. A sort under an `@file` node marks it dirty, since the order is the file's `@others` order. `leolib::Document` gains both, recorded by a new `Bead::Sort` that restores the old order in one step.

- **Leo's marked-node commands:** `mark-subheads`, `mark-node-and-parents`, `unmark-node-and-parents`, `clone-marked-nodes`, `copy-marked-nodes` and `delete-marked-nodes`, from `:`. Each is one undo step. `delete-marked-nodes` keeps the last top-level node, as `delete-node` does; Leo has no such guard. It also keeps the selection where it was, if it survives, rather than folding the whole outline. `move-marked-nodes`, which Leo cannot undo, is not ported.

- **`hoist`, `dehoist` and `clear-all-hoists`, on `zh` and `zH` in the outline.** A hoist shows one node and its subtree as the whole outline, as Leo's does. Moves that would take a node out of the hoisted tree, or move the hoisted node itself, are refused. Selecting a node outside it, by search, `gd` or `go-back`, dehoists until it shows, as `c.selectPosition` does. Leo leaves hoist unbound; its alternative `Ctrl-Shift-h` reaches a terminal as `Ctrl-h`.

- **`gd` in the body selects the node a `<< section >>` reference names.** It is the section branch of Leo's `open-url-under-cursor`, which also opens urls, unls and gnxs; those are not ported. The headline is matched as the `@file` writer matches it, so `gd` lands on the node the file includes. `go-back` returns.

### Changed

- **`leolib::Error` and `leolib::undo::Bead` are `#[non_exhaustive]`.** A new variant, such as this release's `Bead::Sort`, no longer breaks a caller's exhaustive `match`. Such a match needs a wildcard arm once.

- **`Ctrl-c` quits, in every mode, and never discards text.** In INSERT it abandoned the session with no undo, which lost the typing of anyone pressing it to leave. It now commits the open edit (an INSERT session or a headline), cancels a `:` or `/` line, and runs `quit`, which asks before losing unsaved work. In a yes/no prompt it answers no. Quit over vim's leave-INSERT: a user pressing `Ctrl-c` most likely wants out, and the prompt makes a mistaken quit harmless.

### Fixed

- **The bindings shared by both panes work in the body.** In body focus every key went to the vim grammar, so `Ctrl-s`, `Ctrl-f`, `Ctrl-b`, `Ctrl-d`, `Ctrl-u`, `PageUp`, `PageDown` and `F1` answered "no such command". A key the grammar does not know now falls through to the binding table. Paging in the body also moves the cursor, as vim's does; it moved only the view, and the next motion snapped the view back.

- **A `:` line ends VISUAL, as in vim.** The mode returned to NORMAL, but the selection stayed set.

- **A modified arrow, `Home` or `End` in the body reaches the binding table.** The vim grammar took `Alt-Left` as `Left`, whatever the modifier.

- **A Ctrl or Alt chord types nothing in INSERT or a one-line input.** It typed its letter. `Ctrl-w` and `Ctrl-u` now delete the word and the text before the cursor, as in vim and readline. Ctrl and Alt together still type, because Windows sends AltGr as that pair.

- **A long count no longer panics or hangs.** Typing digits multiplied without a bound, so `99999999K` panicked in a debug build and looped for 1.9s in release. Counts now saturate and are capped at 99,999. `gg` takes the raw count, so a line number is not capped.

- **A count on `.` replaces the change's count, as in vim.** `2dw` then `3.` deleted six words and recorded three undo beads; it now runs `3dw` once. The old loop let `9999.` record thousands of beads, each holding the old and new body.

## [0.4.0]

### Fixed

- **An `@clean` file edited in the same second as a write was never read again.** The cached mod time was truncated to whole seconds, so `old >= new` held for an outside edit made within the second of our own write, and the next write reverted it. The cache holds a `SystemTime` now, as Leo compares float mtimes. An edit to a body also drops it, as Leo's `setBodyString` does; only `setHeadString` had a counterpart here.

- **A body `@path` was ignored in `@clean`, `@auto`, `@edit`, `@nosent` and `@asis` nodes.** Leo skips one only in `@file` and `@thin`, where the headline already names the file (`getPathFromNode`). The file resolved against the wrong directory, and since `@clean` and `@nosent` are exempt from `may_overwrite`, the write could land on an unrelated file.

- **An `@encoding` this port cannot write is refused whatever its name.** `is_valid_encoding` held a list of seven names; `@encoding cp1252` was not among them, so `get_encoding` fell back to utf-8 and the write replaced the file's bytes without a report. The test is now the shape of a codec name, and every name outside the utf-8 and ascii aliases reaches `encoding_is_supported` and is refused. Leo asks Python's codec registry, which this port has no equivalent of; a name neither implementation knows leaves the file unread here rather than read as utf-8, which cannot lose its bytes.

- **A sentinel header's `-encoding=utf8,.` field kept its comma.** The comma is Leo 4.2's field terminator, not part of the name (`at.parseLeoSentinel`), so a file the writer gave any encoding but `utf-8` did not read back.

- **A CRLF file was rewritten as LF.** `replace_file` compared bytes, where Leo's `compareIgnoringLineEndings` treats a file that differs only in its line endings as unchanged unless an `@lineending` directive asks for them. A write-all rewrote every CRLF file in the outline.

- **An `@auto` tree that writes nothing truncated its file.** Leo reports "not written" (`writeOneAtAutoNode`); an importer that produced an empty tree emptied the file it was read from.

- **`@edit`: an empty file replaced the body, and a node with children was written.** Leo's #391 leaves the node alone, because for `@edit` the body is the only copy of the text once the file is empty. The write refuses a node with children, whose text nothing else would reach the file.

- **A uA the reader unescaped was written back raw.** Only `str_` and `json_` values were quoted. A pickled value is hexlified, so quoting it changes nothing, but a value some other writer put there ended its attribute early and left a `.leo` file neither implementation could read.

- **The descendent-uA blob follows the tree.** `descendentVnodeUnknownAttributes` holds the uAs of the nodes a `.leo` file does not otherwise store -- the ones an importer or a sentinel file builds -- keyed by each node's position under the one that carries it. This port wrote back the blob it read, so after an insert, delete or move Leo's `restoreDescendentAttributes` handed those uAs to whichever node then sat at each position.

  The read now gives the blob's uAs to the nodes it names, once the external files are read, and the write rebuilds it from the tree, as Leo does. `pickle` reads and writes the part of Python's protocol 1 that Leo's blobs use: all 79 blobs in a leo-editor checkout parse, and re-emitting one gives CPython's own bytes back, so a blob nothing changed leaves no diff. One outside that subset stays as it was read, and a structural change drops it rather than let it name other nodes, with `SaveResult::dropped_descendent_uas` naming the tree -- in leotui's message too, because nothing in the outline records it afterwards.

  `promote`, `demote`, `move_to` and undo's `relink` move children without `link_child` or `cut_link`, so they now say so themselves; only the three linking primitives did, and a blob survived every one of those four.

- **A hard link was broken by the atomic rename.** The rename replaces the inode, so every other name for the file kept the old contents. Such a file is written in place, which is the one case where atomicity is given up for it. Leo writes every file in place.

- **A move inside an `@<file>` tree reaches the file.** `Document::move_to` set the dirty bit on the node it moved, where the write asks the `@<file>` node above it, so `find_files_to_write` had nothing to write. An `@file` tree lives in its file rather than in the `.leo` file, so the next read gave the old order back. Undo already marked both trees (`undo::relink`).

- **A re-read clone's children each gained a second parent link.** The scan cleared an existing vnode's children without removing it from their parent lists, so the nodes the read then relinked looked cloned, and `is_cloned` answered true for them.

- **An `@tabwidth` or `@lineending` value the port cannot use hid the ancestor's.** The scan stopped at the first directive of that name and fell back to the default. Leo's patterns do not match an unusable value, so the scan continues; `@tabwidth wide` in a node no longer discards the width its ancestor declares. `@encoding` and `@pagewidth` take the same route.

- **A `@comment` or `@delims` directive no longer loses the tree below it.** Python sentinels carry a space between the delimiter and the `@`, `# @+others`, and the `@+leo` line is the only place a reader can learn that. The scan took a later directive's delimiter literally, so every sentinel after it stopped matching: the file read back as one body, with no error, and the next save wrote that body over it. The space now carries across a directive. Leo drops it too and loses the same trees, so this port reads a file Leo wrote and Leo does not; the writer is unchanged, and still produces Leo's bytes.

- **The sentinel reader's warnings are reported.** They were collected and dropped, so a file with a line the reader kept but did not understand (#2213), or more `@first` lines than the header has, read silently. They arrive in `ReadResult::warnings`, on the channel the importers already use.

### Added

- **One corpus case per feature.** `demo/cases/directives` held six `@<file>` kinds in one outline, and nothing at all held `@path`, `@others`, `@all`, `@ignore`, `@first`, `@last`, `@comment`, `@delims`, `@language`, `@tabwidth`, `@section-delims`, a section reference, a doc part or a uA. There are 26 feature cases now, one directive each, and `demo/README.md` indexes them. A case has to read as Python Leo reads it, rewrite its `.leo` file unchanged and leave its external files alone; leo-editor's copy is held to the same three. The tangle test gates on each file rather than each case, and fails a case none of whose files it compared: `@nosent` and `@asis` are never read, so their own cases record `read_external: false`, and the per-case gate had skipped the only two kinds whose files are write-only.

  Building them found that Leo reads a section reference back with its delimiters regex-escaped, `\{ imports \}`, because the reader assigns the escaped delimiters to `section_delim1` (`leoAtFile.py:4016`). This port keeps what the file spells, and `corpus.rs`'s `KNOWN` records the difference. Two further Leo bugs they turned up, and cannot hold, are in `TODO.md`.

### Changed

- **`external::replace_file`'s third parameter is `ignore_line_endings: bool`.** It was an `encoding` the function never read.

- **`outline::is_valid_encoding` answers for the shape of a name, not a list of seven.** It decides whether an `@encoding` names an encoding at all; what this port can write is `external::encoding_is_supported`, which it already was.

- **`Outline::mod_time_cache` holds a `SystemTime`** rather than whole seconds since the epoch.

## [0.3.0]

### Fixed

- **A write refuses a node that nothing includes.** A child of an `@file` or `@clean` node that neither `@others` nor a section reference reaches was left out of the file, and the write reported success. After the next save the node was gone from the outline too. The write now fails with "orphan node", as Leo's `warnAboutOrphandAndIgnoredNodes` does. `tangle` does not check, as Leo's `atFileToString` does not.

- **gnxs no longer collide.** Each `Outline` had its own allocator, whose counter restarts every second. Three open-insert-save cycles within one second gave three nodes one gnx, and the next read merged them. One allocator now serves the process, as in Python leolib. `new_vnode` also skips a gnx the outline already holds, which covers another process writing the file in the same second. `Outline::ni` is removed.

- **A write refuses a file changed on disk since it was read.** `w` replaced another editor's changes without a word. Each file's size and mtime are recorded when it is read or written. A write whose file no longer matches fails with `Error::ChangedOnDisk`, and is listed in `WriteResult::changed_on_disk`. leotui asks before overwriting, and names changed files when the terminal regains focus. Size and mtime over a hash: a hash reads every file on each write, and the stamp misses only a same-size edit within the mtime's resolution.

- **Quitting and `:e` see unwritten `@file` edits.** Both checked `outline.changed`, which saving the `.leo` file clears. An `@file` tree's text is not in the `.leo` file, so its edits were dropped without a prompt. Both now also count dirty `@<file>` nodes.

- **`g<` (promote) is undoable and marks the outline changed.** It called `Outline::promote` directly: `u` undid the previous change, and `q` quit without asking. `Document::promote` records the moves, as `demote` does. Undoing a move now also marks the `@file` trees involved dirty; an undone move after `w` was never written.

- **No panic on a multi-byte space in a `:`, `/` or headline input.** Completion sliced one byte past the first word, inside an NBSP (Option-Space on macOS) or U+3000. The panic lost the session.

- **`:wq` and `:x` stay open when the save fails.** They quit regardless, and the error was never shown.

- **A paste is inserted as text.** Bracketed paste was off, so a pasted newline acted as Enter and the rest ran as commands. Pasting `title` and `dd` into a headline renamed it, then cut a subtree. In the body a paste is one change; in a one-line input its line breaks become spaces.

- **`:w path` writes a copy.** It moved the outline to `path`, which also moved where relative `@file` paths resolve, and it overwrote an existing file without asking. `:w path` is now vim's `:w path` and Leo's `save-to`. The new `:saveas path` is Leo's `save-as`. Both refuse an existing file until given `!`, as vim does.

- **The `.leo` file is written atomically**, through the temporary file and rename that external files use. It was written in place, so a crash or a full disk truncated the outline.

- **A write follows a symlink and refuses a read-only file.** The rename replaced a symlink with a regular file and left its target stale. It also replaced a 0444 file regardless of its mode.

- **A write refuses a missing directory.** `replace_file` created any directory a headline named: writing an outline whose files were absent created 375 stub files in 24 directories. `Config::create_nonexistent_directories` now decides; it was defined but never read. Leo's default also refuses (`at.precheck`, #1450).

- **An external file that is not UTF-8 is no longer rewritten as UTF-8.** The reader decoded every external file with `from_utf8_lossy`, so a latin-1 file arrived with U+FFFD in place of each high byte, and the writer ignored the encoding it was handed. Editing any node in such a tree and saving replaced the user's source -- `name = 'caf\xe9'` became `name = 'caf\xef\xbf\xbd'` -- and neither the read nor the write reported anything.

  Leo decodes with the file's own encoding and encodes with it again, from Python's `codecs`. Rust's standard library gives only UTF-8, so matching Leo means a new dependency; refusing does not. Such a file is now reported in `ReadResult::errors` and left unread, which leaves `may_overwrite` to refuse the write. `@nosent` is never read and `@clean` is exempt from `may_overwrite`, so the write side checks too, and the file is kept out of `WriteResult::refused`: approving that prompt would write UTF-8 over the bytes. `atfile_read::read_into_root` returns `Result<()>` rather than `bool`, to carry the reason.

- **A `.leo` file that is not UTF-8 no longer opens.** It was decoded the same lossy way, where the loss is worse: the replacement lands in a headline or a body, and the next save writes it over the outline itself. Opening now fails with `Error::NotUtf8`. Leo reads those bytes with an XML parser, which honours the encoding in the prolog. `write_leo_file` refuses a `leo_file_encoding` it cannot produce, since the prolog copies that name while the bytes are always UTF-8.

### Changed

- **leotui parses its command line with `clap`.** The hand-written parser printed `--help` to stderr with exit status 2, and a second file argument silently replaced the first. `--help` now prints to stdout and exits 0, and a second file is an error. `--press` may be repeated, its specs joining in order. clap adds 377KB to the release binary, 13.09MB to 13.47MB.

- **`Ctrl-s`, `:w`, `:w path` and `:saveas` write the dirty external files after the `.leo` file.** They wrote the `.leo` file alone and reported `saved`, leaving `@file` edits only in memory. An `@clean` edit was lost on reopen: its text is in the `.leo` file, but the read merges the unwritten file over it. Leo's `save`, `save-to` and `save-as` write both. `:write-outline-only` writes the `.leo` file alone.

  The `.leo` file goes first, where Leo writes it last, so the outline's edits reach disk however the files fare. A file that fails does not stop the others; it stays dirty, and the next save retries it. If the `.leo` file is not saved, by an error or a declined prompt, no file is written. `@nosent` and `@asis` files are never read back, so one newer than its `.leo` file would reopen stale and be overwritten by the next write.

- **One error type, `leolib::Error`.** The crate answered with three: `Box<dyn Error>` from `open_outline` and `Document::open`, `LeoFileError` from the `.leo` reader, and `String` everywhere else. A caller could not tell a missing file from a failed importer from a refused overwrite without matching on message text. `LeoFileError` is gone; its variants are `Error::NotALeoFile` and `Error::BadXml`. `external::FileReport` carries the error itself rather than a rendered string, so a front end can offer a prompt for `RefusedOverwrite` and nothing at all for `UnsupportedEncoding`. Warnings, which are not errors, move to `external::FileNote`.

- **`Outline::scan_from` walks instead of listing.** `next_marked`, `prev_marked` and `next_clone` built `all_positions()` and searched it: 665us per call on an 11,598-position outline, per keystroke. Stepping with `thread_next`/`thread_back` and stopping where it started takes 27ns when the match is a row away, 304us when there is none at all. `Outline::position_is_linked` is new, and rejects a position the walk could not come back to.

- **`atclean`, `atfile_write` and `seqmatch` are `pub(crate)`.** Nothing outside the crate used them, and every item in them was semver-visible. `AtWrite::orphans` and `AtWrite.explicit_line_ending` went with the narrowing: both were dead, and only their visibility had hidden it. `langdata` stays public: it is Leo's tables, and a front end has reason to read them.

- `corpus.rs`'s tangle test skips files the read reported unread. They have nothing in the outline to reproduce.

### Added

- **`leotui --version`**, and `-V`, print the package version.

- **`:refresh-from-disk` and `:read-at-file-nodes`**, Leo's commands to read external files again: the `@<file>` node at or above the selection, or every one at or under it. Both ask before discarding unwritten edits. Both clear the undo history, as Leo's do, because the history names nodes the read replaces. `Document::read_files` and `external::read_files` are the library side.

- **`:e!` and `:revert`** open the `.leo` file again, discarding every change.

- **`leolib::save_all` and `Document::save_all`** write the `.leo` file, then every dirty external file, and report both in `SaveResult`. No file is written if the `.leo` write fails. `save` and `write_external_files` still do one half each.

- **`leolib::save_to` and `Document::save_to`** write a copy without changing the outline's file name.

- **Corpus case `empty_auto`**: an `@auto` file with nothing in it. Leo's `at.readFileAtPosition` raises `AttributeError: 'NoneType' object has no attribute 'v'` on it (`leoAtFile.py:642`, leo-editor `b6e06060ad`) and reports the file unread; this port imports the empty tree. Found by comparing the two importers over 591 files, where it was the only difference in all 12 cases that differed. `KNOWN` in `corpus.rs` records it, so the test says so when Leo is fixed.

- **What a `.leo` file can write**, in the README. An `@<file>` headline or `@path` can name any absolute path, climb out with `..`, or expand `~`. The write guards refuse an unread file, a file changed on disk and a missing directory, but not a new file in an existing directory. So an outline from someone else is as dangerous as a Makefile from someone else. Leo is the same.

- **GitHub Actions.** `make check` on push and pull request, and `make corpus` against a pinned leo-editor commit. Nothing ran the checks the Makefile had. `make audit` stays local: it fetches the RustSec database, and is run by hand.

- **Corpus case `unreadable`**: an `@file` whose file has no sentinels, which both implementations report unread, beside an `@clean` file that reads. No case had an unread file, so nothing checked that Python and Rust agree on which files are unread.

## [0.2.1]

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
