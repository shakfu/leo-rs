# Scripting and `@button`: options and costs

Status: design record, 2026-10-25. Nothing here is scheduled.

The decision stands:

- Scripting is deferred (`ideas.md`, `leocub-vs-leotui.md`).
- Rhai is the chosen path if it is ever wanted.
- Code-in-outline node kinds, such as leo-cub's `@action`, are rejected.

This note records what each option would cost, for the day the deferral is reviewed. It answers the `TODO.md` item "Scripting and `@button` (needs a design)".

Terms:

- **Script**: user code the editor runs at run time, not compiled into the binary.
- **Host API**: the functions the editor gives a script.
- **Code-in-outline**: a script stored in a node body of the `.leo` file.
- **Handle**: the value a script holds to name a node. Here, a gnx string.

## 1. What Leo offers

| Feature | How it runs | Source in leo-editor | Default |
|-|-|-|-|
| `execute-script` | Runs `c.p`'s script with `exec(script, d)`; `d` holds `c`, `g`, `p`, `input`, `script_args`, `script_gnx` | `leo/core/leoCommands.py`: `Commands.executeScript`, `executeScriptHelper`; script text from `getScript` in `leo/leolib/util.py`, which expands section references and `@others` | on |
| `@button` | A button in the icon bar and a command; runs on click | `leo/plugins/mod_scripting.py`: `ScriptingController.handleAtButtonNode`, `AtButtonCallback.execute_script`, `executeScriptFromButton` | `scripting-at-button-nodes = True` |
| `@command` | A minibuffer command; `@key=` binds a key, `@args=` sets `script_args` | `mod_scripting.py`: `handleAtCommandNode` | `scripting-at-commands-nodes = True` |
| `@rclick` | A context-menu entry under an `@button` | `mod_scripting.py`: `handleAtRclickNode`, `build_rclick_tree` | `scripting-at-rclick-nodes = False` |
| `@script` | Runs when the outline opens | `mod_scripting.py`: `handleAtScriptNode` | off; an outline's own setting is ignored (`g.issueSecurityWarning`) |
| `@test` | Each node runs under pytest's assert rewriting | `leoCommands.py`: `execute_pytest` | on |

`ScriptingController.createAllButtons` scans the whole outline on open for these headlines. Buttons and commands are created then; no code runs until the user clicks or types the command.

The host API is all of Leo, since a script gets `c` and `g` objects with no filter. Typical use is narrower. `leo/scripts/scripts.leo` holds 365 bodies. The most frequent `c.`, `g.` and `p.` attributes in them:

| Group | Attributes (count) |
|-|-|
| node text | `p.h` 36, `p.b` 23, `p.headString` 4, `p.v` 4 |
| navigation | `c.p` 19, `g.findNodeAnywhere` 11, `g.findNodeInTree` 5, `c.selectPosition` 6, `c.lastTopLevel` 4, `p.level` 3, `p.copy` 3 |
| output | `g.trace` 59, `g.es_print` 34, `g.es` 13, `g.printObj` 9, `g.cls` 21 |
| state and files | `c.redraw` 13, `c.isChanged` 6, `c.save` 6, `p.setDirty` 3, `p.isAnyAtFileNode` 3, `c.fullPath` 3 |
| utilities | `g.splitLines` 19, `g.toUnicode` 9, `g.os_path_*` 23 in total |

These are Leo's own maintenance scripts; user scripts may differ. Leo records no undo for a script unless it calls `c.undoer` itself: `beforeChangeBody`, `afterChangeBody`, `beforeChangeGroup`, `afterChangeGroup` in `leo/core/leoUndo.py`.

## 2. What leo-rs already covers

Three mechanisms exist.

- **Compiled plugins.** `leolib::ext` defines `FileKind` and `TreeKind` (`crates/leolib/src/ext.rs`). `leoapp::plugins::AppPlugin` adds commands, ex commands with an argument (`with_argument`), settings, menu entries, background polling, `open_url`, completion and rule checks (`violations`). `leo-plugins` registers them at build time. Design: `docs/dev/plugins.md`.
- **The command table and ex commands.** `COMMANDS` in `crates/leoapp/src/commands.rs` holds 136 entries. Each is a `Command { name: &'static str, summary, run: fn(&mut App, usize) }`. `commands::all()` chains the plugins' commands, so `:help`, completion and leogui's palette see both. Ex commands (`crates/leoapp/src/app/ex.rs`) add `:s`, `:bufdo`, `:clone-find-all`, `:import-auto` and others.
- **MCP tools.** `crates/leoapp/src/app/mcp.rs` serves 11 tools over `leomcp`: `outline`, `read_node`, `search`, `selection`, `set_headline`, `set_body`, `insert_node`, `delete_node`, `move_node`, `select_node`, `save`. Nodes are named by gnx. Edits need `mcp-edit`, saves `mcp-save`. Any program with the token can call them today over HTTP on 127.0.0.1.

