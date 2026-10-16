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

- [ ] Add fuzz targets for what the four in `fuzz/` miss: the line importers (`importers/lines.rs`, where a hostile level hung or panicked the read), a renamed markdown heading re-reading unchanged, and random outline operations with undo, checking `links_agree` (now only in `tests/undo.rs`).

### Usability

### leolib

- [ ] **An external `@clean` edit with an older mtime is skipped, and the next save overwrites it.** `atclean.rs` skips a file whose stamp is not newer, so `cp -p`, `rsync -t` or a restored backup goes unseen, and `seen()` records it. `mod_time_cache` is also set before the read succeeds. Skip only on an equal `FileStamp`; set the cache after a successful read.

- [ ] **A failed `@auto` import is approved for writing without a round-trip check** (`external.rs`, after the import fails). Lines the writer treats as directives (`@c`, `@language x`) are dropped from the source on the first save. Call `remember_read_path` only when `write_string(body) == contents`, as `read_unknown_file_type` does.

- [ ] **An outline's own `@settings` can turn off the directory-must-exist guard** (`createnonexistentdirectories`, used in `external.rs`). Honour it only from `myLeoSettings.leo` or `settings.toml`.

- [ ] **A deeply nested `<v>` overflows the stack and aborts** (`leofile.rs` `VnodeVisitor::visit`; `atfile_write` `put_body` recurses too). 100,000 levels abort a debug build. Cap the depth in `parse`, or make `visit` iterative.

- [ ] **A deep import uses memory quadratic in depth**: each `Position` clones its stack, and an otl file 20,000 tabs deep runs out of memory. `add_parent_links`, `cut_parent_links`, `copy_tree_helper`, `detach_subtree_keeping_clones` and `add_descendant_parent_links` recurse. Cap the import depth; make the helpers iterative.

- [ ] **`Document::delete_node` and `move_to` trust stale positions**: an index panic, or the wrong child removed in release builds. Return `None` when `!position_exists(p)`.

- [ ] **Undoing a large group is quadratic** (`undo.rs`: `position_of` per bead). Undoing `unmark_all` over 3,000 marks takes 0.70 s. Find the selection once, after the last bead.

- [ ] **The line importers are quadratic in headings** (`importers/lines.rs` `body_of` scans). 20k headings take 2.29 s. Keep the body index on each stack entry.

- [ ] **Make `Outline`'s link fields private before 1.0** (needs a decision; breaking). The `pub` link fields and `link_as_last_child_raw` let plugin code break invariants leolib cannot then defend; expose only checked operations.

- [ ] Run the `fuzz/` targets under libFuzzer. They compile on stable but need nightly and `cargo-fuzz` to run, and neither was installed when they were written. A one-minute random-mutation run over `demo/` found the pickle allocation bug and nothing else.

### leoapp and leolsp (both frontends)

- [ ] **A language-server edit lands at stale positions if the user edits within the 150 ms debounce.** `:lsp-rename g` then `dd` passes the version check, and `apply_edits` splices at shifted rows; `workspace/applyEdit` has no check at all (`app/lsp.rs`, `leolsp/src/lib.rs`). Call `sync_now` before `lsp.poll`, or compare the outline generation at request time.

- [ ] **`:bufdo` changes read-only nodes** (`app/ex.rs` `bufdo` calls `doc.set_body`, bypassing `refuse_read_only`), so `@entangled` `include=` fences get rewritten.

- [ ] **A second crash or instance replaces an unopened recovery file**: `NAME.recovered.leo` is a fixed name, and every unsaved new outline shares `recovered.leo` (`app/files.rs`). Add a pid or timestamp, or rename the old file aside.

- [ ] **A theme hex colour with a non-ASCII character panics at every launch** (`theme.rs` `hex` slices `&digits[i..i + 2]`). Return `None` unless `digits.is_ascii()`.

- [ ] **A language server's `Content-Length` is allocated unbounded** (`leolsp/src/server.rs`: `vec![0; length]`), so an absurd value aborts the editor. Cap it at 16 MiB, as leomcp does.

