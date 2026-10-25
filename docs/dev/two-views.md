# Two views of one outline

Design note for the `TODO.md` item "Two views of one outline (needs a design)" under leogui, and the last entry of `gui-roadmap.md`. No code is changed by this note.

Terms:

- **Document state**: facts about the outline that every view must agree on. The tree, bodies, undo history, dirty flag.
- **View state**: facts about one pane pair (tree and body) looking at the outline. Selected node, cursor, scroll, hoist.
- **Active view**: the view that receives keys. One keyboard means exactly one at a time.
- **Stale position**: a `Position` whose path no longer resolves. `Position` is a child-index path (`crates/leolib/src/position.rs`), so any insert, delete or move above it can break it.

## 1. Use cases

| # | Use case | Scope |
|-|-|-|
| U1 | Two nodes side by side: a header and its implementation, a spec and its test | in scope |
| U2 | The same node in two panes: the top and bottom of a 5,000-line body | in scope; falls out of U1 |
| U3 | Outline plus rendered view | done: `crates/leogui/src/rendered.rs`, Leo's `viewrendered` |
| U4 | Two trees: two hoists or two `@chapter`s side by side | in scope, later step; needs per-view folds |
| U5 | A second OS window on one outline | out of scope for now; egui viewports add a second event loop |
| U6 | Two processes on one `.leo` file | out of scope; a file-level conflict, not a view |
| U7 | leotui split, vim's `Ctrl-w s` | in scope after leogui; same `App` API |

## 2. How Leo does it

- **Chapters.** `ChapterController.selectChapterByName` (`leo/core/leoChapters.py:162`) selects an `@chapter` node. The helper pushes it onto `c.hoistStack` (`leoChapters.py:453`). A chapter is a hoist with a name. leo-rs has hoist (`crates/leoapp/src/app/hoist.rs`) but not chapters (`delta.md`, "Chapters").
- **`add-editor`.** Several body editors in one frame, each showing a node. Upstream removed it in `765e7bf33b` ("Remove more cruft", 2024-08-25) and moved it to the attic in `1ef537bcae`. `leoserver.py:4042` still lists the name.
- **free_layout.** Its "Open Window" put a pane in its own window. Moved to the attic in `826e1db67e` (2026-09-01). `qt_layout.py` replaces it with fixed layouts.
- **`tab-detach`** (`leo/plugins/qt_frame.py:4416`) detaches a whole outline's tab. It is not a second view of the outline.
- **`clone-frame`.** No command, function or doc by that name exists in the `~/projects/leo-editor` working tree (`decouple-model-gui`, grep).
- **`open-second-view`.** Exists only on the `decouple-model-gui` branch of `~/projects/leo-editor`, not on its `main`. `leo/commands/commanderFileCommands.py:299` makes a second `Commands` over a shared `Outline`. `LEO_REFACTOR.md` records its decisions:
  - one undo history per outline ("Undo across views", line 1054);
  - expansion, `insertSpot` and scroll moved per view, keyed by gnx ("Stage 5", line 1241);
  - other views revalidate their position after every undo, falling back to the nearest surviving ancestor.
  - Tests: `leo/unittests/core/test_leoOutline.py` (`test_undo_revalidates_other_views`, `test_interleaved_undo_groups_are_detected`, `test_expansion_is_per_view`).

`delta.md:155` lists `open-second-view` as not ported.

## 3. Current state

`App` (`crates/leoapp/src/app/mod.rs:145`) holds one view and one document. MCP already acts as a second editor: `refind_current` (`app/mcp.rs:180`) re-finds the selection after a client edit, and `may_edit` (`app/mcp.rs:193`) refuses edits while the user is in INSERT or holds a buffer.

| Field | Owner | Note |
|-|-|-|
| `doc` | document | outline, `Undoer`, clipboard |
| `current` | view | |
| `focus` | view | tree or body within one view |
| `mode`, `mini`, `pending`, `parser`, `overlay` | active view only | inactive views are always NORMAL; see invariant below |
| `editor.cursor`, `desired_col`, `visual` | view | |
| `editor.register`, `last_change`, `last_find` | shared | vim keeps registers and `.` global across windows |
| `buffer` | active view only | the uncommitted INSERT copy |
| `hoists` | view | Leo fork: `hoistStack` per commander |
| `history` (`NodeHistory`) | view | |
| `selection` (multi-select) | view | |
| `top`, `body_scroll`, `body_hscroll`, `tree_height`, `body_height` | view | |
| `body_spots` | view | Leo fork moved `insertSpot` per view; keyed by `VnodeId` today |
| `expansion_level`, `expansion_node` | view | |
| `row_cache` | view | its key includes the hoist |
| `colouring`, `semantic_cache` | unclear | one cache per drawn body, or one shared cache of N entries |
| `Outline.expanded` (folds) | unclear | in the document today; `outline.rs:97` notes Leo keeps it per view |
| `search_origin`, `completion`, `signature`, `code_actions` | active view only | cleared on switch |
| `last_search`, `hlsearch`, `command_history`, `search_history` | shared | vim keeps them global |
| `last_hit` | unclear | |
| `diagnostics`, `lsp_synced` | view | both depend on `current` |
| `lsp`, `mcp`, `plugin_data`, `settings`, `theme`, `unread`, `changed_on_disk`, `rules_seen`, `pending_overwrite`, `pending_read` | shared | one per process |
| `message`, `messages` | shared | one status line in both front ends |
| `tree_percent` | front end | layout, not `App` state |

