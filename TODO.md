# TODO

## Critical

## High

## Medium

- [x] **Tab completion from the language server.** Done; additional edits (auto-imports) and snippets are not applied.

- [ ] **Syntax colouring from the language server.** Semantic tokens (`textDocument/semanticTokens/full`) name what tree-sitter cannot know: a parameter, a type from another file, a macro, a read-only variable. Request them per document, map each token's line and column to a body row as diagnostics are mapped, and lay them over `highlight`'s spans, tree-sitter staying the colouring for a node with no server. The token types go to Helix scopes (`variable.parameter`, `type`, `function.macro`) so themes colour them. Tokens arrive as deltas against the previous set, and a body edited since the request needs its tokens moved or dropped.

### leoegui

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
- [ ] Performance, in this order (detail and measurements in the roadmap's Performance section):
  - [x] Language-server sync: a selection move with 20 documents open went from 99 ms to 0.01 ms, an edit from 99 ms to 6.8 ms (roadmap Performance 1).
  - [ ] Colour the visible lines first on a body's first visit (46 ms at 5,000 lines).
  - [ ] Keep a few colourings by node, so switching between large bodies does not recolour.
  - [ ] Split the body once a frame and key the colouring on node and generation, not a hash (2 ms a frame at 5,000 lines).
  - [x] Line offsets for mapping diagnostics: 8 ms to 8.7 us for 500 diagnostics in a 6,300-line file.
  - [x] A gnx index in leolib, for tabs and lookups: 1.2 ms to 1.5 us at 11,600 nodes.
  - [x] Measure startup and the glow and wgpu renderers. Glow: 0.09 s and 117 MB to the first frame against wgpu's 0.28 s and 198 MB. Switching is a decision (roadmap Performance 7); `strip` saves 10% and is not applied.
- [ ] Scripting and `@button` (needs a design).
- [ ] Two views of one outline (needs a design).

### Other

- [ ] Run the `fuzz/` targets under libFuzzer. They compile on stable but need nightly and `cargo-fuzz` to run, and neither was installed when they were written. A one-minute random-mutation run over `demo/` found the pickle allocation bug and nothing else.

- [ ] Port `@jupytext`. It is refused on read and write now. Leo reads a notebook as `@clean` over the `py:percent` text jupytext makes of it (`at.readOneAtJupytextNode`), and writes that text back through jupytext (`writeOneAtJupytextNode`). A port has to do the conversion both ways and keep the cells' outputs and metadata, which the text does not carry.
