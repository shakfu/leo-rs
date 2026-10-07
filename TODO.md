# TODO

## Critical

## High

### leolib

- [x] **Report a clone whose body differs between external files.** A node cloned into two external files takes the body of whichever file is read last, with no report, so an edit made in one file is lost when the other still holds the old text. Found with `@clean README.md` and `@file tests/test_readme.py` sharing an example node (`docs/dev/entangled_leo_backend.md`, the spike). Done: reported on read, both texts kept under `Recovered Nodes`. Python Leo keeps the last file's text without a word.

### leoapp and leolsp (both frontends)

- [x] Performance, measured in leogui (detail in `docs/dev/gui-roadmap.md`, Performance):

  - [x] Language-server sync: a selection move with 20 documents open went from 99 ms to 0.01 ms, an edit from 99 ms to 6.8 ms (roadmap Performance 1).

  - [x] Colour the visible lines first on a body's first visit: 21 ms to 1.4 ms at 5,000 lines. The whole body is coloured on a thread.

  - [x] Keep four colourings by node: switching between two 5,000-line bodies went from 42 ms to 0.8 ms.

  - [x] Split the body once a frame, and check the colouring by comparing lines, not hashing them. An unchanged 5,000-line frame takes 0.27 ms in leoapp: 0.15 ms splitting and 0.05 ms in `language_of`.

  - [x] Line offsets for mapping diagnostics: 8 ms to 8.7 us for 500 diagnostics in a 6,300-line file.

## Medium

### leolib

- [ ] Run the `fuzz/` targets under libFuzzer. They compile on stable but need nightly and `cargo-fuzz` to run, and neither was installed when they were written. A one-minute random-mutation run over `demo/` found the pickle allocation bug and nothing else.

- [x] A gnx index, for tabs and lookups: 1.2 ms to 1.5 us at 11,600 nodes.

### leoapp and leolsp (both frontends)

- [x] **Tab completion from the language server.** Done; additional edits (auto-imports) and snippets are not applied. With no server for the body, Tab after a dot names the missing `lsp-LANGUAGE` setting.

- [ ] **Syntax colouring from the language server.** Semantic tokens (`textDocument/semanticTokens/full`) name what tree-sitter cannot know: a parameter, a type from another file, a macro, a read-only variable. Request them per document, map each token's line and column to a body row as diagnostics are mapped, and lay them over `highlight`'s spans, tree-sitter staying the colouring for a node with no server. The token types go to Helix scopes (`variable.parameter`, `type`, `function.macro`) so themes colour them. Tokens arrive as deltas against the previous set, and a body edited since the request needs its tokens moved or dropped.

- [ ] **`@wiki`: wikilinks in a markdown subtree** (leo-rs only; proposed, nothing built). An `@wiki <name>` node's descendants are markdown pages linked by `[[Page]]`, `[[Parent/Page]]` and `[[other:Page]]`; `export-wiki` writes the subtree to `<name>.md` with the links made markdown links. Also `open-url-under-cursor` (`gd`, `gf`) extended from `<< section >>`, its only branch today, to wiki links, `gnx:` and UNLs; `Ctrl-o` for `go-back`; `[[` headline completion through the completion list; renames that rewrite links; and `wiki::check` enforcing the note's five constraints on load, edit and export. Design and open questions in `docs/dev/wiki.md`, revised 2026-10-07 for leoapp.

### leogui

Each is described, with an effort estimate, in `docs/dev/gui-roadmap.md`.

- [x] Go to node: fuzzy quick open over every headline (Cmd-P).

- [x] External-file status in the outline: unwritten, changed on disk, unread, refused; Reload or Keep.

- [x] Language-server code actions and quick fixes.

- [x] Find panel: Leo's Find tab, with replace and `clone-find-all`.

- [x] Several outlines: tabs, Open Recent, native Open and Save As dialogs. Separate windows are not done.

- [ ] Rendered view of markdown, reStructuredText and image nodes.