Which Leo use cases each covers:

| Leo use case | Compiled plugin | Ex / command | MCP + external program |
|-|-|-|-|
| Reformat or rewrite bodies | yes, needs Rust and a rebuild | `:s`, `:bufdo` for regex rewrites | yes: `read_node`, `set_body` |
| Generate nodes from data | yes | no | yes: `insert_node` |
| Walk the tree and report | yes | `clone-find-all` for matches | yes: `outline`, `search` (500 nodes per call) |
| A new file format | yes: `FileKind` | no | no |
| Run an external tool | yes: `:entangled-tangle` spawns one | no; `:!cmd` is ranked in `ideas.md`, not built | the program is the tool |
| Bind user code to a name or key | only for the plugin author | no user-defined commands | no; started outside leo-rs |
| `@test` | no | no | no |

Gaps in the MCP route:

- No tool for marks, clones, unknown attributes or the body selection.
- Each edit is its own undo step; only `insert_node` groups its parts.
- An edit waits for NORMAL mode.
- leo-rs cannot start the program; the user runs it from a shell.

## 3. Options

Three costs are shared by (b), (c) and (d):

- **Run-time commands.** `Command` holds `&'static str` and a `fn` pointer. A script's command needs a second, owned table that `commands::all()` chains, or leaked strings and a trampoline. About 1 to 2 days.
- **One undo step per run.** `Document::begin_group` and `end_group` exist (`crates/leolib/src/document.rs`). Wrapping a run in one group makes it one undo step. `App::check_rules` then runs once, keyed on `Undoer::version`.
- **Handles are gnx strings**, as in MCP. A `Position` held across edits goes stale; a gnx does not.

Effort figures are estimates in person-days, from the size of `mcp.rs` (733 lines) and `plugins/mod.rs`. None was measured.

### (a) No scripting; MCP plus external programs

- **API**: the 11 MCP tools, unchanged.
- **Security**: no code in outlines. A client is as trusted as its token holder; `SECURITY.md` covers it. New files on save already wait for approval (`hold_new_file`).
- **Undo**: one step per edit call.
- **Cost**: none. No new crate.
- **MSRV**: none.
- **Effort**: 0. Optional: 2 to 3 days for `:!cmd` from `ideas.md`, to start a program from leo-rs. 1 to 2 days for marks and clones tools.
- **Conflicts**: none. This is the current state.

### (b) Rhai, commands only, scripts in the config directory

Scripts are `.rhai` files under `~/.config/leo-rs/scripts/`, enabled in `settings.toml`. Each registers commands by name.

