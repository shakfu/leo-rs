# TODO

## Critical

### leolib

### leo-markdown (blocks publishing)

## High

### Plugins

- [ ] **Use `@entangled` on real documents, then release it.** leotui and leogui register `@qmd` and `@rmd` through `leo-plugins`, and `leo-markdown` and `leo-plugins` are publishable. `leo-entangled` stays `publish = false` and out of `leo-plugins`, since crates.io refuses an unpublished dependency, even an optional one; `make ... ENTANGLED=1` builds and tests it.

### leolib

### leomcp

### leoapp and leolsp (both frontends)

## Medium

### Tests

### Usability

### leolib

### leoapp and leolsp (both frontends)

### leogui

### leotui

### leomcp

### Plugins

### Workspace

### Leo coverage

### leolib

### leoapp

### leotui

### leomcp and leolsp

### Plugins

### leogui

- [ ] On Windows, a release build prints nothing to a terminal: `--help`, `--version` and open errors are lost, because it uses the GUI subsystem. Attach to the parent's console at startup (`AttachConsole`, through `windows-sys`). Needs a Windows machine to test.

- [ ] On macOS, ship a signed `.app` bundle rather than a bare binary, which Gatekeeper blocks until `xattr -d com.apple.quarantine`. leotui's binary has the same problem. Needs an Apple developer account.

- [ ] Scripting and `@button` (deferred; options recorded in `docs/dev/scripting.md`).

- [ ] Two views of one outline (design in `docs/dev/two-views.md`, awaiting answers to its open questions).

### Workspace