- [ ] **Semantic-token arithmetic overflows on server data** (`leolsp/src/lib.rs`): `1 << i` past 32 modifiers panics in debug builds and sets wrong bits in release. Use `checked_shl` and `checked_add`.

### leogui

- [ ] **The settings dialog strips `\` and `"` from values** (`settings.rs` `quoted`): `C:\tools\clangd.exe` saves as `C:toolsclangd.exe`. `config::parse` does no escape processing; reject `"` and newlines, keep `\`.

### leotui

- [ ] **A panic on any thread restores the terminal while the session runs on** (`main.rs` panic hook). The highlight worker catches its own panics, but the hook has already left raw mode. Restore only on the main thread.

### leomcp

- [ ] **`mcp-edit` plus `mcp-save` let a client create files anywhere the user can write.** `may_overwrite` allows any path that does not exist, so a prompt-injected agent can rename a node to `@file ~/.ssh/authorized_keys` and save (`app/mcp.rs`). Confirm with the user before an MCP save writes outside the outline's directory, or document it.

- [ ] **Changing `mcp-token` without the port keeps the old token valid** until restart (`app/mcp.rs` `set_mcp` returns early on an unchanged port). Restart the server on any MCP setting change.

### Plugins

- [ ] **`@entangled` `include=` reads any file on disk into the markdown** (`leo-entangled/src/lib.rs`). A cloned repo with `include=../../.aws/credentials` puts the secret into `doc.md` on the next save. Refuse absolute paths and paths outside the file's directory. Required before `@entangled` is registered.

- [ ] **`:export-wiki` overwrites an unrelated `NAME.md`** (`leo-wiki/src/app.rs`). Refuse when the target exists and was not written by an earlier export.

- [ ] **Renaming a markdown heading can change the tree on the next read, without a warning** (`leo-markdown/src/lib.rs`). A setext heading renamed to `<!-- draft` opens an unclosed comment; `Issue #` re-reads as `Issue`. Re-scan the written heading with the reader's rules; fall back to ATX, or escape a trailing `#`.

- [ ] **Renaming a parent wiki page breaks qualified links through it**: `[[Guide/Setup]]` is not rewritten when Guide is renamed (`leo-wiki/src/lib.rs`).

### Workspace

- [ ] **Update `SECURITY.md`.** It says `@clean` and `@nosent` are exempt from `may_overwrite` (nothing is now), counts two fuzz targets (there are four), omits leo-markdown, leo-entangled and the language-server reader from the untrusted-input parsers, and does not mention the MCP server, the only network-facing component.

## Low

### Leo coverage

### leolib

- [ ] Port `@jupytext`. It is refused on read and write now. Leo reads a notebook as `@clean` over the `py:percent` text jupytext makes of it (`at.readOneAtJupytextNode`), and writes that text back through jupytext (`writeOneAtJupytextNode`). A port has to do the conversion both ways and keep the cells' outputs and metadata, which the text does not carry.

- [ ] A headline containing `\n` corrupts the `@file` sentinel, and the root body grows on each write and read (`atfile_write.rs`). Escape or reject the newline.

- [ ] A replaced file is umask-readable before its permissions are copied, and owner, group, ACLs and xattrs are lost (`external.rs`). Create the temp file with mode 0600.

- [ ] `toggle_marked` calls `undoer.push`, not `self.push`, so undoing a mark does not set `changed` (`document.rs`).

- [ ] `Kinds::check_new` accepts directive names `find_at_file_name` never matches: `@my.kind`, `@\u{e9}nt`, a bare `@` (`ext.rs` against `node.rs`).

### leoapp

