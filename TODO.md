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

- [ ] Run the `fuzz/` targets under libFuzzer. They compile on stable but need nightly and `cargo-fuzz` to run, and neither was installed when they were written. A one-minute random-mutation run over `demo/` found the pickle allocation bug and nothing else.

### leoapp and leolsp (both frontends)

### leogui

### leotui

Nothing open.

### Workspace

## Low

### Leo coverage

### leolib

- [ ] Port `@jupytext`. It is refused on read and write now. Leo reads a notebook as `@clean` over the `py:percent` text jupytext makes of it (`at.readOneAtJupytextNode`), and writes that text back through jupytext (`writeOneAtJupytextNode`). A port has to do the conversion both ways and keep the cells' outputs and metadata, which the text does not carry.

### leoapp

- [ ] **Incremental colouring, as Helix does it** (possible optimization; needs a design). `tree-sitter-highlight` parses from scratch on every call (it passes no old tree to `parse_with_options`) and queries the whole tree, so a long body is coloured whole: 21 ms at 5,000 lines, now on a worker thread. Instead, keep a `tree_sitter::Tree` per cached body, apply each change with `Tree::edit` and re-parse incrementally, and run the highlight query on the visible lines only (`QueryCursor::set_byte_range`). That would remove the partial colouring, the worker and `patch`. Costs: line edits must become byte-offset `InputEdit`s, and injections, `@language` regions and `plan`'s masking move to the lower-level API. Measured on a 5,000-line Python body (2026-10-07, Apple silicon): the whole highlight 21 ms, of which a fresh parse is 12 ms and a whole-tree query 6.7 ms; a query over 50 visible lines 0.07 ms. An incremental re-parse after one typed character costs 0.25 ms in a name or a comment, 6.7 ms for an unclosed quote, and 12 ms when it breaks a keyword (`def` to `qdef`), where error recovery re-parses everything below. So a keystroke would cost 0.3 to 12 ms on the UI thread, against `patch`'s 1.8 ms now, unless the parse stays on the worker; a first visit still needs the 12 ms parse.

### leogui

- [ ] On Windows, a release build prints nothing to a terminal: `--help`, `--version` and open errors are lost, because it uses the GUI subsystem. Attach to the parent's console at startup (`AttachConsole`, through `windows-sys`). Needs a Windows machine to test.

- [ ] On macOS, ship a signed `.app` bundle rather than a bare binary, which Gatekeeper blocks until `xattr -d com.apple.quarantine`. leotui's binary has the same problem. Needs an Apple developer account.

- [ ] Scripting and `@button` (needs a design).

- [ ] Two views of one outline (needs a design).
