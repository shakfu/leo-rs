# TODO

## Critical

## High

### Plugins

- [ ] **Use `@entangled` on real documents, then release it.** leotui and leogui register `@qmd` and `@rmd` through `leo-plugins`, and `leo-markdown` and `leo-plugins` are publishable. `leo-entangled` stays `publish = false` and out of `leo-plugins`, since crates.io refuses an unpublished dependency, even an optional one; `make ... ENTANGLED=1` builds and tests it.

## Medium

### Plugins

- [x] Explore how to use agents in a leo outline (options in `docs/dev/agents.md`). Here are some ideas:
  - [x] A `leo-agent` plugin crate: the runner (agent table in `agents.toml`, scope, thread, cancel, undo) and `:ask PROMPT`, which inserts the reply below the cursor's line; from VISUAL it sends the selected lines, and `reply=replace` replaces them. `lines=all` acts on the whole body. Typed ranges (`:3,5ask`) are skipped: they need the ex range passed to plugin commands.
  - [x] Headline form, the default: `@agent scope=1 top 5 sort algorithms`; replies go in the body. Built: `:agent-run`, and leogui's double-click through `AppPlugin::activate`.
  - [x] Block form, for long prompts: `@agent` ... `@agent-end` in the body. Line form with it: `@agent prompt=headline`. Built, with `Enter` on the `@agent` line.
  - [x] Parameters: `reply=append|replace|child` (default `append`), `scope=0|N|all`, `agent=NAME` (default `claude`), `provider=NAME`, `model=NAME` (default: the agent's own).
  - [x] MCP approach: an agent whose command names `{mcp}` edits the outline over a server of its own for the run, as one undo step, its tools kept to the scope's subtree. Built.

### leogui

- [ ] On Windows, a release build prints nothing to a terminal: `--help`, `--version` and open errors are lost, because it uses the GUI subsystem. Attach to the parent's console at startup (`AttachConsole`, through `windows-sys`). Needs a Windows machine to test.

- [ ] On macOS, ship a signed `.app` bundle rather than a bare binary, which Gatekeeper blocks until `xattr -d com.apple.quarantine`. leotui's binary has the same problem. Needs an Apple developer account.

- [ ] Scripting and `@button` (deferred; options recorded in `docs/dev/scripting.md`).

- [ ] Two views of one outline (design in `docs/dev/two-views.md`, awaiting answers to its open questions).

## Low