- **API**: the MCP tool set as Rhai functions, plus `current()`, marks, and `message()` writing to `:messages`. About 20 functions. Edits go through `Document` methods only, never `outline_mut_untracked`.
- **Security**: an outline carries no code. Rhai has no file or process access unless the host registers it ([Rhai book](https://rhai.rs/book/about/features.html)). `Engine::set_max_operations`, `set_max_call_levels` and `set_max_string_size` stop a runaway script. A save that creates files waits for approval, as `save` over MCP does.
- **Undo**: one group per run.
- **Cost**: `rhai` 1.27.0 is a 525 KB crate. It adds 7 crates absent from `Cargo.lock`: `rhai`, `rhai_codegen`, `smartstring`, `thin-vec`, `const-random`, `const-random-macro`, `tiny-keccak`. A scratch release build with `lto = true` grew from 0.36 MB to 4.29 MB: about +3.9 MB. leotui's release binary was 14.4 MB on 2026-09-28. Behind a cargo feature, the cost is opt-in.
- **MSRV**: `rhai` declares 1.66; the workspace is at 1.90. None.
- **Effort**: 8 to 12 days: engine and limits 2, API 3 to 4, run-time commands 2, loading and errors 1 to 2, tests 2.
- **Conflicts**: the deferral (`ideas.md`, the project memory). It follows the rules in `leocub-vs-leotui.md`: code in the config directory, edits through `Document`, `leolib` free of scripting.

### (c) Rhai with `@button` and `@command` nodes in outlines

Everything in (b), plus nodes the app scans for.

- **API**: as (b), plus `@key=` and `@args=` parsing, a rescan after edits, and a button row in leogui. leotui has no button row; `@button` would become a command there.
- **Security**: needs a trust store keyed on outline path, script gnx and a hash of the body. Prompt on first run and after any change to the body, its clones, or its `<< sections >>`. `SECURITY.md`'s "Nothing in a `.leo` file is executed" stops being true.
- **Language clash**: Leo's `@button` bodies are Python. `executeScriptFromButton` passes the script text to `executeScript`, which skips its language check. Leo would run a Rhai body as Python and fail. leo-rs cannot run Python bodies. A shared outline breaks in both editors, unless a new directive is invented. A new directive is `@action` by another name.
- **Undo**: as (b).
- **Cost and MSRV**: as (b).
- **Effort**: 18 to 28 days: (b), plus scanning and rescans 3, trust store and prompts 4 to 6, leogui buttons 2 to 3, conformance tests 2.
- **Conflicts**: the rejection of code-in-outline nodes; `leocub-vs-leotui.md` ("Plugins live in the config directory ... A `.leo` file then carries no code"); `SECURITY.md`.

### (d) External processes over stdio JSON, reusing the MCP tools

`settings.toml` names programs: `[scripts] tidy = ["python3", "~/bin/tidy.py"]`. `:tidy` spawns the program. The program sends JSON-RPC 2.0 requests, one per line, on stdout. Each request names an MCP tool; the app answers on the program's stdin.

- **API**: the MCP tool names and schemas. Dispatch reuses `App::mcp_tool` unchanged. Any language works, Python included.
- **Security**: an outline carries no code. The program runs with the user's full rights, as Leo's Python does. Access follows `mcp-edit` and `mcp-save`, or a per-script flag.
- **Undo**: one group per run. Requests arrive between frames, so the group stays open across ticks. The app must refuse user edits while it is open, or close it on the first one.
- **Cost**: `serde_json` is already a dependency. No new crate. Process handling follows `leo-entangled/src/app.rs` and `leolsp/src/server.rs`, which already spawn children.
- **MSRV**: none.
- **Effort**: 6 to 10 days: spawn, framing and timeouts 2 to 3, the undo window 2, run-time commands 1 to 2, tests 2.
- **Conflicts**: the deferral; it is an extension hook by another name. It is not Rhai. It overlaps the deferred CLI's JSON batches (`leocub-vs-leotui.md`, "CLI: deferred").

### (e) WebAssembly

A script is a `.wasm` module in the config directory. WebAssembly (WASM) is a bytecode a host runs in an isolated memory.

- **API**: host functions over linear memory, or WIT interfaces under the component model (wasmtime only). Strings cross as pointer and length.
- **Security**: a module reaches nothing the host does not import. Fuel metering stops runaway loops. The strongest isolation of the five.
- **Undo**: as (b).
- **Cost**: `wasmtime` 50.0.0 is a 1.07 MB crate and pulls in Cranelift. `wasmi` 2.0.0, an interpreter, is 301 KB. `extism` 1.41.0 wraps wasmtime. Binary growth not measured.
- **MSRV**: `wasmtime` 50.0.0 declares 1.97, above the workspace's 1.90. `wasmi` declares 1.86: none.
- **Authoring**: a user needs a compiler that targets `wasm32`. A script cannot be edited and rerun in the editor.
- **Effort**: 15 to 25 days. The ABI design dominates.
- **Conflicts**: the deferral; Rhai is the chosen path; `plugins.md` rejected WASM for compiled plugins (that reason, "nothing needs loading at run time", does not apply to scripts).

### Summary

| | (a) MCP | (b) Rhai, config | (c) Rhai, in outline | (d) stdio JSON | (e) WASM |
|-|-|-|-|-|-|
| Code in `.leo` file | no | no | yes | no | no |
| Sandbox | n/a | yes, by registration | yes, by registration | no | yes |
| Trust prompt needed | no | no | yes | no | no |
| Undo | per call | per run | per run | per run | per run |
| New crates | 0 | 7 | 7 | 0 | 1 to 60+ |
| Binary growth | 0 | ~3.9 MB | ~3.9 MB | ~0 | not measured |
| MSRV raised | no | no | no | no | wasmtime: yes |
| Effort, days | 0 | 8-12 | 18-28 | 6-10 | 15-25 |
| Breaks a recorded decision | no | deferral only | deferral, code-in-outline | deferral, Rhai choice | deferral, Rhai choice |

The crate counts for (e) are inference: wasmi has few dependencies, wasmtime many. They were not counted.

## 4. Security

Threat model:

- **Attacker**: whoever wrote a `.leo` file the user opens: a cloned repository, an attachment, a pull request.
- **Goal**: run code as the user, or plant code the user later runs.
- **Today**: `SECURITY.md` treats a `.leo` file as a Makefile for writes. Nothing in it is executed.

How code in an outline reaches execution, and what Leo does:

1. **On open.** `@script` runs at load. Leo turns it off and ignores an outline's own `@bool scripting-at-script-nodes`.
2. **On a click.** Leo registers every `@button` and `@command` on open, with no prompt. A button labelled as a harmless action runs the author's code on first press. `@key=` lets the outline bind a key; whether it can shadow a built-in binding was not checked.
3. **After trust.** A trusted body changes through a clone, a `<< section >>` it expands, or a `git pull`. Leo has no trust state, so this does not arise there.
4. **Through imports.** `executeScript` puts the outline's directory and the working directory on `sys.path`. A `.py` file planted beside the outline is imported by name.

Leo runs scripts with `exec` in the editor's own Python process. It has no sandbox and no trust prompt. The only guard is the `@script` setting.

What each option needs:

| Option | Needs |
|-|-|
| (a) | Nothing new. The token and the `mcp-edit` and `mcp-save` gates exist. |
| (b) | No file or process functions registered. Operation, depth and size limits. Save approval for new files. Scripts only from the config directory. |
| (c) | All of (b). A trust store on path, gnx and body hash, including clones and expanded sections. A prompt before first run and after change. No run on open. No `@key=` that shadows a built-in. A rewrite of `SECURITY.md`. |
| (d) | Programs named only in `settings.toml`, never in an outline. A timeout and kill. Save approval for new files. The program itself is not sandboxed. |
| (e) | No WASI imports by default. Fuel limits. Save approval for new files. |

## 5. Recommendation, if the deferral is lifted

The current decision is deferral. This section applies only after it is reversed.

Recommended: (b), Rhai with commands only, from the config directory.

- It matches the recorded choice and the rules in `leocub-vs-leotui.md`.
- The API is the MCP tool set, whose semantics, gnx handles and access gates are tested.
- One undo group per run fixes MCP's one-step-per-call gap.
- It is behind a cargo feature, so a build without it costs nothing.
- leo-cub's `src/rhai_run.rs` and `docs/reference/rhai-api.md` are a working reference.

Do not build (c). It is the rejected code-in-outline form, and its bodies clash with Leo's Python `@button`s in shared outlines.

Rejected alternative framing: "the need is automation, not an embedded language". Under it, (a) then (d) come first. MCP exists, any language works, and the binary does not grow. It was rejected because:

- it contradicts the recorded Rhai choice;
- (d) runs child programs with the user's full rights, with no sandbox;
- holding an undo group open across frames while the user can type is a new failure mode;
- Leo users' Python scripts need a rewrite either way: the API is MCP's, not `c`, `g` and `p`.

A second framing, from `ideas.md`: if leotui runs beside leo-editor, scripting stays in leo-editor. Then deferral costs nothing, and none of these options is needed.

## 6. Open questions

- Does leo-rs replace leo-editor or run beside it? (`ideas.md`)
- Which scripts do users run? `scripts.leo` is Leo's own maintenance; a sample of user scripts is needed.
- Is binary size a constraint? (b) adds about 27% to a 14.4 MB leotui.
- May a script save, or run an external program? Each widens what (b) must guard.
- Should a script's edits be one undo step per run, or per call as MCP has?
- How does a long script behave? Rhai runs on the UI thread; `set_max_operations` aborts it, but the UI blocks until then.
- Does the script API get its own version, apart from leoapp's?
- Is `@test` in scope at all? No option above ports it.