- [ ] **Incremental colouring, as Helix does it** (possible optimization; needs a design). `tree-sitter-highlight` parses from scratch on every call (it passes no old tree to `parse_with_options`) and queries the whole tree, so a long body is coloured whole: 21 ms at 5,000 lines, now on a worker thread. Instead, keep a `tree_sitter::Tree` per cached body, apply each change with `Tree::edit` and re-parse incrementally, and run the highlight query on the visible lines only (`QueryCursor::set_byte_range`). That would remove the partial colouring, the worker and `patch`. Costs: line edits must become byte-offset `InputEdit`s, and injections, `@language` regions and `plan`'s masking move to the lower-level API. Measured on a 5,000-line Python body (2026-10-07, Apple silicon): the whole highlight 21 ms, of which a fresh parse is 12 ms and a whole-tree query 6.7 ms; a query over 50 visible lines 0.07 ms. An incremental re-parse after one typed character costs 0.25 ms in a name or a comment, 6.7 ms for an unclosed quote, and 12 ms when it breaks a keyword (`def` to `qdef`), where error recovery re-parses everything below. So a keystroke would cost 0.3 to 12 ms on the UI thread, against `patch`'s 1.8 ms now, unless the parse stays on the worker; a first visit still needs the 12 ms parse.

- [ ] Any body edit adds or removes the trailing newline: `"\n"` becomes `""`, and the `@file` is marked dirty without a real change (`editor/mod.rs`).

- [ ] A counted put has no size limit and is quadratic in the count: `yG` on 5,000 lines then `99999p` builds about 5e8 lines with a `Vec::insert` each (`editor/mod.rs`).

### leotui

- [ ] The 5 s watchdog can `exit(1)` while the recovery file is being written (`main.rs`).

- [ ] Keyboard enhancement flags are popped twice on a panic, which can pop the parent shell's entry (`main.rs`). Make `restore` idempotent.

### leomcp and leolsp

- [ ] A dead language server's `docs`, `pending` and `diagnostics` are never cleared, and there is no restart command (`leolsp/src/lib.rs`).

- [ ] `didClose` is never sent; a change of `@language` sends `didChange` for a URI the new server never opened (`leolsp/src/lib.rs`).

- [ ] MCP slowloris: the 300 s timeout applies per read, so 16 trickled connections block auth (`leomcp/src/lib.rs`). Add a 10 s deadline per request before auth.

- [ ] On Windows, drive-letter case differs between our URI keys and the server's, and a symlinked root breaks the mapping; diagnostics and rename then fail silently (`leolsp/src/uri.rs`).

- [ ] MCP conformance: a missing `Content-Length` reads as 0; `Bearer` is matched case-sensitively; a JSON-RPC batch is answered `202`; a 30 s timeout replies "did not answer" though the edit may still apply.

### Plugins

- [ ] A wiki rename to an existing headline makes links ambiguous, with no warning (`leo-wiki/src/lib.rs`).

- [ ] The markdown fence closer uses `trim_end()`, which strips U+00A0, so cell boundaries differ from CommonMark's (`leo-markdown/src/lib.rs`).

### leogui

- [ ] On Windows, a release build prints nothing to a terminal: `--help`, `--version` and open errors are lost, because it uses the GUI subsystem. Attach to the parent's console at startup (`AttachConsole`, through `windows-sys`). Needs a Windows machine to test.

- [ ] On macOS, ship a signed `.app` bundle rather than a bare binary, which Gatekeeper blocks until `xattr -d com.apple.quarantine`. leotui's binary has the same problem. Needs an Apple developer account.

- [ ] Scripting and `@button` (needs a design).

- [ ] Two views of one outline (needs a design).

- [ ] Session and recent-outline files are written in place, not atomically, and their paths are trimmed (`session.rs`, `outlines.rs`).

- [ ] The rendered view rebuilds the markdown every frame and re-reads `include=` files from disk (`rendered.rs`).

### Workspace

- [ ] No CI job builds at the declared `rust-version` (1.89, 1.90, 1.95), so leolib's "oldest toolchain it was checked with" is not enforced.

- [ ] Pin GitHub Actions by commit SHA, not tag (`actions/checkout@v7`, `Swatinem/rust-cache@v2`, `dtolnay/rust-toolchain@stable`); release builds use whatever `stable` is current.
