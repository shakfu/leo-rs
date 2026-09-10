# leotui design

Status: **implemented**. Stages 1 to 3 of section 13 are built and Q1 to Q9 are settled. Sections 14 to 18 record where the implementation differs from what this document proposed, and why.

## 0. Why

The current TUI is a placeholder. Measured against a 17-row frame of `LeoPyRef.leo`:

- The one-line cheat sheet is cut off mid-word at 120 columns, and there is no other way to see the key list.

- No search. The only way to reach node 9,000 of 11,583 is `j`.

- No focus indicator, and body scrolling is on `n`/`p`, which nothing tells you.

- The body pane holds 55% of the width whether or not the node has a body.

- One non-modal key table, so every future command needs a free letter.

This document settles the input model before any of that is rebuilt.

---

## 1. Goals

1. Leo's outline bindings work where a terminal can deliver them.

2. A large command vocabulary without a large key space: `:` reaches every command, as Leo's minibuffer does.

3. Navigating an 11,000-node outline is practical.

4. The binding table is data, so `:help`, `--keys` and the tests read the same source as the dispatcher.

5. The body pane is a vim buffer: operators, motions, text objects, VISUAL and `.`, over the model's own text.

Non-goal: being vim *in the tree*. A tree is not a buffer of lines, and Leo has 30 years of outline vocabulary that a terminal can mostly deliver. Where vim and Leo disagree, focus decides: Leo owns the tree, vim owns the body. Section 9 tabulates every collision.

---

## 2. What a terminal can actually deliver

Measured with crossterm 0.28 under a pty (`cargo run -p leotui --example keyprobe`), feeding the escape sequences a terminal emits:

| sent | decoded as | usable |
|---|---|---|
| `ESC [1;2A` Shift-Up | `Up` + SHIFT | yes |
| `ESC [1;3A` Alt-Up | `Up` + ALT | yes |
| `ESC [1;5A` Ctrl-Up | `Up` + CONTROL | yes |
| `ESC [1;4A` Alt-Shift-Up | `Up` + SHIFT\|ALT | yes |
| `ESC i` | `Char('i')` + ALT | yes |
| `0x09` Tab / Ctrl-I | `Tab`, no modifier | **collide** |
| `0x0d` Enter / Ctrl-M | `Enter`, no modifier | **collide** |
| `0x08` Ctrl-H | `Char('h')` + CONTROL | yes, distinct from Backspace |
| `0x7f` | `Backspace` | yes |
| `0x1d` Ctrl-] | `Char('5')` + CONTROL | **wrong key** |
| `0x1a` Ctrl-Z and Shift-Ctrl-Z | both `Char('z')` + CONTROL | **collide** |
| `0x00` Ctrl-@ | `Char(' ')` + CONTROL | **wrong key** |
| `0x03` Ctrl-C, `0x13` Ctrl-S | `Char` + CONTROL | yes (raw mode) |
| `ESC [2~` Insert, `ESC [3~` Delete | `Insert`, `Delete` | yes |
| `ESC O P` F1, `ESC [1;2P` Shift-F1 | `F(1)`, `F(1)`+SHIFT | yes |

**The whole modified-arrow space survives.** That is what makes Leo's outline bindings portable: `!tree` arrows and Shift-arrows are Leo's primary navigation and node-movement keys, and they transfer verbatim.

Three of Leo's bindings cannot be delivered by a legacy terminal, because ASCII gave those control codes to other keys 60 years ago:

| Leo | binding | terminal reality |
|---|---|---|
| `insert-node` | Ctrl-I | is Tab |
| `mark` | Ctrl-M | is Enter |
| `promote` / `demote` | Ctrl-[ / Ctrl-] | Ctrl-[ is Escape; Ctrl-] decodes as Ctrl-5 |
| `redo` | Shift-Ctrl-Z | shift does not change a control code |
| `clone-node` | Ctrl-\` | not a control code at all |

### 2.1 The escape hatch

Also measured: crossterm decodes the Kitty keyboard protocol's `CSI u` form.

| sent | decoded as |
|---|---|
| `ESC [105;5u` | `Char('i')` + CONTROL |
| `ESC [9;1u` | `Tab`, no modifier |
| `ESC [109;5u` | `Char('m')` + CONTROL |
| `ESC [91;5u` | `Char('[')` + CONTROL |

So on kitty, foot, wezterm, ghostty, alacritty >= 0.13 and iTerm2 >= 3.5, pushing `KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES` makes Ctrl-I, Ctrl-M and Ctrl-[ distinct from Tab, Enter and Escape, and **Leo's literal bindings become reachable**.

Proposal: push the flag when the terminal accepts it, and bind Leo's literal key *in addition to* the portable one. The portable binding is what the help screen shows; the literal one is a bonus that costs one table entry.

**Q1: settled -- push them, and do not ask.** See section 18: asking costs a two-second stall on every terminal that does not support them.

---

## 3. Leo's own bindings

From `leo/config/leoSettings.leo`, the outline table verbatim:

```text
clone-node          = Ctrl-`          insert-child        = Ctrl-Insert
contract-node       = Alt-[           insert-node         = Ctrl-I
contract-all        = Alt--           insert-node   !tree = Insert
copy-node           = Shift-Ctrl-c    mark                = Ctrl-M
cut-node            = Shift-Ctrl-x    move-outline-down   = Ctrl-D
delete-node   !tree = Delete          move-outline-left   = Ctrl-L
delete-node   !tree = BackSpace       move-outline-right  = Ctrl-R
demote              = Ctrl-] Ctrl-}   move-outline-up     = Ctrl-U
edit-headline       = Ctrl-h          paste-node          = Shift-Ctrl-V
expand-node         = Alt-]           promote             = Ctrl-[ Ctrl-{
goto-next-clone     = Alt-N
```

and the arrow table, which is the important half:

```text
contract-or-go-left  !tree = LtArrow        move-outline-left  !tree = Shift-LtArrow
expand-and-go-right  !tree = RtArrow        move-outline-right !tree = Shift-RtArrow
goto-next-visible    !tree = DnArrow        move-outline-down  !tree = Shift-DnArrow
goto-prev-visible    !tree = UpArrow        move-outline-up    !tree = Shift-UpArrow
focus-to-body        !tree = Tab            goto-first-visible-node = Alt-Home
indent-region        !body = Tab            goto-last-visible-node  = Alt-End
```

Leo also binds `full-command = :` in its "Vim plain keys" mode. **The proposal in this document is what Leo already does, taken seriously.**

`!tree` and `!body` are Leo's own way of saying a binding applies only when that pane has focus. Section 5 keeps that rule exactly.

---

## 4. Modal or not

**Proposal: modal.** Three reasons, in order of weight:

1. A terminal has perhaps 60 unmodified keys and Leo has 286 commands. Without modes every command needs a modifier, which is how Leo arrived at Ctrl-` and Shift-Ctrl-V -- bindings a terminal cannot deliver.

2. The outline and the body want different vocabularies for the same keys. Non-modal forces one of them onto modifiers.

3. `:` over Leo's own command names gives a complete, discoverable, testable command surface for free.

The cost is a mode indicator and the discipline of Escape. Both are cheap.

---

## 5. Modes and focus

Two orthogonal pieces of state. Getting them confused is the usual way a modal TUI becomes unpredictable, so state the rule once:

- **Focus** is which pane a key acts on: `tree` or `body`. Tab toggles it, as in Leo. Focus is shown by the pane border.

- **Mode** is how keys are interpreted. Mode is shown at the left of the status line, vim-style.

```text
                Tab
        tree <--------> body        (focus: which pane)

 tree focus                     body focus
 ----------                     ----------
 NORMAL --- e ---> HEADLINE     NORMAL --- i a o I A O s S c C --> INSERT
 NORMAL --- : ---> COMMAND      NORMAL --- v ------------------> VISUAL
 NORMAL --- / ---> SEARCH       NORMAL --- V ------------------> VISUAL LINE
                                NORMAL --- : / ? --------------> COMMAND, SEARCH
   ^                                ^                               |
   +---------- Escape --------------+------------ Escape -----------+
```

| mode | focus | what it is |
|---|---|---|
| `NORMAL` | either | keys are commands, dispatched by focus, as Leo's `!tree`/`!body` |
| `HELP` | either | the help overlay has the keyboard. `q` or `Escape` closes it |
| `CONFIRM` | either | a yes/no question in the status line |
| `HEADLINE` | tree | a one-line edit of the current headline. Enter or Escape commits |
| `INSERT` | body | keys are text. Escape ends the change |
| `VISUAL` | body | a charwise selection; an operator applies to it |
| `VISUAL LINE` | body | the same, linewise |
| `COMMAND` | either | the `:` minibuffer, completing over the command table |
| `SEARCH` | either | `/` and `?`. Headlines with tree focus, body text with body focus |

VISUAL exists only with body focus. The tree uses marks instead -- section 8.

**Resolved (was Q2, Q3).** Vim editing settles both: the undo unit is a *change*, not a keystroke and not a pane switch. Escape ends an INSERT session, which is one change and one undo bead. See section 8.2.

**Q8 (new).** Does `Escape` in body NORMAL return focus to the tree, or do nothing as vim does? Recommendation: return to the tree. The body is a pane, not the whole editor, and Escape is the one key a user will press when lost. Vim's Escape-in-normal is a no-op only because there is nowhere to go back to.

---

## 6. The `:` command line

The vocabulary is Leo's command names. `leolib` already implements the operations; the names come from `@g.command` decorators in Leo's own source (286 of them; the outline subset is what matters here).

Behaviour:

- Tab completes: longest common prefix, then cycles matches.

- Up/Down walk the command history.

- An unknown command is an error in the status line, not a beep.

- `:` with a count prefix (`:3`) is reserved; see Q6.

Vim habits that must not be errors:

| typed | means |
|---|---|
| `:w` `:write` | `save` |
| `:w <path>` | `save-to <path>` |
| `:q` | quit; refuses if the outline is changed |
| `:q!` | quit, discarding changes |
| `:wq` `:x` | `save` then quit |
| `:e <path>` | open another `.leo` file |
| `:h` `:help` [cmd] | the help overlay, or one command's summary |
| `:set <option>...` | one or more, as vim reads them: `name`, `noname`, `name=value`, `name:value`, `name?`; see section 10 |
| `:[range]s/pat/rep/[flags]` | substitute in the current node's body; a `regex` crate pattern, not vim's dialect |

### 6.1 The v1 command set

Every name is Leo's. "have" means `leolib` implements the operation today.

| command | status |
|---|---|
| `insert-node` `insert-child` `insert-node-before` | have |
| `delete-node` `cut-node` `copy-node` `paste-node` | have |
| `clone-node` | have |
| `move-outline-up` `-down` `-left` `-right` | have |
| `promote` | have |
| `demote` | add (mirror of promote) |
| `mark` `unmark-all` | have |
| `undo` `redo` | have |
| `save` `save-to` | have |
| `read-at-file-nodes` `write-at-file-nodes` `write-dirty-at-file-nodes` | have |
| `save` writes the `.leo` file; `write-outline-only` is its Leo name | have |
| `expand-node` `contract-node` `expand-all` `contract-all` | add (view state) |
| `expand-to-level-1`..`-9` `expand-next-level` `contract-all-other-nodes` | add |
| `goto-next-visible` `goto-prev-visible` `goto-parent` | add (trivial) |
| `goto-first-node` `goto-last-node` `goto-next-sibling` `goto-prev-sibling` | add |
| `goto-next-marked` `goto-prev-marked` `goto-next-clone` | add |
| `sort-children` `sort-siblings` | add |
| `hoist` `dehoist` | add (view state) |
| `clone-marked-nodes` `delete-marked-nodes` `copy-marked-nodes` `move-marked-nodes` | add |
| `edit-headline` | have |

**Q4.** Is that the right v1 line? The additions are all small; `hoist` and the expand family need per-view state that `leolib::Outline` already carries.

---

## 7. Key tables

Data, not code: one table, read by the dispatcher, by `:help` and by `--keys`.

### 7.1 NORMAL, focus = tree

Navigation:

| key | command | note |
|---|---|---|
| `j` `Down` | `goto-next-visible` | Leo binds DnArrow |
| `k` `Up` | `goto-prev-visible` | Leo binds UpArrow |
| `h` `Left` | `contract-or-go-left` | Leo binds LtArrow; vim's h already means "out" |
| `l` `Right` `Enter` | `expand-and-go-right` | Leo binds RtArrow |
| `gg` `Alt-Home` | `goto-first-visible-node` | Leo binds Alt-Home |
| `G` `Alt-End` | `goto-last-visible-node` | Leo binds Alt-End |
| `gp` | `goto-parent` | |
| `{` `}` | `goto-prev-sibling` `goto-next-sibling` | |
| `Ctrl-f` `Ctrl-b` `Ctrl-d` `Ctrl-u` | page and half-page | same in both panes -- see section 9 |
| `[m` `]m` | `goto-prev-marked` `goto-next-marked` | |
| `]c` `Alt-n` | `goto-next-clone` | Leo binds Alt-N |

Structure:

| key | command | note |
|---|---|---|
| `o` `Insert` | `insert-node` | Leo binds Insert `!tree`; vim's o opens below |
| `O` | `insert-node-before` | |
| `a` `Ctrl-Insert` | `insert-child` | Leo binds Ctrl-Insert |
| `dd` | `cut-node` | vim's `dd` cuts into the register |
| `Delete` `Backspace` | `delete-node` | Leo binds both `!tree`, and neither copies |
| `yy` | `copy-node` | |
| `p` | `paste-node` | |
| `` ` `` | `clone-node` | a nod to Leo's unreachable Ctrl-\` |
| `m` | `mark` | Leo's Ctrl-M is Enter in a terminal |
| `M` | `unmark-all` | leotui's own: Leo has no binding |
| `J` `Shift-Down` | `move-outline-down` | Leo binds Shift-DnArrow `!tree` |
| `K` `Shift-Up` | `move-outline-up` | Leo binds Shift-UpArrow `!tree` |
| `<<` `Shift-Left` | `move-outline-left` | Leo's GUI calls this "deindent node" |
| `>>` `Shift-Right` | `move-outline-right` | Leo's GUI calls this "indent node" |
| `g<` `g>` | `promote` `demote` | Leo's Ctrl-[ / Ctrl-] are unreachable |
| `e` `Ctrl-h` | `edit-headline` | Leo binds Ctrl-h, which a terminal *can* deliver |

Folding, on vim's `z` family, which already means folding:

| key | command |
|---|---|
| `Space` `za` | toggle expand/contract |
| `zo` `zc` `Alt-]` `Alt-[` | `expand-node` `contract-node` (Leo's Alt-] / Alt-[) |
| `zR` `zM` `Alt--` | `expand-all` `contract-all` (Leo binds Alt-- to contract-all) |
| `zr` `zm` | `expand-next-level` `expand-prev-level` |
| `zx` | `contract-all-other-nodes` |
| `z1`..`z9` | `expand-to-level-N` |

Files and history:

| key | command |
|---|---|
| `u` `Ctrl-z` | `undo` (Leo binds Ctrl-Z) |
| `Ctrl-r` | `redo` (Leo's Shift-Ctrl-Z is not a distinct control code) |
| `/` `?` `n` `N` | search headlines (stage 2) |
| `Ctrl-s` `:w` | `save` (Leo binds Ctrl-S) |
| `w` | `write-at-file-nodes` |
| `Tab` | `focus-to-body`, as Leo's `focus-to-body !tree = Tab` |
| `Ctrl-Left` `Ctrl-Right` | resize the split. leotui's own: Leo has no panes |
| `F1` | the help overlay, also `:help` |
| `q` | quit |

### 7.2 NORMAL, focus = body

The body pane is a vim buffer. The grammar is vim's: an optional count, an optional operator, and a motion or text object. Section 8 specifies the editing core; this is the key table.

Motions:

| key | means |
|---|---|
| `h` `l` `Left` `Right` | character left/right |
| `j` `k` `Down` `Up` | line down/up, keeping the desired column |
| `w` `W` `b` `B` `e` `E` `ge` | word motions; capitals use whitespace-delimited WORDs |
| `0` `^` `$` | start of line, first non-blank, end of line |
| `gg` `G` | first line, last line; with a count, that line |
| `{` `}` | previous/next blank line |
| `f{c}` `F{c}` `t{c}` `T{c}` `;` `,` | find character on the line, and repeat |
| `%` | matching bracket |
| `H` `M` `L` | top, middle, bottom of the visible body |
| `Ctrl-f` `Ctrl-b` `Ctrl-d` `Ctrl-u` | page and half-page |

Operators, which take a motion, a text object, or a doubling:

| key | means |
|---|---|
| `d` | delete into the text register |
| `c` | change: delete, then INSERT |
| `y` | yank into the text register |
| `>` `<` | indent / unindent, linewise. Leo calls these `indent-region` |
| `gu` `gU` `g~` | lowercase, uppercase, toggle case |

Simple edits, each one change:

| key | means |
|---|---|
| `x` `X` | delete the character under / before the cursor |
| `r{c}` | replace one character |
| `s` `S` | substitute character / line, then INSERT |
| `D` `C` `Y` | to end of line: delete, change, yank |
| `A` `I` `o` `O` | append, insert at first non-blank, open a line below/above |
| `J` | join this line with the next |
| `~` | toggle the case of the character under the cursor |
| `p` `P` | put the text register after / before the cursor |
| `.` | repeat the last change |
| `u` `Ctrl-r` | undo, redo -- the outline's history, not a separate one |

Mode and focus:

| key | means |
|---|---|
| `i` `a` `I` `A` `o` `O` `s` `S` `c` `C` | enter INSERT |
| `v` `V` | enter VISUAL, VISUAL LINE |
| `/` `?` `n` `N` | search within the body |
| `:` | the command line |
| `Tab` `Escape` | focus the tree (see Q8) |

In INSERT, `Tab` inserts indentation: Leo binds `indent-region !body = Tab`, and `>>` covers that meaning in NORMAL.

### 7.2.1 Text objects

Usable after an operator (`ciw`) or in VISUAL (`vi"`):

| object | means |
|---|---|
| `iw` `aw` | word, without / with trailing whitespace |
| `iW` `aW` | WORD |
| `i"` `a"` `i'` `a'` `` i` `` `` a` `` | quoted string, without / with the quotes |
| `i(` `a(` `i[` `a[` `i{` `a{` `i<` `a<` | bracket pair, without / with the brackets |
| `ip` `ap` | paragraph |

`b` and `B` are accepted as aliases for `(` and `{`, as in vim.

### 7.3 Counts

`5j`, `3>>`, `2dd` -- a leading count repeats the command. Applies to motions and to node commands.

**Q6.** Reserve `:N` for "go to the Nth visible node"? vim's `:N` goes to line N. There is no line-numbered outline, so the analogue is the visible row. Recommendation: yes, it costs nothing and matches the status line's `row N/M`.

---

## 8. Selections: VISUAL in the body, marks in the tree

Two different problems, two different mechanisms.

### 8.1 The body: VISUAL

`v` starts a charwise selection, `V` a linewise one. The cursor moves with any motion; the anchor stays. `o` swaps them. An operator (`d c y > < gu gU g~`) applies to the selection and returns to NORMAL. `Escape` cancels.

Blockwise VISUAL (`Ctrl-v`) is **not** in v1: block insert and ragged right edges are a large amount of work for a rare need in an outliner, where a body is usually a screenful of prose or code.

### 8.2 The editing core

Three decisions, because they constrain everything else.

**The model is the buffer.** `leolib::Outline` is authoritative for body text; that was the point of separating the model from the view. The body pane holds the cursor, the selection and the pending operator -- never a second copy of the text that has to be reconciled. Every change goes through `Document::set_body`.

**One change, one undo bead.** A change is one operator application, one simple edit, or one INSERT session from entry to `Escape`. That is vim's own undo granularity, and it makes `u` mean the same thing in both panes: the tree's `u` and the body's `u` are the same history, in the same order. Typing 200 characters and pressing Escape is one `u`.

**A change is a value, not a closure.** `.` repeats the last change, so a change must be replayable: `{ count, operator, target, inserted_text }`, not a `FnMut`. This is the one requirement that shapes the code rather than the key table, so it is settled here rather than discovered later.

### 8.3 Registers

Two registers, because nodes and text are not the same thing:

| register | written by | read by |
|---|---|---|
| node clipboard | `yy` `dd` in the tree, `copy-node`, `cut-node` | `p` in the tree, `paste-node` |
| text register | `y` `d` `c` `x` in the body | `p` `P` in the body |

`p` therefore always does the obvious thing for the pane you are in. Named registers (`"ayy`) are v2; the register file is a map from the start so that adding them is a lookup, not a redesign.

### 8.4 The tree: marks

vim's visual mode selects a contiguous range. An outline's useful selections are rarely contiguous -- "every node I marked while reading" -- and Leo already has the mechanism, with the vocabulary to match:

```text
clone-marked-nodes    delete-marked-nodes    copy-marked-nodes
move-marked-nodes     mark-subheads          mark-changed-nodes
mark-node-and-parents unmark-all             goto-next-marked
```

`m` marks, `[m` and `]m` walk marks, and the `-marked-nodes` commands are reachable from `:`. `V` in the tree is left unbound, so a contiguous sibling selection can be added later without moving anything.

## 9. Where vim and Leo collide

Focus resolves most of them: the same key can be Leo's in the tree and vim's in the body, which is what Leo's own `!tree`/`!body` qualifiers already say.

| key | vim | Leo | resolution |
|---|---|---|---|
| `h` `l` | left/right char | contract/expand node | **focus**: Leo in the tree, vim in the body |
| `J` | join lines | -- | **focus**: move node down in the tree, join lines in the body |
| `c` | change operator | `clone-node` (Ctrl-\`) | **focus**: clone in the tree, change in the body |
| `m` | set a mark | `mark` a node | **focus**: mark the node in the tree, unbound in the body until v2 marks |
| `p` | put text | `paste-node` | **focus**, and two registers -- section 8.3 |
| `/` `?` `n` `N` | search text | -- | **focus**: headlines in the tree, body text in the body |
| `d` `y` | operators | -- | **focus**: `dd`/`yy` act on the node in the tree, on lines in the body |
| `>>` `<<` | indent line | Leo's GUI: indent/deindent node (Cmd-R/Cmd-L) | **focus**: `move-outline-right`/`-left` in the tree, indent in the body |
| `Ctrl-d` `Ctrl-u` | half page | `move-outline-down`/`-up` | **vim, in both panes** -- see below |
| `Ctrl-r` | redo | `move-outline-right` | **vim, in both panes** -- see below |
| `Tab` | indent (insert) | `focus-to-body !tree`, `indent-region !body` | focus toggle in NORMAL, indent in INSERT and VISUAL |
| `u` `:` | undo, command line | undo, `full-command` | both agree |

One rule decides the last two rows and is worth stating on its own: **a Ctrl- key means the same thing in both panes.** Undo, redo and paging are operations on the editor, not on the tree or the buffer, and a user who has to remember that `Ctrl-r` redoes here but moves a node there has been given a puzzle.

The cost is Leo's `Ctrl-D`/`Ctrl-L`/`Ctrl-R`/`Ctrl-U` set for moving nodes. That set was always Leo's keyboard alternative to Shift-arrows, and both of its replacements -- `J K << >>` and Leo's own Shift-arrows -- are kept. Leo's `redo = Shift-Ctrl-Z` is unreachable in a legacy terminal anyway: shift does not change the control code for Z.

## 10. Configuration

Settings live in `~/.config/leotui/config.toml`, read at startup. It is TOML, in the subset `theme.rs` already parses, and holds two settings: `theme` and `split-ratio`. An accepted `:theme` or `:set split=N` rewrites its line and leaves the rest of the file alone (section 19.7.3). A line the reader does not understand is reported on the status line and skipped.

Key bindings still ship as one built-in table. The override file proposed below predates `config.toml`; whether it keeps Leo's `@shortcuts` syntax or becomes a `[keys]` table there is open.

Proposed format, deliberately Leo's `@shortcuts` syntax so the file can be lifted from a Leo settings node:

```text
# ~/.config/leotui/keys.conf
[normal.tree]
insert-node   = o
insert-node   = Insert
clone-node    = `
[normal.body]
...
```

`:set` options for the session: `wrap`, `number`, `split` (the pane ratio), `search` (`headlines` or `all`), `kitty-keys`.

---

---

## 11. Implementation notes

- One `Binding { mode, focus, keys, command }` table. The dispatcher, the `:help` overlay and `--keys` all read it. A binding with no command, or a command with no binding and no `:` name, is a test failure.

- `--keys` prints the table; `--dump` already prints a frame. Both are how the UI gets tested without a terminal.

- Multi-key sequences (`gg`, `dd`, `zo`, `[m`, `ciw`, `f{c}`) need a pending buffer. A prefix is either completed by the next key or discarded; there is no timeout, so `g` followed by nothing waits.

- The body's grammar is a small state machine over that buffer:
  `count? (operator count? (motion | text-object | operator))` -- doubling an
  operator means linewise. Parse it once and hand the result to the editor; do not scatter the grammar across key handlers.

- A change is a value (section 8.2), so `.` replays it and the tests can assert on it without a terminal.

- Motions are pure: `fn(buffer, cursor, count) -> Target`, where `Target` is a position plus charwise/linewise. Every motion is then testable in isolation, and operators compose with all of them by construction rather than by a matrix of special cases.

---

## 12. Summary of open questions

Settled by the decision to have vim editing in the body:

| | question | settled as |
|---|---|---|
| Q2 | Escape from INSERT commits or abandons? | commits: one INSERT session is one change |
| Q3 | INSERT writes live or on Escape? | on Escape, one undo bead |
| Q5 | vim operators in the body? | **yes**, with text objects and registers |
| Q7 | VISUAL mode? | **yes, in the body only**; the tree uses marks |

Settled during implementation:

| | question | settled as |
|---|---|---|
| Q1 | Kitty keyboard flags by default? | pushed without asking -- section 18 |
| Q4 | Is the section 6.1 command set the right v1? | as listed |
| Q6 | `:N` goes to the Nth visible node? | yes |
| Q8 | `Escape` in body NORMAL: focus the tree, or nothing? | focus the tree |
| Q9 | Is the section 7.2 key set the right v1 for the body? | as listed |

Deliberately v2, listed so the v1 code leaves room:

| | why not v1 |
|---|---|
| blockwise VISUAL (`Ctrl-v`) | block insert and ragged edges; rare in an outliner |
| named registers (`"ayy`) | the register file is a map from the start, so it is a lookup |
| marks (`ma`, `` `a ``) | `m` is `mark` in the tree; needs a separate namespace in the body |
| macros (`q`, `@`) | needs a key-event recorder, which the change record does not give |
| `:s///` and `:g//` with ranges | the `:` parser reserves the syntax; no implementation |
| a rebindable keymap file | section 10; the built-in table is data, so this is parsing only |

---

## 13. Size and staging

The body editor is roughly as large as everything else in this document. An estimate, so the staging is a decision rather than a surprise:

| | lines |
|---|---|
| motions (§7.2) | 250 |
| text objects (§7.2.1) | 200 |
| operators and change application (§8.2) | 250 |
| VISUAL (§8.1) | 100 |
| INSERT | 150 |
| change record and `.` | 100 |
| the key grammar (§11) | 200 |
| **body editor** | **~1,250 plus ~400 of tests** |
| tree NORMAL and the binding table (§7.1) | 300 |
| `:` command line, completion, command table (§6) | 350 |
| search, both panes | 200 |
| help overlay, status line, focus chrome, layout (§0) | 350 |
| **the rest** | **~1,200 plus ~300 of tests** |

Against today's 1,110 lines of `leotui`, that is roughly a 3x.

Proposed order, each stage usable on its own:

1. **Chrome and the binding table.** Focus model, status line with the mode, help overlay, `--keys`, and the tree table from §7.1 driven by data. Fixes every defect in §0 except search.

2. **`:` and search.** The command line, completion over the §6.1 command set, and incremental search in both panes. After this the outline is navigable.

3. **The body editor.** Motions, then operators and text objects, then VISUAL, then `.`. Each is independently testable through `--dump` and the change record, so this stage can stop anywhere and still be coherent.

---

## 14. Stage 1, as built

Implemented: the focus model, the mode indicator, the breadcrumb, the help overlay, `--keys`, the resizable split, and the whole of section 7.1 driven by the binding table. 35 tests in `leotui`, 8 new in `leolib`.

`leolib` gained the commands section 6.1 listed as "add" for the tree: `demote`, `insert-child`, `insert-node-before`, `cut-node`, `unmark-all`, `expand-all`, `contract-all`, `expand-to-level`, `contract-all-other-nodes`, `expand-all-ancestors`, `last_position`, `last_visible_position`, `next_marked`, `prev_marked`, `next_clone`.

### 14.1 Where the build differs from sections 5-7

| | change | why |
|---|---|---|
| `dd` | `cut-node`, not `delete-node` | vim's `dd` cuts into the register, so `dd` then `p` moves a node. `Delete` and `Backspace` remain Leo's non-copying `delete-node` |
| `Tab` | `focus-to-body` in the tree, `focus-to-tree` in the body | Leo's own rule. A generic `toggle-focus` was written first and removed: two named commands beat one that needs the current state to explain it |
| `M`, `zx`, `Ctrl-Left/Right`, `w`, `Ctrl-s` | added | `unmark-all`, `contract-all-other-nodes`, the split, and the two file commands had no key. `:` will reach them in stage 2; until then a command with no binding is a command nobody can run, which a test now enforces |
| `q` | tree only | `w` and `q` are a vim motion and the macro key, which the body wants in stage 3. `Escape` reaches the tree first |
| `HELP`, `CONFIRM` | new modes | the overlay and the quit prompt take the keyboard, which is what a mode is |

### 14.2 What the tests hold

Four properties of the table, checked rather than reviewed:

- every binding parses, and names a command that exists;

- every command has a binding (see the table above for why);

- no two bindings claim the same keys in one mode and pane;

- **no binding is a prefix of another**, or the longer one could never be typed. This caught a real bug: the key parser lowercased single characters, so `G` parsed as `g` and shadowed `gg`, and `O` shadowed `o`. Case folding is correct only when Control is held, because a terminal reports Ctrl-H as Ctrl-h.

A fifth pins the point of the exercise: Leo's arrow bindings -- plain arrows to navigate, Shift-arrows to move a node, `Insert`, `Delete`, `Backspace`, `Ctrl-h`, `Alt-Home`, `Alt-End` -- are asserted present by name.

### 14.3 Testing without a terminal

`--press "l,l,F1"` applies a sequence of binding specs before drawing, so any state is reachable headlessly:

```text
leotui F.leo --dump --press "F1"          the help overlay
leotui F.leo --dump --press "Tab,i"       the body, in INSERT
leotui --keys                             the binding table
```

The pty harness in `examples/keyprobe.rs` measures what a terminal delivers; `--press` and `--dump` cover everything above the key layer.

### 14.4 Not in stage 1

`:` and search are stage 2; the vim body editor is stage 3. Body NORMAL is navigation only, and `i` opens the same line editor as before -- with one change, from Q2: **Escape commits** and `Ctrl-c` abandons, where the old TUI had `Ctrl-s` commit and Escape abandon.

---

## 15. Stage 2, as built

The `:` minibuffer and search, in `minibuffer.rs` and `search.rs`.

**`:` takes Leo's command names**, with Tab completion (the longest common prefix first, then each match), Up/Down history, and the vim spellings a user will type anyway: `:w`, `:w path`, `:q`, `:q!`, `:wq`, `:x`, `:e path`, `:h cmd`. `:N` selects the Nth visible row -- Q6, as recommended.

**`:set`** carries the options section 10 named: `search=all|headlines`,
`split=N`, `wrap`/`nowrap`, `number`/`nonumber`.

**Search is incremental and smartcase.** `/` and `?` move the selection as the pattern is typed and put it back on Escape; `n` and `N` go to the next and previous match. One walk covers the outline in order, each headline then its body, whichever pane has focus; a body match puts the body's cursor on it, and `:set search=headlines` leaves bodies out. The pattern is a `regex` crate regex. An all-lowercase pattern ignores case; one with a capital does not. Matches are highlighted until `:noh`.

### 15.1 The minibuffer is drawn plain

The status line is a readout and carries a background; the minibuffer replaces it while a `:` command, a `/` search or a headline is being typed, and is drawn with no background at all. Painting an input line in the status bar's colours makes it read as a readout rather than as something to type into. vim draws its command line the same way.

A test asserts it from the rendered buffer: every cell of the status row has a background when it is a status line, and none when it is a minibuffer.

### 15.2 What changed from section 6

| | change | why |
|---|---|---|
| `q!` alias | removed | `parse_command` strips `!` into a `force` flag, so `q` covers it. The entry was dead and misleading |
| every command needs a binding | relaxed | `:` reaches every command by name. The test now allows an explicit `COMMAND_LINE_ONLY` list, which holds one entry: `goto-visible-row` |
| `:e` with unsaved changes | refuses | opening another outline would discard them silently |

---

## 16. Stage 3, as built

The vim body editor, in `editor/`: `motion.rs` (motions and text objects), `change.rs` (the change model), `parse.rs` (the grammar), `mod.rs` (applying changes). 1,900 lines with 60 tests.

Section 8.2's three decisions held up:

- **The model is the buffer.** A working copy exists only while a change is being typed; everything else reads `p.b(&outline)` and writes through `Document::set_body`.

- **One change, one undo bead.** `A`, 200 characters, Escape is one `u`.

- **A change is a value.** `Change` is an enum, so `.` replays it and the tests assert on it directly.

### 16.1 The one irregular rule

`cw` behaves as `ce`: it does not swallow the space after the word. vim has this special case and users rely on it without knowing it is one. It lives in `Editor::adjust_for_change`, because the rule depends on whether the cursor is on whitespace, which the parser cannot see.

### 16.2 What changed from section 7.2

| | change | why |
|---|---|---|
| body keys are not in the binding table | the grammar handles them | `count? (operator count? (motion \| object \| operator))` is a parser, not a lookup. The table holds one documentation row per group so `F1` and `--keys` still list them |
| `i` in the outline | opens the body in INSERT | the old TUI did this, and it saves a Tab |
| `Ctrl-d`/`Ctrl-u` in the body | paging, per section 9 | the same in both panes |

### 16.3 Not built, as planned

Blockwise VISUAL, named registers, marks, macros, `:s///`. The register file is a struct rather than a map, since only one register exists; naming registers means making it a map, which is the change section 8.3 anticipated.

---

## 17. Where the TUI stands

Measured, not estimated:

| | lines |
|---|---|
| `keys.rs`, `bindings.rs`, `commands.rs` | 1,044 |
| `app.rs` | 1,622 |
| `editor/` | 2,130 |
| `minibuffer.rs`, `search.rs` | 577 |
| `ui.rs`, `main.rs` | 670 |
| **total** | **6,043**, with 107 tests |

Section 13 estimated ~3,200 lines. The gap is tests, which that estimate counted separately and undercounted: they are 40% of the editor.

Everything above the key layer is testable without a terminal: `--press` applies binding specs before drawing, `--dump` renders one frame, `--keys` prints the table. `examples/keyprobe.rs` measures what a terminal delivers.

Every question in section 12 is settled.

---

## 18. Q1, as built: the keyboard enhancement flags

**Pushed on startup, popped on exit, and never asked about.**

`crossterm::terminal::supports_keyboard_enhancement` is the documented way to ask, and reading its source rules it out: it writes the kitty query followed by a primary-device-attributes query, then polls for a *keyboard flags* reply with a 2,000 ms timeout. A terminal that does not support the protocol answers the second query and not the first, so the poll runs to its timeout and returns an error. **Asking costs two seconds at startup on every terminal that says no** -- which is the common case.

Pushing blind costs nothing, because the protocol is designed for it: a terminal that does not understand `CSI > 1 u` ignores it, and the bindings that depend on it never fire. `--no-kitty-keys` opts out.

### 18.1 What this makes reachable

Leo's literal bindings, measured through a pty (`examples/keyprobe.rs`):

| Leo | sends | decoded as | portable binding it joins |
|---|---|---|---|
| `insert-node = Ctrl-I` | `ESC [105;5u` | `Char('i')`+CONTROL | `o`, `Insert` |
| `mark = Ctrl-M` | `ESC [109;5u` | `Char('m')`+CONTROL | `m` |
| `promote = Ctrl-[` | `ESC [91;5u` | `Char('[')`+CONTROL | `g<` |
| `demote = Ctrl-]` | `ESC [93;5u` | `Char(']')`+CONTROL | `g>` |
| `clone-node = Ctrl-\`` | `ESC [96;5u` | `` Char('`') ``+CONTROL | `` ` `` |  |  |  |
| `redo = Shift-Ctrl-Z` | `ESC [122;6u` | `Char('z')`+CONTROL\|SHIFT | `Ctrl-r` |

Also measured: with the flags pushed, `Escape` arrives as `ESC [27u` and decodes as `Esc`, and unmodified keys are unchanged. Nothing that worked before stops working.

A test asserts both halves of each row: the literal binding is present, **and** so is a binding a legacy terminal can deliver. A key that only some terminals can send must never be the only way to reach a command.

### 18.2 A bug this found

Under the protocol, `Ctrl-Shift-Z` arrives as `Char('z')+CONTROL|SHIFT` from
some terminals and `Char('Z')+CONTROL|SHIFT` from others. The key normalizer stripped SHIFT from uppercase characters, so those were two different keys and only one of them could be bound.

The rule is now one sentence: **a control chord is named by its lowercase letter plus its modifier bits**; without Control, an uppercase character keeps its case and drops the redundant SHIFT. `Ctrl-H` and `Ctrl-h` are the same binding; `G` and `g` are not.

### 18.3 Restoring the terminal

Pushing the flags makes restoring them mandatory: a process that exits without popping leaves the *shell* receiving `CSI u` sequences it does not understand.

`TerminalGuard` owns raw mode, the alternate screen and the flags, and undoes them in reverse on `Drop`. A panic hook calls the same code, which fixes a bug that predates this section: a panic in the event loop used to leave the terminal in raw mode with the alternate screen up, so the panic message was invisible and the shell unusable. Verified by patching a panic into the loop and watching the pty: flags popped, alternate screen left, message visible, exit 101.

---

## 19. Highlighting the body

The language a node is written in is a model question, and `leolib` already answers it: `Outline::get_language` is Leo's four-pass rule over the node, its ancestors and the nearest `@<file>` extension. That is where the colouring *starts*.

A body may then change it. Leo's `match_at_language` returns 0 unless `i == 0`, so an `@language` line counts only at column 0, and it assigns `self.language` from that line onward. One node can hold Python and then C. `highlight.rs` follows the same rule, and `@nocolor`, `@color` and `@killcolor` bracket regions as they do in Leo.

**Which language a node starts in is not the first line's.** Leo's `scanLanguageDirectives` calls `c.getLanguage(p)`, whose first pass returns the *first* valid `@language` anywhere in the body. A body with one directive halfway down is therefore coloured in that language from its first line, in Leo and here. Position matters once a body holds two.

### 19.1 Scope: a declared language, or nothing

`@language` reaches the node that holds it and that node's descendants. So does an `@<file>` node's extension. A node with neither above it is **left plain**.

Leo does not stop there: `scanLanguageDirectives` ends in `language or c.target_language`, so an undeclared node is coloured as whatever the outline's default is -- Python, normally. That paints a prose node wrong. In `class`, `if`, `import`, `for` and `return` become keywords, `#` starts a comment, and an apostrophe in `It's a plan` opens a string that runs to the end of the body.

`Outline::language_at` returns `Option<String>` and reports the absence; `get_language` keeps Leo's behaviour by falling back, because the writers must have a language to choose comment delimiters with. The colorizer takes the Option and colours nothing when it is None.

### 19.2 Where the code lives

In `leotui`, for the reason Leo keeps `leoColorizer` out of its model: colouring is a view's business, and `leoColorizer` is one of the nine view modules `leolib` was defined to exclude.

It shares the model's *data* rather than restating it:

| | from |
|---|---|
| comment delimiters | `leolib::outline::set_delims_from_language`, over Leo's 190-language table |
| keywords | `keywords.rs`, generated from `leo/modes/*.py` |
| grammars | `treesit.rs`, one tree-sitter crate per language |

So every language Leo knows gets comments and strings; 33 get keywords.

String delimiters used to come from `leolib::importers::LANGUAGES`, whose `string_list` the importers already need. They now live in `highlight.rs`'s `lex_for`. The importers answer a different question -- where a block of code starts -- and `rust.string_list` is deliberately empty because the Rust importer scans for itself. Read as a colouring rule it said Rust has no strings, so `//` inside a literal opened a comment that ran to the end of the line. `lex_for` also carries the two rules a delimiter list cannot: whether a backslash escapes inside a string, and whether block comments nest.

### 19.3 The keyword cap

Leo's mode files hold 44,259 keywords across 156 languages. jEdit's `keyword1` is a language's own keywords; `keyword2` to `keyword4` are its library names, and some modes list thousands -- 5,126 for matlab, 4,261 for r, 2,309 for php. Colouring every library name turns a script into confetti and the generated file into half a megabyte.

The rule: `keyword1` always, the rest only when a language's whole set stays under 400. Python keeps all 265, C all 42, matlab keeps its 457 keywords and loses its 5,126 function names. The two classes are drawn differently, as Leo draws them.

### 19.4 Two engines

Twelve languages have a tree-sitter grammar compiled in -- c, cplusplus, css, go, html, java, javascript, json, python, rust, shell, typescript -- and are parsed. Every other language runs the line scanner.

A parse tree adds the classes a table lookup cannot produce -- function, type, property, attribute, and a builtin told from a plain one -- because those depend on where an identifier sits, not on what it spells. `self.count += 1` colours `count` as a field; `fn f(p: P)` colours `f` as a function and `P` as a type.

Cost, measured on this machine: the release binary goes from 2.8MB to 12MB, about 1MB per grammar. A clean release build takes the same 28s either way -- the C parse tables compile alongside the Rust.

`function.builtin`, `type.builtin` and `constant.builtin` are three classes, not one. Of the 31 themes that name both `function.builtin` and `type.builtin`, 26 give them their own colours, so collapsing them threw away a distinction the theme had already made. The line scanner cannot follow: jEdit's `keyword2` to `keyword4` is one list holding both, and lands on `function.builtin`, which is what most of it is -- 138 entries for Lua, 303 for Tcl, 174 for Scheme are library functions and variables, against Objective-C's 19, which are types.

One grammar is corrected rather than followed. tree-sitter-rust tags every literal `@constant.builtin`, numbers included, so `treesit` appends two rules tagging integers and floats `@constant.numeric`; a later pattern wins. Without them a Rust `1` and a Python `1` differ in the 29 of 38 themes that give `constant.builtin` and `constant.numeric` their own colours.

### 19.5 A body is a fragment

A Leo body is not a file. It is a method with no class above it, a class whose methods are `@others`, a function whose middle is `<< a section >>`. Measured against tree-sitter-python and tree-sitter-c:

| body | parse |
|---|---|
| method body, no enclosing class | clean |
| indented `def`, no enclosing class | clean |
| `class C:` then `@others` then more methods | clean |
| `def f():` then an inline `<< do the work >>` | error, and `return` on the next line stops being a keyword |

Error recovery handles a missing enclosing scope. Only Leo's own lines break a parse, so `plan()` claims them first -- directives and whole-line section references -- and replaces each with spaces of the same byte width. Spaces beat an identifier, which is not a C statement, and beat a comment, which needs a per-language delimiter. Every offset survives, so the spans come back aligned.

`directiveKind4` allows leading whitespace before `@others` and `@all`, and nothing before the rest. `highlight.rs` matched every directive at column 0 until this mattered: an unclaimed indented `@others` reads as a decorator on the `def` beneath it.

### 19.6 Keeping the answer

`highlight` parses the whole body, and the body pane redraws on every key. Measured, release build, generated Python: 48 lines 0.55ms, 500 lines 2.2ms, 5000 lines 22.7ms. The last row is real -- an `@edit` node holds a whole file in one body -- so `Colouring` keeps the last result, keyed on a hash of the lines and the language.

Incremental re-parse would beat a cache. It costs threading `InputEdit` out of every vim operator in `app.rs`, for a body whose median length in `LeoPyRef.leo` is 12 lines.

### 19.7 The theme

`style_for` used to hold twelve `Color` constants. Twelve classes do not fit the terminal's sixteen colours: only Red, Magenta, DarkGray, LightBlue and LightMagenta clear a contrast of 3.0 on both a black and a white background, so any fixed assignment is unreadable on one of them. The palette was the weak layer, not the class list.

Themes are read from Helix's files. Their scopes *are* tree-sitter capture names -- `keyword`, `type.builtin`, `variable.other.member` -- which is the vocabulary `treesit` already speaks, and resolution is longest-prefix, the rule tree-sitter already applies to captures. The two agree by construction, and a hundred themes exist that nobody here had to write.

Nothing is vendored. `theme.rs` reads `~/.config/leotui/themes`, then `~/.config/helix/themes`, then `$HELIX_RUNTIME/themes`, so leotui carries no other project's files or licence. The theme comes from `--theme NAME`, then the `theme` line of `~/.config/leotui/config.toml`, then the default, `sonokai`. A missing default leaves the built-in sixteen colours, unannounced; a theme asked for by name reports that it is missing.

The parser is by hand. A theme file uses three shapes -- `key = "value"`, `key = { fg = "value", modifiers = [..] }`, and a `[palette]` of names to hex -- against five crates for `toml` and `serde`. Anything it does not recognise is skipped: a theme is decoration, and refusing to start over one helps nobody. `inherits` is followed, parents first, capped at eight to survive a file that names itself.

`bg` and `underline` are parsed and dropped. Painting a pane's background is a decision about the whole frame, not about a run of text, and a theme's background under a terminal's own would leave the outline pane and the borders mismatched.

#### 19.7.1 Fewer colours than the theme wants

`Depth` is truecolor, the 256-colour cube, or the terminal's sixteen, from
`COLORTERM` and `TERM`, and `:set colors=true|256|16` overrides the guess.
Crossterm emits truecolor unconditionally, so the reduction has to be ours.

It measures in CIELAB. Distance in RGB puts grey nearest to anything unsaturated, because (127,127,127) sits in the middle of the cube: sonokai's pink keyword `#fc5d7c` is 16,790 from DarkGray and 24,034 from red, so a whole theme collapsed to two greys at sixteen colours. In CIELAB it is red.

A colour a theme names rather than spells -- `"red"`, not `"#fc5d7c"` -- passes through every depth untouched. The terminal's own palette is already the best answer for it, and the sixteen keep their ratatui names rather than becoming `Indexed`, for the same reason.

The palette is resolved once per frame. A scope walk and a 240-candidate CIELAB search per run of text would cost more than the colouring does.

#### 19.7.2 Choosing one

`:theme` names the current theme; `:theme NAME` changes it. Tab completes over the names on disk, and the candidates are drawn above the command line.

Tab and the arrow keys both move the selection, and the theme is applied as they land on each name, so the outline is the preview. `/` already worked this way -- `preview_search` shows where a search would go -- and `theme_origin` mirrors `search_origin`: Escape puts back the theme that was in force when the line opened, and an accepted line keeps what it shows.

Two details the drop-down forced:

- **The menu reads from the completion, not from the line.** Tab replaces the line with the selection, so recomputing candidates from it would leave a list of one.

- **Completion replaces a slice, not the line.** `Candidates::at` is where the argument starts, so `theme one` completes `one` and leaves the command alone. Before this, completion only ever rewrote the whole line, because only command names completed.

A bare command name gets no drop-down. A list over the whole command table would cover the outline every time `:` is pressed, and vim's in-place completion is what a `:` line is expected to do. `Menu::is_open` answers for both the drawing and the keys, so the arrows never move something the eye cannot see.

Up and Down move the selection only while the drop-down is open, and walk the command history otherwise. Left and Right stay on the cursor: a name is being typed as well as chosen. Tab takes the longest common prefix before it takes a name; an arrow never does, because it is aimed at the list rather than at what is safe to type.

`theme::names` is read when the `:` line opens, not per redraw. It is a `readdir` over 110 files, and a frame must not touch the disk.

#### 19.7.3 Keeping one

An accepted `:theme NAME` writes `theme = "NAME"` to `~/.config/leotui/config.toml`, and startup reads it back. `--theme` wins for one launch and never writes. A preview and Escape never write.

Only that line changes. `parse` obeys the last top-level `theme` line, so that is the one rewritten, keeping any comment after it. With none, the line goes in before the first `[table]`, where TOML still reads it as top-level. Every other line is kept byte for byte.

The file is the user's, so the write is careful:

- A name holding a quote, backslash or newline is refused, not escaped. A theme name is a file stem, and none of those belongs in one.

- A file that exists but cannot be read is an error, not a thing to overwrite.

- A symlink is followed. A dotfiles manager links the file from elsewhere, and renaming onto the link would replace it with a copy.

- The text goes to a temporary file, then a rename puts it in place. A crash mid-write leaves the old file intact.

`App::config_path` is None in `App::new` and set by `main`, like the theme itself. A test that builds an `App` cannot write the user's settings.

### 19.8 What is not done

- **Injections.** A SQL string inside Python, CSS inside HTML.
  `tree_sitter_highlight` takes an injection callback and this passes `|_| None`.

- **Inline section references.** `<< x >>` in the middle of a line is neither coloured nor masked, so the line after it can lose a keyword.

- **The other ~170 languages.** They get the scanner, which is a lexer: it knows comments, strings, numbers, keywords and Leo's own constructs, and not that a Python `f"{x}"` holds an expression.

- **jEdit mode files.** Leo reads them with full span and regex rules. That is 159 more files and a rule engine, and tree-sitter covers the languages it would have paid off on.

- **The rest of the frame.** Only the body pane is themed. Borders, the status line and the outline's own flags still hold their own colours.