Scale: `.current` appears 225 times in `crates/leoapp/src` and 25 times in the front ends and plugins. `AppPlugin` methods take `&mut App` (`plugins/mod.rs:25`).

### Hard cases

1. **INSERT text in view A while view B edits the same node.** `buffer` is a full copy of the body. `commit_body` (`app/body.rs:80`) writes the whole body back. A commit after B's edit silently reverts B's edit. An undo in B has the same effect.
2. **Undo across views.** `Undoer` lives in `Document`. Beads name `VnodeId`s, not positions, and record no cursor (`leolib/src/undo.rs`). Undo is already model-only, so one shared history works. `undo` returns one `Position` to select, for the view that asked.
3. **Stale position.** A deletes or moves B's node. `Outline::position_exists` (`outline.rs:742`) checks each `(vnode, index)` pair, so B detects it in O(depth). Re-finding by vnode then has two problems:
   - a clone has several positions; `refind_current` takes the first in outline order, O(n);
   - `free_unreachable` (`outline.rs:318`) recycles arena slots. A `VnodeId` held across a freeing pass can name another node. The window is narrow: a deleted node stays referenced by its undo bead until the bead drops.
4. **Hoist.** `hoist_refuses` stops a view moving its own hoisted node. It cannot stop another view. `dehoist_to_show` then pops the stale hoist, and the view loses its hoist without a message.
5. **LSP.** `Lsp::sync(o, p)` opens the file containing `p`. With two views, the second view's file is opened only when it becomes active. Diagnostics are fetched for `current` only.

### The invariant that removes most of the hard cases

One keyboard means one active view. Make switching views do what vim does on leaving a window:

- commit INSERT, as `commit_typing` (`app/files.rs:333`) does before a close;
- drop VISUAL, completion, signature and the search origin;
- return to NORMAL.

Then only the active view can hold uncommitted text. Hard case 1 reduces to non-keyboard writers: MCP (already gated by `may_edit`), plugin `poll`, and language-server edits. Each must call one revalidation routine.

## 4. Options

### (a) Split `App` into shared state and `View`; one active view, others stored

`App { view: View, others: Vec<View>, ... }`. Per-view fields move into `View`. Switching swaps `view` with an entry of `others` (`std::mem::swap`). Drawing an inactive view swaps it in, calls `body_view` or `tree_view`, and swaps it back. This is the pattern `Parked` (`crates/leogui/src/outlines.rs:14`) uses for whole outlines.

- **Changes**: field paths (`self.current` becomes `self.view.current`), mostly mechanical. No borrow types change. Plugins keep `&mut App`.
- **Stale positions**: gnx-based re-find. On swap-in, compare the stored `Outline::generation` with the current one; re-find if it moved. `Tabs` (`leogui/src/tabs.rs`) already re-finds by gnx per generation.
- **Undo**: one history; the active view selects the result; others re-find.
- **Effort**: 8-10 person-days to U1, U2 and U7. U4 adds 2-3.
- **Risk**: low to medium. A missed field stays shared and leaks between views; tests per field catch it.
- **leotui**: gains `Ctrl-w s` and `Ctrl-w w` with no change to its event loop.

### (b) Two `App`s over one `Document` behind `Arc<Mutex<_>>` (or `Rc<RefCell<_>>`)

- **Changes**: `outline()` returns `&Outline` today. Through a lock it returns a guard, and every `p.h(o)` borrow is tied to it. 85 `self.doc` and 107 `app.doc` uses change. A guard held across a mutation panics (`RefCell`) or deadlocks (`Mutex`). `Arc<Mutex<_>>` buys nothing: both front ends run one UI thread.
- **Duplication**: each `App` starts its own language servers and MCP server. Two MCP servers contend for the configured port (`leomcp::Server::start`).
- **Stale positions**: each `App` polls the generation and re-finds, as (a). Edit notifications are possible but not needed.
- **Undo**: shared through `Document`. Hard case 1 stays open: each `App` has its own `buffer`, so the invariant must be enforced across instances.
- **Effort**: 12-15 person-days. **Risk**: high (runtime borrow failures, duplicated servers).
- **leotui**: none until it opens a second `App`.

### (c) leogui only: a secondary selection

`Gui` holds `Secondary { gnx, scroll }`. A second body pane draws that node's text from the outline. Clicking it makes it the selection, storing the old selection as the new secondary.