- [x] Clone navigation: clone count on the row, and a list of a node's clones.

- [x] Hoist banner with a de-hoist button.

- [ ] Find references gathered as clones under a `Found` node.

- [ ] Signature help and format document.

- [x] Language-server status and log.

- [ ] Matching bracket highlight; `@pagewidth` ruler, whitespace and indent guides; sticky headers.

- [ ] Multi-select in the outline.

- [x] Drop a file on the outline to import it as `@auto`.

- [x] Session restore.

- [x] Measure startup and the glow and wgpu renderers. Glow: 0.09 s and 117 MB to the first frame against wgpu's 0.28 s and 198 MB. Switching is a decision (roadmap Performance 7); `strip` saves 10% and is not applied.

- [x] A selected list item takes the theme's `ui.menu.selected` text colour.

- [x] Renamed from leoegui; `leogui` 0.6.0 is on crates.io.

- [x] Release archives on every platform, and no console window on Windows release builds.

### leotui

Nothing open.

### Workspace

- [x] Settings, session, recent outlines and themes in `~/.config/leo-rs/`, renamed from `~/.config/leotui/` on first start.

- [x] Each crate ships a copy of `LICENSE`; `make lint` checks the copies match.

## Low

### leolib

- [ ] **Literate markdown with entangled**: the `@entangled` kind. Phase 1 is done: the kind, its CommonMark scanner, named fences as `<< name >>` nodes, the exact writer and its read-time check, and fence languages for colouring and the language servers. Next: renaming a fence node (1b), `:entangled-tangle` and `:entangled-check` through the CLI (2), `include=` (3), and the Python harness (4). Design and plan in `docs/dev/entangled_leo_backend.md`.

- [ ] Port `@jupytext`. It is refused on read and write now. Leo reads a notebook as `@clean` over the `py:percent` text jupytext makes of it (`at.readOneAtJupytextNode`), and writes that text back through jupytext (`writeOneAtJupytextNode`). A port has to do the conversion both ways and keep the cells' outputs and metadata, which the text does not carry.

### leoapp

- [ ] **Incremental colouring, as Helix does it** (possible optimization; needs a design). `tree-sitter-highlight` parses from scratch on every call (it passes no old tree to `parse_with_options`) and queries the whole tree, so a long body is coloured whole: 21 ms at 5,000 lines, now on a worker thread. Instead, keep a `tree_sitter::Tree` per cached body, apply each change with `Tree::edit` and re-parse incrementally, and run the highlight query on the visible lines only (`QueryCursor::set_byte_range`). That would remove the partial colouring, the worker and `patch`. Costs: line edits must become byte-offset `InputEdit`s, and injections, `@language` regions and `plan`'s masking move to the lower-level API. Measured on a 5,000-line Python body (2026-10-07, Apple silicon): the whole highlight 21 ms, of which a fresh parse is 12 ms and a whole-tree query 6.7 ms; a query over 50 visible lines 0.07 ms. An incremental re-parse after one typed character costs 0.25 ms in a name or a comment, 6.7 ms for an unclosed quote, and 12 ms when it breaks a keyword (`def` to `qdef`), where error recovery re-parses everything below. So a keystroke would cost 0.3 to 12 ms on the UI thread, against `patch`'s 1.8 ms now, unless the parse stays on the worker; a first visit still needs the 12 ms parse.

### leogui

- [ ] On Windows, a release build prints nothing to a terminal: `--help`, `--version` and open errors are lost, because it uses the GUI subsystem. Attach to the parent's console at startup (`AttachConsole`, through `windows-sys`). Needs a Windows machine to test.

- [ ] On macOS, ship a signed `.app` bundle rather than a bare binary, which Gatekeeper blocks until `xattr -d com.apple.quarantine`. leotui's binary has the same problem. Needs an Apple developer account.

- [ ] Scripting and `@button` (needs a design).

- [ ] Two views of one outline (needs a design).
