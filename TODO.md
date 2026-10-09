# TODO

## Critical

### leolib

- [x] **A new `@clean` or `@nosent` node overwrites an existing file it never read.** `Outline::may_overwrite` (`outline.rs:891`) exempts both kinds. Typing `@clean b.py` over a 12-byte `b.py` and saving left 0 bytes, with no prompt. Through MCP, a client with edit and save rights can write any file the user can (`@nosent ~/.bashrc`). Leo has the same exemption (`shouldPromptForDangerousWrite`).

- [x] **External edits to an `@clean` file are overwritten without notice.** A file that matches the tree on open gets no stamp (`atclean.rs:114,134`, `external.rs:350`), so `changed_files()` never reports a later edit. With `--no-external`, a save writes the `.leo` copy over the file.

- [x] **An `@file` with no `@-leo` line reads as success with every body empty** (`atfile_read.rs:611`). A truncated file shows empty nodes with no error, and the next save writes them.

- [x] **Gnxs are written to `.leo` unescaped** (`leofile.rs:391`, `put_t_elements`). `t="a&amp;b.1"` saves as `t="a&b.1"`, which the next open refuses. The id comes raw from `$USER` (`gnx.rs:62`); Leo's `cleanLeoID` strips `.`, `,`, quotes and whitespace.

- [x] **The Python importer panics on a backslash before a non-ASCII character** (`importers/python.rs:81-83`). `y = "\é"` under `@auto` crashes at open.

- [x] **A node listed as its own child hangs the reader** (`leofile.rs:255`). `<v t="a.1"><vh>x</vh><v t="a.1"/></v>` runs out of memory.

### leo-markdown (blocks publishing)

- [x] **A body with no trailing newline joins the next heading onto it** (`write_own`, `heading`). `text` then `## B` is written as `text## B`, with 0 errors, and the headings are gone on reopen.

- [x] **Every `\r` is deleted on read, and a BOM is dropped.** A CR-only file loses all line breaks on its first write; `a\rb` in code becomes `ab`. The module doc claims byte for byte.

- [x] **A refused read erases `str_leo-rs-cell-ids`** (`save_cell_ids`). The `@clean` clones lose their link for good.

- [x] **A display fence whose first line is `<< name >>` made the file unsaveable.** Fixed: the refused file is written as its body holds it. The read is still refused, so such a file opens as one node; reading it as text needs the writer to tell a deleted cell from a display fence.

- [ ] **A refused write leaves the tree's edits only in memory.** Deleting, cloning or reordering a cell, or adding a child under one, is refused at write, and the `.leo` file does not store the tree; the quit guard asks first. Current Leo does the same for any `@<file>` (`addToOrphanList`, the "Not Written" dialog). Storing a failed tree in the `.leo` file would go beyond Leo: a design decision.

- [x] **The packaged tests read files outside the package.** `tests/cells.rs` reads `../leo-entangled/tests/data` and `../../demo/entangled`; `cargo test` on the crates.io tarball fails.

## High

### Plugins

- [ ] **Use `@entangled` on real documents, then release it.** leotui and leogui register `@qmd` and `@rmd` through `leo-plugins`, and `leo-markdown` and `leo-plugins` are publishable. `leo-entangled` stays `publish = false` and out of `leo-plugins`, since crates.io refuses an unpublished dependency, even an optional one; `make ... ENTANGLED=1` builds and tests it.

- [x] **Quoted labels.** `#| label: "b c"` reads as `<< b c >>`, and a rename adds a second `#| label:` line, which Quarto rejects. knitr's `{r label='x y'}` reads as `<< x >>` and renames to `label='z y'`.

- [x] **An indented fence with a whitespace-only line is refused**: the line loses its indent and the read check fails.

- [x] **Edits that change structure silently.** Done: an underlined heading renamed to text that would not read as one is written with `#`; a heading node with no headline, and a cell whose code would close its fence, are refused at write; `#` lines in an HTML comment spanning lines stay text. Open: `#` lines in unclosed YAML front matter still become headings.

- [x] **`leo-markdown` API before publishing.** Done: unused items private, `get`/`set` hidden, `FENCE_OPEN` replaced by `fence_opener_info`, `std::sync::LazyLock` for `once_cell`, `#[non_exhaustive]` on `FenceInfo`, `Style` and `Policy`, keywords, categories and a crate README. Not done: `leolib::ext::Rename`, which leo-markdown constructs from outside leolib.

