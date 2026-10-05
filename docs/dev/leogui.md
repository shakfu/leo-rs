# A GUI front end: leoegui

A design record. Proposed on 2026-10-05 against leo-rs `8d3a5d8` (v0.6.0) and built the same day: leotui's front-end-neutral code moved into `leoapp`, an egui front end over it, and language servers in both front ends through `leolsp`. Where the build departs from the proposal, the section says so and why.

## Why not gpui

gpui was evaluated first and set aside:

- No usable crates.io release. `gpui` 0.2.2 dates from 2025-10-22 ([crates.io](https://crates.io/crates/gpui)). The README's `gpui_platform` is not published. Apps pin a Zed git rev, as [zaku](https://github.com/buildzaku/zaku) does (`rev = "bd747337d7"`), and a git dependency cannot be published to crates.io.
- No API stability: "pre-1.0. There will often be breaking changes between versions" ([README](https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md)).
- Toolchain: Zed pins Rust 1.98.1 and edition 2024. macOS builds need full Xcode.
- `gpui` is Apache-2.0, but Zed's `editor`, `multi_buffer`, `language`, `text`, `rope`, `vim`, `ui`, `workspace` and `project` are GPL-3.0-or-later (each crate's `Cargo.toml`). zaku is AGPL-3.0. Reusing either editor makes the binary GPL.
- gpui-kit's `Editor` (Apache-2.0) owns its text and cursor, and has no vim mode ([repo](https://github.com/longbridge/gpui-kit)). leotui writes every change through `Document::set_body` and its vim layer owns the cursor.

Either way the body is a custom element over leotui's vim layer. egui (eframe 0.36) gives that with stable crates.io releases.

## Crates

```text
leolib    model. Adds goto::line_map_of, goto::LineMap, goto::body_as_code.
leolsp    language servers: transport, documents, position mapping. leolib only.
leoapp    leotui minus drawing: App, modes, vim editor, commands, bindings,
          search, minibuffer, highlight, theme, view, LSP glue.
leotui    ratatui renderer and terminal loop.
leoegui   eframe renderer and event translation.
```

`leolsp` is a plain dependency of `leoapp`, not behind the proposed feature. A server's threads and processes exist only once the settings name a server and a node in its language is shown, so the feature would have saved compile time and nothing at run time.

## Input

`leoapp::keys` defines `KeyCode`, `KeyModifiers` and `KeyEvent` with crossterm's names, so the move changed imports and nothing else. `KeyCode::Null` stands for a key with no name, which nothing binds.

The proposed `Input` enum became five methods: `handle_key`, `handle_text`, `click_tree`, `click_body` and `scroll`. Each front end calls the one its event means, and an enum would only be matched straight back into them.

egui-winit sends a printable key twice, as `Event::Key` and `Event::Text`, and sends no text while Ctrl or Cmd is held. It turns Cmd-C, Cmd-X and Cmd-V (Ctrl elsewhere) into `Copy`, `Cut` and `Paste` with no key event (`egui-winit-0.36.2/src/lib.rs`, `on_keyboard_input`). So `leoegui::input` takes a bare printable key from its text, a chord from its key, turns `Copy` back into `Ctrl-c` and `Cut` into `Ctrl-x`, and pastes `Paste`. The text egui sends after an Alt chord (Option-g types a symbol on macOS) is dropped. On Windows `Ctrl-Insert` also arrives as `Copy`, so it reaches leoapp as `Ctrl-c`, not as `insert-child`.

On macOS, Cmd is Leo's Ctrl, as Qt makes it for Leo (`ControlModifier` is the Command key, [Qt docs](https://doc.qt.io/qt-6/qt.html#KeyboardModifier-enum)). leoegui sends Cmd as SUPER, and leoapp runs a Cmd chord the outline binds from either pane, so Cmd-R indents the node from the body as in Leo; any other Cmd chord is Ctrl. Control stays leoapp's Ctrl, which is vim's in the body. A first version made Cmd and Control both Ctrl, which made Cmd-R vim's redo. Leo's `qt-mac-dont-swap-ctrl-and-meta` (`leo/plugins/qt_gui.py:118`, leo-editor `e3b3841f64`) makes Cmd Meta, which binds nothing. leoegui turns off winit's default menu, so Cmd-Q, Cmd-H and Cmd-M reach the app rather than macOS.

## View

`App::tree_view`, `body_view` and `help_view` take a pane's size in cells, scroll as `ui.rs` used to, and return plain data. Scroll stays in lines, because `zz`, `Ctrl-d` and `H`/`M`/`L` are defined in lines. Both front ends draw the body as a monospace grid, so a vim column is a screen column. `view::decorate` cuts a line wherever its colouring, a search match or a diagnostic starts or ends; both renderers style its segments.

The proposed `BodyView<'a>` borrowing the text became an owned `BodyView`: the working copy is cloned once per draw, as `ui.rs` did, and a borrow would tie the view to `&App` while the renderer also needs `&mut App` for clicks.

`--dump` frames from leotui were compared with v0.6.0's on 342 states (6 outlines, 19 key sequences, 3 sizes): identical.

## ropey: not taken

The proposal's case for ropey was index conversion, chiefly UTF-16 columns for LSP. `leolsp::map::Encoding` does that over plain lines, and the servers choose UTF-8 when they offer it. Porting the editor's 2,000 lines of `&[String]` indexing to a rope buys nothing measured: bodies are a few KB. Revisit if a profile shows large bodies.

## LSP

A server sees each external file as `external::file_contents` writes it; a node in no file is an `untitled:` document of its body, directives blanked by `goto::body_as_code`.

- `goto::line_map_of` maps each file line to a gnx, a body row, and the indent `@others` put before it, or no column for a sentinel or directive. It refuses a file the sentinel writer does not write: the check is that `write_to_string` with the file's sentinel setting gives the same text. `@file`, `@clean`, `@nosent` and code `@auto` map; `@edit`, `@asis` and `@auto` of markdown, org, rst or otl do not. The proposal assumed every `@auto` file went through `AtWrite`; the line importers write their own.
- Coverage on `demo/LeoPyRef.leo`: 374 of 381 roots map. The 7 that do not are `@edit`.
- Servers start only when the settings name one (`lsp-python = "pylsp"`). rust-analyzer runs build scripts, so a `.leo` file from someone else must not choose a server.
- Sync: full text, 150 ms after the outline stops changing, at once on moving to another node. INSERT's working copy is not sent; the commit is, as neovim's default `update_in_insert = false`.
- Keys: `K` hover, `Ctrl-]` definition, `]d` `[d` diagnostics, neovim's. `:lsp-rename NAME` and `:lsp-diagnostics`. An answer that arrives during INSERT or a `:` line is not acted on.
- Edits apply all or none, as one undo step. Refused: an edit across a sentinel or into another body, an edit to a file the outline does not hold, file operations, and a rename whose document changed since the request.
- Checked against Apple clangd 21.0.0: diagnostics land on the body row and column behind an `@others` indent, hover and definition answer from a root into a child node, and a rename edits two nodes and undoes as one.
- Not built: completion. INSERT-mode completion needs a popup and an insertion protocol in the vim layer; neither front end has one yet.

## Open

1. Completion, above.
2. leoegui reads leotui's settings and themes, `~/.config/leotui/`. One file for both front ends is deliberate for now; a rename to a shared name would move users' files.
3. IME composition in leoegui is untested on a real input method. The code follows egui's events; whether winit sends `Commit` or `Text` for ASCII typing with IME allowed differs by platform.
4. Diagnostics drawn in leoegui are untested on screen: the display was off when it was ready to check. The same marks are tested in leotui's render test.