- **Changes**: `crates/leogui` only; about 300 lines (estimate).
- **Stale positions**: re-find by gnx, as `Tabs`.
- **Undo**: unchanged; only `App`'s view edits.
- **Effort**: 2-3 person-days. **Risk**: low.
- **leotui**: none; leotui gets no split.
- **Limits**: the secondary has no cursor, no VISUAL, no search highlight unless leogui reimplements them. It covers U1 and U2 for reading only.

### (d) The second view is read-only

A restriction, not an architecture. Applied to (a) or (c), it removes hard cases 1 and 2 for that view. Only re-finding remains. Under the invariant in section 3, (a) already gives the same safety while keeping both views editable. On its own, (d) mainly lowers the effort of (c).

### Stale positions: re-find versus edit notifications

| | gnx re-find | edit notifications |
|-|-|-|
| Mechanism | on a generation change, test `position_exists`, else search | `Document` reports each move or delete; views transform their positions |
| Clone accuracy | approximate: prefer the clone whose ancestor vnodes match the old stack | exact |
| Code touched | one routine in leoapp | about 30 `Document` methods plus undo and redo replay |
| Precedent here | `refind_current`, `Tabs`, `leolsp` root lookup | none |

Re-find wins on cost. Its order: keep the path if it still resolves; else the same vnode, nearest matching clone; else the nearest surviving ancestor in the old stack (as the Leo fork does); else the root. Check the gnx as well as the `VnodeId` to rule out a recycled slot.

## 5. Recommendation

Option (a), with the active-view invariant from section 3.

- Only one view holds uncommitted state, so whole-body commit stays correct.
- Undo needs no change: beads are already model-only.
- It keeps plugins, MCP and the front ends on `&mut App`.
- It gives leotui a split for about 2 extra days.

**Rejected framing: two views as two concurrent editors** (option (b), and the Leo fork's two `Commands`). It treats each view as a peer that may hold uncommitted text. That needs locking or merging between peers. Leo needs it because Qt widgets own their text (`LEO_REFACTOR.md`, finding 4). leo-rs does not: the model already owns the text, and one keyboard gives one writer.

**Other framing considered: a view is a saved cursor.** Option (c) taken to its end: a view is a gnx, a cursor and a scroll, restored on focus. This is (a) without per-view hoist, history or folds. It fits U1 and U2 but not U4. Step 1 below keeps it open: if U4 is dropped, `View` can shrink to those three fields.

## 6. Implementation plan

Each step lands separately, with `make test` passing.

1. **Extract `View`.** Move the per-view fields from section 3 into `pub struct View` in `crates/leoapp/src/app/view_state.rs`. `App` holds `view: View`. No behaviour change. Test: existing suite. (1-2 days)
2. **One re-find routine.** `View::refind(&Outline)` re-finds `current`, `hoists`, `history`, `selection` and `expansion_node` in the order of section 4. `refind_current` and `clamp_current` call it. Tests: a node deleted, moved, re-parented, a clone deleted, an undone insert, a recycled `VnodeId`. (1-2 days)
3. **Several views.** `others: Vec<View>`, `switch_view(i)` enforcing the invariant, and commands `open-second-view` (Leo's name), `close-view`, `cycle-view`. Tests: switch mid-INSERT commits once as one undo step; undo in B of A's insert moves A to the parent; B's hoisted node moved by A keeps the hoist. (1-2 days)
4. **Draw an inactive view.** `App::with_view(i, f)` swaps, runs `f`, swaps back. An inactive view on the active view's node shows the live `buffer`. Test: both `body_view`s of one node agree mid-INSERT. (1 day)
5. **MCP and LSP.** `selection` reports the active view; edits re-find all views; `Lsp::sync` runs for each view's `current`; diagnostics per view. Test: an MCP delete under an inactive view. (1 day)
6. **leogui panes.** A second body in the central panel, focus by click, the rendered pane following the active view, views saved in the session by gnx. Test: `Gui` session round trip, as the existing tabs test in `gui.rs`. (2-3 days)
7. **leotui split.** `Ctrl-w s`, `Ctrl-w v`, `Ctrl-w w`, `Ctrl-w c`, beside the existing `Ctrl-w <` and `>`. Test: `render` snapshot of two bodies. (2 days)
8. **Per-view folds, for U4.** Move `Outline.expanded` into `View`; a new view copies the active one's set. Then a second tree pane in leogui. Test: folding in one view leaves the other unchanged, as `test_expansion_is_per_view`. (2-3 days)

Estimates are inference from the field counts above, not measured.

## 7. Open questions

1. Folds: per view (Leo fork) or per document (leo-rs today)? Step 8 depends on it.
2. Should B's cursor move when A inserts lines above it? vim adjusts other windows (`mark_adjust`); step 3 only clamps.
3. Undo in B of A's change: select the changed node in B, or leave B where it is and name the step on the status line?
4. Is U5 (a second OS window) wanted, or are panes enough?
5. Is a leotui split (step 7) worth doing now, given TUI usability is the stated priority?
6. Should views persist in the session file, or start as one view each launch?
7. Is U4 wanted at all? If not, the saved-cursor framing in section 5 is enough and step 8 goes.