- [x] **`leo-plugins` tests fail under `--no-default-features`.** They assume the `markdown` feature.

- [x] **A failed `register()` is silent in release builds.** The `debug_assert!` goes, and `@qmd` nodes open as plain nodes.

- [x] **An error names the wrong plugin**: renaming a cell to `mk df` says "an entangled name has no spaces" (`leo-markdown/src/lib.rs:862`).

### leolib

- [x] **Plugins: move the leo-rs-only kinds out of leolib.** Done (released in 0.7.0 as the API only): leolib is Leo plus `leolib::ext`; `@entangled` is in `leo-entangled`, `@qmd` and `@rmd` in `leo-markdown`, registered by `leo-plugins`. Design: `docs/dev/plugins.md`.

- [x] **`@qmd` and `@rmd`: Quarto and R Markdown with cells as nodes.** Unreleased plugin. Opt-in; `@auto` stays Leo's. Exact round trip, cells and labelled fences as nodes, labels, fenced divs, and labelled cells cloned into `@clean` trees that survive a reopen. Design: `docs/dev/markdown_importer.md`, `docs/dev/plugins.md`.

- [x] **Report a clone whose body differs between external files.** A node cloned into two external files takes the body of whichever file is read last, with no report, so an edit made in one file is lost when the other still holds the old text. Found with `@clean README.md` and `@file tests/test_readme.py` sharing an example node (`docs/dev/entangled_leo_backend.md`, the spike). Done: reported on read, both texts kept under `Recovered Nodes`. Python Leo keeps the last file's text without a word.

- [x] **Descendent uAs are lost when an external read fails.** Done: a node whose read failed keeps its blob parked and writes it back. Open: a blob the pickle reader cannot parse is still dropped on a restructure, since its positions change; that needs the fuller pickle reader (Low).

- [x] **Saves use one fixed temp name and no fsync** (`external.rs:1003`). Two instances saving at once share `{path}.leo-rs-tmp`.

- [x] **An `@clean` tree with no text outside its directives takes the file's text into its root body**, dropping `@others`, so its children fail the next write as orphans. Not changed: Leo's `readOneAtCleanNode` does the same, nothing is lost, and the write error stops the save. Reported as an empty file; it needs a tree with no text at all.

### leomcp

- [x] **Resource limits before auth.** The server allocates up to 16 MiB per request, reads unbounded lines and spawns a thread per connection before checking the token (`leomcp/src/lib.rs:156,188`).

- [x] **The token file's permissions.** `settings.toml` holding `mcp-token` is created with the umask (usually 0644).

### leoapp and leolsp (both frontends)

- [x] Performance, measured in leogui (detail in `docs/dev/gui-roadmap.md`, Performance):

  - [x] Language-server sync: a selection move with 20 documents open went from 99 ms to 0.01 ms, an edit from 99 ms to 6.8 ms (roadmap Performance 1).

  - [x] Colour the visible lines first on a body's first visit: 21 ms to 1.4 ms at 5,000 lines. The whole body is coloured on a thread.

  - [x] Keep four colourings by node: switching between two 5,000-line bodies went from 42 ms to 0.8 ms.

  - [x] Split the body once a frame, and check the colouring by comparing lines, not hashing them. An unchanged 5,000-line frame takes 0.27 ms in leoapp: 0.15 ms splitting and 0.05 ms in `language_of`.

  - [x] Line offsets for mapping diagnostics: 8 ms to 8.7 us for 500 diagnostics in a 6,300-line file.

## Medium

### Tests

- [x] A generated round trip for `leo-markdown`: headings, prose, `{lang}` and display fences, fenced divs, front matter, CRLF, no final newline. Assert `write_string(read(x)) == x` and that a save reports the file unchanged.

- [x] CI runs `make check ENTANGLED=1`, `leo-plugins` without default features, and a minute of `cargo fuzz run` per target, with new targets `read_leo` and `markdown`. The fuzz job has not run yet; a stable smoke run of 200k mutated inputs per new target found the `tx` attribute bug.

- [x] MCP: `save` writes the `.leo` and external files when allowed; a non-JSON body, an oversized Content-Length and a wrong path or method get errors and the server keeps answering.

- [x] An undo property test (`crates/leolib/tests/undo.rs`): 300 random sequences of edits. It found that demoting into, or moving right into, a node's own clone made a cycle, and that deleting the only top-level node emptied the outline; both fixed.

