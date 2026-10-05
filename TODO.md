# TODO

## Critical

## High

### Upstream (Leo)

Leo's bugs, which this port does not reproduce. Nothing to change here.

- [ ] **Leo's `descendentVnodeUnknownAttributes` blob is not stable across a read.** Its pickled dict comes back in another key order, so opening a `.leo` file with two uAs on one node and saving it rewrites the file with no edit. `demo/cases/uas` puts one uA per node to stay inside leo-editor's "rewritten unchanged" test. This port rebuilds the blob from the tree, in the key order the tree gives.

- [ ] **Leo reads a section reference back with its delimiters regex-escaped** when `@section-delims` set them (`leoAtFile.py:4016` assigns `re.escape`'d delims to `section_delim1`), so `{ imports }` becomes `\{ imports \}` in the body it hands back. This port keeps what the file spells. Report upstream; `corpus.rs`'s `KNOWN` holds it meanwhile.

## Medium

- [ ] **Tab completion from the language server.** In INSERT, Tab (or vim's `Ctrl-n`) asks the server for `textDocument/completion` at the cursor and shows the items in a list: a popup in leoegui, the drop-down above the status line in leotui. The chosen item's `textEdit` maps back to the body like a rename's edits, and is refused if it crosses a line the body does not write. The servers see committed text only (`app/lsp.rs`), so the working copy has to be synced first, or the request made at the last commit's position. Tab is INSERT's indent today, so completing on Tab has to fall back to a tab where the server has nothing.

- [ ] **Syntax colouring from the language server.** Semantic tokens (`textDocument/semanticTokens/full`) name what tree-sitter cannot know: a parameter, a type from another file, a macro, a read-only variable. Request them per document, map each token's line and column to a body row as diagnostics are mapped, and lay them over `highlight`'s spans, tree-sitter staying the colouring for a node with no server. The token types go to Helix scopes (`variable.parameter`, `type`, `function.macro`) so themes colour them. Tokens arrive as deltas against the previous set, and a body edited since the request needs its tokens moved or dropped.

### leoegui

Each is described, with an effort estimate, in `docs/dev/gui-roadmap.md`. The first five are the recommended order.

- [ ] Go to node: fuzzy quick open over every headline (Cmd-P).
- [ ] External-file status in the outline: unwritten, changed on disk, unread, refused; Reload or Keep.
- [ ] Language-server code actions and quick fixes.
- [ ] Find panel: Leo's Find tab, with replace and `clone-find-all`.
- [ ] Several outlines: tabs or windows, Open Recent, native Open and Save As dialogs.
- [ ] Rendered view of markdown, reStructuredText and image nodes.
- [ ] Clone navigation: clone count on the row, and a list of a node's clones.
- [ ] Hoist banner with a de-hoist button.
- [ ] Find references gathered as clones under a `Found` node.
- [ ] Signature help and format document.
- [ ] Language-server status and log.
- [ ] Matching bracket highlight; `@pagewidth` ruler, whitespace and indent guides; sticky headers.
- [ ] Multi-select in the outline.
- [ ] Drop a file on the outline to import it as `@auto`.
- [ ] Session restore.
- [ ] Performance, in this order (detail and measurements in the roadmap's Performance section):
  - [ ] Measure language-server sync on a large `@file` tree; then re-render only dirty documents, and keep each document's root position.
  - [ ] Colour the visible lines first on a body's first visit (46 ms at 5,000 lines).
  - [ ] Keep a few colourings by node, so switching between large bodies does not recolour.
  - [ ] Split the body once a frame and key the colouring on node and generation, not a hash (2 ms a frame at 5,000 lines).
  - [ ] Line offsets for mapping diagnostics.
  - [ ] A gnx index in leolib, for tabs and lookups (1.7 ms a commit at 21,000 nodes).
  - [ ] Measure startup and the glow and wgpu renderers; `strip = true` for size.
- [ ] Scripting and `@button` (needs a design).
- [ ] Two views of one outline (needs a design).

### Other

- [ ] Run the `fuzz/` targets under libFuzzer. They compile on stable but need nightly and `cargo-fuzz` to run, and neither was installed when they were written. A one-minute random-mutation run over `demo/` found the pickle allocation bug and nothing else.

- [ ] Port `@jupytext`. It is refused on read and write now. Leo reads a notebook as `@clean` over the `py:percent` text jupytext makes of it (`at.readOneAtJupytextNode`), and writes that text back through jupytext (`writeOneAtJupytextNode`). A port has to do the conversion both ways and keep the cells' outputs and metadata, which the text does not carry.
