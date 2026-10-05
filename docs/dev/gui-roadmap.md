# leoegui roadmap

Features proposed for leoegui on 2026-10-05, after the GUI redesign, the Leo key bindings and the Helix theme work. Effort is a rough estimate against this codebase: small is a day or less, medium a few days, large needs a design first. Done: go-to-node, external-file status, code actions, the find panel and several outlines (see `CHANGELOG.md`). Language-server completion and semantic colouring are in `TODO.md` already.

The first five, by value for effort: go-to-node, external-file status, code actions, the find panel, several outlines.

## Leo's own features

- **Find panel**, Leo's Find tab (small to medium). Headline or body, regex, whole word, and a scope of outline, subtree or marked nodes. Results are a clickable list, with replace and a `clone-find-all` button. Today there is only `/`.
- **Go to node**, quick open on Cmd-P (small). Fuzzy search over every headline, then select the node. The command palette's scoring and window carry over.
- **Several outlines** (medium). A tab or window per `.leo` file, File > Open Recent, and native Open and Save As dialogs (the `rfd` crate) where paths are typed now. `delta.md` lists all three as missing.
- **Rendered view**, Leo's `viewrendered` (medium). A pane showing a markdown, reStructuredText or image node rendered. `egui_commonmark` renders markdown.
- **External-file status in the outline** (small). A badge on an `@<file>` node that is unwritten, changed on disk, unread or refused, and a bar offering Reload or Keep for a file changed on disk. `ReadResult` and `WriteResult` already hold the facts.
- **Clone navigation** (small). The number of clones on the row, and a list of a node's clone positions to jump between.
- **Hoist banner** (small). The hoisted node at the top of the sidebar, with a button to de-hoist.

## Language servers

- **Code actions and quick fixes** (small). clangd offers fixes on finlib's warnings ("fix available"); applying one is a workspace edit, which the rename path already maps and refuses whole.
- **Find references into a node** (medium). The references gathered as clones under a `Found` node, as `clone-find-all` gathers matches. No other editor can show references this way.
- **Signature help and format document** (medium). Parameter hints while typing; formatting is a whole-file edit through the line map.
- **Server status** (small). Running, failed or stopped on the status bar, and the server's log in the bottom panel.

## The body

- **Matching bracket highlight** (small), from the theme's `ui.cursor.match`.
- **Ruler, whitespace and indent guides** (small). A ruler at `@pagewidth`; `ui.virtual.ruler`, `ui.virtual.whitespace` and `ui.virtual.indent-guide` colour them.
- **Sticky headers** (medium). The enclosing function or class line pinned at the top of a long body.

## The outline and the window

- **Multi-select** (medium). Shift-click and Cmd-click several rows to move, mark or delete them together. leoapp's commands act on one node, so each needs a form for many.
- **Drop a file to import it** (small). A file dragged from Finder becomes an `@auto` node.
- **Session restore** (small). The open outlines, tabs and panel sizes, as Leo's `leo.session` keeps them.

## Performance

Proposed after the 2026-10-05 profile (release build, Apple M1, the 21,020-node stress outline with a 5,000-line body), which already cut a stress frame from 6.2 ms to 0.2 ms and a key in the big body from 35 ms to 1.8 ms. Measured costs are marked as such; the rest come from reading the code and need measuring first. In recommended order:

1. **Language-server sync** (done). Measured 2026-10-05 on leo-editor's `LeoPyRef.leo` with its files (11,596 nodes, 376 mappable files; release build, Ryzen 9 7940HX), `Lsp::sync` against `ruff server`, 20 Python files open:

   | | before | after |
   |-|-|-|
   | selection moves | 99 ms | 0.01 ms |
   | one body edited | 99 ms | 6.8 ms |
   | first visit to a file | 13 ms | 6 ms |

   Before, every sync rendered every open document, about 5 ms each for a 6,000-line file (`line_map_of`: `file_contents` plus two sentinel writes and the row map), found each root by walking the outline (1 ms), and rendered the selection's document twice. `App::poll` syncs when the selection moves, so each arrow key cost 100 ms. Now a document is rendered only when it opens or the fingerprint of its tree and ancestors changes, nothing is checked while the outline generation is unchanged, and each root is remembered and looked up again only when an edit moved it. `cargo bench -p leolsp` times the same two cases on a synthetic outline: 79 ms to 1.5 us, and 79 ms to 4.4 ms.
2. **Colour the visible lines first on a body's first visit** (small). Measured: 46 ms the first time the 5,000-line body is shown, as tree-sitter colours it whole. The typing path's partial colouring already does visible-first; the rest follows after the first frame, or on a thread.
3. **Keep a few colourings, by node** (small). Measured: switching between two large bodies pays the 46 ms each time, as the cache holds one. A small cache keyed by node and outline generation makes switching back free.
4. **Split the body once a frame and drop the per-frame hash** (small). Measured: 2 ms a frame on the 5,000-line body with nothing changing, against 0.25 ms for a small one. `body_buffer()` splits the body twice a frame (`body_view` and the gutter width), and `Colouring::of` hashes every line to find its cache key; key both on the node and the outline generation.
5. **Line offsets for diagnostics** (done). `Doc::line(n)` scanned from the start of the text for each diagnostic. With offsets computed once per document, a body's diagnostics, 500 in a 6,300-line file, map in 8.7 us instead of 8 ms (`cargo bench -p leolsp`). They are mapped on every poll that moves the selection.
6. **A gnx index in leolib** (done). `Outline::position_of_gnx` builds a position from the existing gnx map by walking up parent links. Tabs, go to definition, applying a server's edits, session restore and leolsp's node documents use it instead of walking the outline: 16 tabs on leo-editor's 11,600 nodes are found in 1.5 us instead of 1.2 ms.
7. **Startup and size** (measured 2026-10-05; release build, Linux, Ryzen 9 7940HX, no outline, to the first drawn frame):

   | build | first frame | peak memory | binary |
   |-|-|-|-|
   | wgpu, eframe's default | 0.28 s | 198 MB | 35.5 MB |
   | glow only | 0.09 s | 117 MB | 29.5 MB |
   | glow only, stripped | | | 26.7 MB |

   leoapp's own start is 0.02 s and 13 MB (`leotui --dump`), so the rest is the window and the renderer. Glow is three times faster to the first frame and uses 80 MB less here; the case for wgpu is macOS, where OpenGL is deprecated, though it still runs there. `strip = true` saves 10%, and costs symbol names in a release panic's backtrace; not applied.

Not worth doing: tree rows (0.003 ms for a screen), idle CPU (none), egui's text layout (cached; a small outline's frame is 0.12 ms). Incremental document sync waits on a profile showing full-text sync matters.

## Larger, each needing a design

- **Scripting and `@button`** (large). Leo's distinguishing feature: Python run against the outline, and `@button` nodes as toolbar buttons. `leocub-vs-leotui.md` records Rhai as the pathway if scripting is ever wanted.
- **Two views of one outline** (large). Two bodies side by side, or two trees, over one `Document`.