- [x] Corpus cases for the unpinned importers: `auto_block_languages` (`.rs`, `.java`, `.lua`, `.php`, `.c`) and `auto_typescript`, a known difference. The Rust case found the `$` bug in the importer patterns; fixed.

- [x] Timing: the LSP test `wait` helper polls by count (`leolsp/src/tests.rs:116`), and `leoapp/src/app/tests.rs:1329` asserts under 500 ms in a debug build.

- [x] An `@auto` write-back is checked against Leo: the corpus records `atAutoToString` for each `@auto` node (`auto_written`), and `every_at_auto_node_writes_back_as_python_leo_writes_it` compares. They agree.

### Usability

- [x] **Recovery.** leotui writes `NAME.recovered.leo` on a hangup, SIGTERM, a failed terminal or a panic. Not done: a `.leo~` backup on save, which the atomic save makes less needed, and recovery in leogui.

- [x] **The quit prompt has no save choice.** Offer save / quit / cancel, answered by one key in both front ends (leotui needs Enter, leogui does not).

- [x] **Save errors name the temp file and are cut off.** `save failed: .../demo.leo.leo-rs-tmp: Permiss`. Lead with the reason and the outline's name; wrap long lines in `:messages`.

- [x] **A malformed `.leo` file is reported as `no <leo_file> element`.** Report the unclosed element and its line. Without a terminal leotui says `Device not configured`.

- [x] **Command names.** The README claims every Leo command name; `save-file`, `exit-leo`, `execute-script` and others are missing. Add did-you-mean, a candidate list on a second Tab, and a way to list unbound commands.

- [x] **Headline editing.** A new outline's `newHeadline` starts selected; the README no longer says `Ctrl-u` clears a headline. No key clears a headline being edited, as `Ctrl-u` moves the node.

