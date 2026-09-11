# Feature ideas

A brainstorm of 2026-09-10: features proposed for leotui, and an assessment of each. It is judgement unless a line says it was measured. Nothing here is scheduled.

One constraint was set at the start: no plugins and no scripting.

## The prior question

`comparison.md` asks what leo-rs is for: a daily Leo in a terminal, a library for Leo files, or a different outliner on Leo's formats. Each idea below serves some of these and costs the others. The answer decides how many to build.

## The ideas

| idea | cost | value | risk |
|-|-|-|-|
| terminal pane | high: a PTY and a terminal emulator inside ratatui | low: tmux, zellij and terminal splits already do this | a terminal emulator to maintain |
| log pane | low | `handle_key` clears the status message on every key, so a message is gone after one keypress | none |
| several outlines open | medium: `App` holds one `Document`, so undo, search origin and cursor become per outline | moving nodes between outlines | state threaded through all of `App` |
| quarto | low, as a shell command | render markdown to several formats | `@file` markdown carries sentinel comments; `@clean` avoids them |
| ripgrep | low to medium | search files that are not in the outline | shallow unless each hit maps to a node |
| fzf | medium: leotui must give up the terminal and take it back | pick a node by name | a built-in fuzzy picker over headlines needs no external tool |
| ruff format | medium to high | formatting for Python outlines | a formatter run on a file with sentinels may move them; untested |
| ruff check | medium | diagnostics | none, given the line mapping below |
| LSP | very high: JSON-RPC, document sync, and a UI for diagnostics, hover and completion | language diagnostics | the server sees the whole external file; the user edits node bodies |
| MCP | medium, as a separate binary over `leolib` | outline-aware AI tools | two writers on one set of files; see the constraints below |

## Two generic pieces instead of per-tool integrations

### Mapping file lines to node lines

Most tools report `file:line:col: message`: ripgrep, ruff, compilers, quarto. LSP diagnostics use the same coordinates. The sentinels in an `@file` external file record which node each line came from, so a line of the file maps to a node and a line of its body, and back.

With that mapping in `leolib`, any tool that reports locations can jump to the right node. It is library API, so it also serves the library answer to the prior question.

Formatters need the other direction. `atclean.rs` ports Leo's `@clean` update algorithm, which merges an edited, sentinel-free file back into the tree with `SequenceMatcher`. So a formatter can run on the file written without sentinels, and the update algorithm can push its changes into the nodes. The formatter never sees a sentinel.

### vim's shell commands and quickfix list

- `:!cmd` runs a command. `%` expands to the current node's external file: `:!quarto render %`.
- `:[range]!cmd` filters body lines through a command and replaces them with its output: `:%!ruff format -`.
- A quickfix list, as vim's `:cexpr`, `:cnext` and `:copen`, reads `file:line:col: message` lines from any tool and steps through them with the mapping above.

These cover ripgrep, ruff, compilers and quarto with no code specific to any of them, and without plugins. They follow vim, as `:s`, `/`, `:set` and `Ctrl-w` did.

## Ranking

1. File-line to node-line mapping, as `leolib` API.
2. `:!`, `:[range]!` and a quickfix list.
3. `:messages`, a log of status messages.
4. A built-in fuzzy node picker.
5. Later: MCP as a separate binary, after leotui can reload changed files. Several outlines open, once moving nodes between files is needed.
6. Not now: the terminal pane. LSP is deferred, since quickfix over `ruff check` gives Python its diagnostics.

## Constraints any of these must respect

- `leolib` stays free of TUI and tool dependencies; `README.md` says nothing in it depends on `leotui`. The mapping belongs in `leolib`, and running tools belongs in `leotui`.
- Tools are found on `PATH` at run time. A missing tool gets a status-line message, not a build requirement.
- A tool that draws on the terminal, such as fzf, needs leotui to leave raw mode and the alternate screen, then restore both.
- leotui reads external files at startup and on `:e`, and not again; `leolib`'s `mod_time_cache` only lets a read skip an unchanged `@clean` file. So a running leotui never sees an outside writer: a formatter, an MCP server, another editor. `w` would then overwrite that writer's changes without asking, because the file counts as read. Reload-on-change must come before MCP or in-place formatting.
- The release binary is 12MB, mostly tree-sitter grammars (CHANGELOG, 0.2.0). LSP and MCP crates would add to it; how much is not measured.

## Open questions

- Who uses leotui: one person inside tmux, or others on plain terminals? The first makes a terminal pane redundant.
- Which languages fill the outlines in use? If mostly Python, ruff with quickfix covers most of what LSP would.
- Do other editors change these files while leotui is open? If so, reload-on-change comes first, before anything above.
- Does leotui replace leo-editor, or run beside it? Beside it, scripting stays in leo-editor, and "no scripting" costs nothing here.
- Is binary size a constraint?