- [x] **`@qmd` and `@rmd` steps are undocumented**: creating one over an existing file, adding a cell (its fence and reference in the parent's body), renaming the root.

- [x] **Startup messages hide each other.** Settings warnings go only to `:messages` and do not name `~/.config/leo-rs/settings.toml`; `split-ratio = 500` and `--theme nosuch` are accepted silently.

- [x] **README order.** Install starts near line 47, after the conformance table; the leolib API precedes using leotui.

- [x] **leogui offers `.leojs` and `.db`** in its open dialog and file drop, then fails to open them.

### leolib

- [ ] Run the `fuzz/` targets under libFuzzer. They compile on stable but need nightly and `cargo-fuzz` to run, and neither was installed when they were written. A one-minute random-mutation run over `demo/` found the pickle allocation bug and nothing else.

- [x] A gnx index, for tabs and lookups: 1.2 ms to 1.5 us at 11,600 nodes.

### leoapp and leolsp (both frontends)

- [x] **Tab completion from the language server.** Done; additional edits (auto-imports) and snippets are not applied. With no server for the body, Tab after a dot names the missing `lsp-LANGUAGE` setting.

- [x] **Syntax colouring from the language server.** Done: full-document semantic tokens, asked for when the server's text changes, laid over tree-sitter's spans, mapped to Helix scopes; not while a change is typed. Deltas (`semanticTokens/full/delta`) are not asked for.

- [x] **`@wiki`: wikilinks in a markdown subtree.** Done in `leo-wiki` (`docs/dev/wiki.md`, with where it departs from the design).

### leogui

Each is described, with an effort estimate, in `docs/dev/gui-roadmap.md`.

- [x] Go to node: fuzzy quick open over every headline (Cmd-P).

- [x] External-file status in the outline: unwritten, changed on disk, unread, refused; Reload or Keep.

- [x] Language-server code actions and quick fixes.

- [x] Find panel: Leo's Find tab, with replace and `clone-find-all`.

- [x] Several outlines: tabs, Open Recent, native Open and Save As dialogs. Separate windows are not done.

- [x] Rendered view of markdown and image nodes, and a plugin kind's headings (unreleased). reStructuredText is shown as text: there is no Rust renderer for it.

- [x] Clone navigation: clone count on the row, and a list of a node's clones.

- [x] Hoist banner with a de-hoist button.

- [x] Find references gathered as clones under a `Found` node.

- [x] Signature help and format document.

- [x] Language-server status and log.

- [x] Matching bracket highlight; `@pagewidth` ruler, whitespace and indent guides. Sticky headers are not done.

- [x] Multi-select in the outline: Delete, Mark and drag act on every chosen row. Other commands act on the current row.

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

### Leo coverage

From the 2026-10-08 review; `docs/dev/delta.md` has the command count.

- [x] `@settings` and `myLeoSettings`, for the seven settings that change what is read or written (`leolib::settings`). Keybindings, `@data`, `@button` and the rest are not read.

- [x] `@ifenv` and `@ifhostname` in `@settings`, as Leo tests them. `@if EXPRESSION` is Python, and its settings are still skipped.

- [x] The gnx id from `~/.leo/.leoID.txt`, which Leo reads before the login name.

- [x] `$VAR` in `@path` and file names (`util.rs:367` expands only `~`).

- [ ] `@persistence`. The pickle reader now reads and writes floats and tuples as CPython does; sets and bytes, pickled as calls to a global, are still refused.

- [ ] `%NAME%` in paths on Windows, as `ntpath.expandvars` does. Needs a Windows machine to test.

- [ ] Chapters and bookmarks. UNLs are followed (`gd` on `unl:gnx://`, `unl://` and `gnx:`), not yet made.

- [x] Two consecutive doc parts read the second `@` back as `@+at`. Leo does the same (corpus case `doc_parts_twice`), so this is Leo's behaviour.

- [ ] `@edit` drops content lines that parse as directives (`external.rs:902`), as Leo's `writeOneAtEditNode` does.

- [x] A CRLF `.leo` file reads with LF, as XML and Leo read it (corpus case `leo_crlf`); `&#13;` stays a CR.

### leolib

- [x] **Literate markdown with entangled**: the `@entangled` kind (unreleased plugin). Phase 1 is done: the kind, its CommonMark scanner, named fences as `<< name >>` nodes, the exact writer and its read-time check, and fence languages for colouring and the language servers. Phase 2 is done: `:entangled-tangle` and `:entangled-check` through the CLI. Phase 4 is done: `demo/entangled/` tests a README's examples through a `tests.md` harness. Phase 1b is done: editing a fence node's headline renames the block everywhere the outline names it, as one undo step. Phase 3 is done: `include=path` and `include=path#anchor` fill a read-only fence from a file tested on its own. Heading levels follow the tree: a heading node moved to another depth moves its `#` level by as much. The `@entangled` plan is complete. Design and plan in `docs/dev/entangled_leo_backend.md`.

- [ ] Port `@jupytext`. It is refused on read and write now. Leo reads a notebook as `@clean` over the `py:percent` text jupytext makes of it (`at.readOneAtJupytextNode`), and writes that text back through jupytext (`writeOneAtJupytextNode`). A port has to do the conversion both ways and keep the cells' outputs and metadata, which the text does not carry.

### leoapp

- [ ] **Incremental colouring, as Helix does it** (possible optimization; needs a design). `tree-sitter-highlight` parses from scratch on every call (it passes no old tree to `parse_with_options`) and queries the whole tree, so a long body is coloured whole: 21 ms at 5,000 lines, now on a worker thread. Instead, keep a `tree_sitter::Tree` per cached body, apply each change with `Tree::edit` and re-parse incrementally, and run the highlight query on the visible lines only (`QueryCursor::set_byte_range`). That would remove the partial colouring, the worker and `patch`. Costs: line edits must become byte-offset `InputEdit`s, and injections, `@language` regions and `plan`'s masking move to the lower-level API. Measured on a 5,000-line Python body (2026-10-07, Apple silicon): the whole highlight 21 ms, of which a fresh parse is 12 ms and a whole-tree query 6.7 ms; a query over 50 visible lines 0.07 ms. An incremental re-parse after one typed character costs 0.25 ms in a name or a comment, 6.7 ms for an unclosed quote, and 12 ms when it breaks a keyword (`def` to `qdef`), where error recovery re-parses everything below. So a keystroke would cost 0.3 to 12 ms on the UI thread, against `patch`'s 1.8 ms now, unless the parse stays on the worker; a first visit still needs the 12 ms parse.

### leogui

- [ ] On Windows, a release build prints nothing to a terminal: `--help`, `--version` and open errors are lost, because it uses the GUI subsystem. Attach to the parent's console at startup (`AttachConsole`, through `windows-sys`). Needs a Windows machine to test.

- [ ] On macOS, ship a signed `.app` bundle rather than a bare binary, which Gatekeeper blocks until `xattr -d com.apple.quarantine`. leotui's binary has the same problem. Needs an Apple developer account.

- [ ] Scripting and `@button` (needs a design).

- [ ] Two views of one outline (needs a design).
