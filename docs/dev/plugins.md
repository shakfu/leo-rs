# Plugins: file kinds and app extensions outside leolib

Status: phases 1 to 4 done 2026-10-08. The app-plugin list is per process (below); the binaries assert in debug builds that registration came first.

## Goal

- leolib stays a port of Leo.
- The leo-rs-only kinds move into crates of their own:
  - `@entangled`
  - `@auto-cells` (to become `@qmd` and `@rmd`)
  - `@wiki`, when it is built
- leolib keeps only an extension API that those crates implement.
- With no plugins registered, leolib behaves as Leo, and a test checks
  this.

## Decisions (2026-10-08)

- **Two layers.**
  - File kinds: a trait in leolib.
  - App extensions: a trait in leoapp. These are commands, settings,
    menu entries and background jobs.
- **The file-kind trait lives in leolib.** Its methods take `Outline` and
  `Position`, and leolib's read, write and save loops call it. A separate
  trait crate would depend on leolib for those types while leolib
  depended on it for the trait.
- **Registration is an explicit list** in leoapp, behind cargo features.
  - Not `inventory` or `linkme`: link-time registration depends on the
    platform's linker, and hides the set of plugins from the code.
  - Not dylib or WASM: Rust has no stable ABI, and nothing needs loading
    at run time.
- **App extensions use a cargo feature.** A plugin crate implements
  leoapp's trait behind a `leoapp` feature, so its file kind does not
  depend on leoapp.
- **`@md` is deferred.**
  - `@auto` on `.md` keeps Leo's importer: headings only.
  - Once `@auto-cells` is split, cells in a plain `.md` file are not
    nodes. They come back only if a later `@md` adds them.

## Layer 1: `leolib::ext`

```rust
pub trait FileKind: Send + Sync {
    /// The headline directive, `@qmd`.
    fn directive(&self) -> &'static str;
    fn read(&self, o: &mut Outline, p: &Position) -> Result<bool>;
    fn write(&self, o: &Outline, p: &Position) -> Result<String>;
    /// Before the `.leo` file is written: set what the node saves.
    fn before_save(&self, _o: &mut Outline, _p: &Position) {}
    /// Whether the `.leo` file keeps the node's body.
    fn stores_body(&self) -> bool { true }
    /// Read before other external files (clones shared with `@clean`).
    fn read_first(&self) -> bool { false }
    fn plan_rename(&self, _o: &Outline, _p: &Position, _headline: &str)
        -> Option<std::result::Result<Rename, String>> { None }
    /// Why p may not be edited, if it may not.
    fn read_only(&self, _o: &Outline, _p: &Position) -> Option<String> { None }
    /// A node's markdown, for the rendered view.
    fn node_markdown(&self, _o: &Outline, _p: &Position) -> Option<String> { None }
    /// Whether leolsp serves each node as a document of its own.
    fn nodes_are_documents(&self) -> bool { false }
}
```

**Registry.** `Kinds` is a list of `Arc<dyn FileKind>`. Where it lives
is open; see below. `Kinds::empty()` is Leo.
- Registering a directive Leo defines is refused: `AT_AUTO_NAMES`,
  `AT_FILE_NAMES`, `@clean`, `@edit`. Plugins add kinds; they never
  replace Leo's.

**Call sites routed through it.** These are today's direct references:

| Site | Today |
|-|-|
| directive recognition | `node.rs` (`at_entangled_node_name`, `at_auto_cells_node_name`); `position.rs` |
| file collection | `external.rs` (`find_files`) |
| read and write dispatch | `external.rs` |
| read order | `external.rs` (`read_files`) |
| stored as external; save hook; empty body | `leofile.rs` (`put_v_element`, `put_t_elements`) |
| rename | `document.rs` (`rename_entangled_block`) |
| nodes as documents | `leolsp/src/lib.rs` (`source_of`) |

`language_at` already reads the `leo-rs-language` attribute, which needs
no hook.

**Extension API.** These are public, in `leolib::ext`:
- `Outline::detach_subtree_keeping_clones`;
- `Outline::link_as_last_child_raw`;
- import warnings, `refilled`, and `remember_read_path`;
- `Error::Import` and `Error::Write`;
- `read_file_to_string`, `util::split_lines`, and the language tables.

**Attribute names.** Each plugin uses its own prefix:
- `leo-rs-<plugin>-` for attributes held in memory, never saved;
- `str_leo-rs-<plugin>-` for saved attributes. Leo keeps `str_` values
  as text.

## Where the registry lives (decided 2026-10-08: the `Outline` field)

leolib's read, write and save code must find the registered kinds. Two
options:

| | Global (`static KINDS: OnceLock<Kinds>`) | Field on `Outline` (`o.kinds: Arc<Kinds>`) |
|-|-|-|
| Reached by | any code, with no parameter | the outline being worked on |
| Set | once, at program start | when a document is opened (`Document::open_with`) |
| Tests | one set per test process; a no-plugins test cannot run beside one that registers kinds | each test picks its set; the parity test opens with none |
| Two outlines with different sets | no | yes |
| Cost | smallest change | every open path passes the set |

Chosen: the field, for the parity test and per-test isolation.

## Layer 2: `leoapp::plugins`

As built in phase 2 (`crates/leoapp/src/plugins/mod.rs`):

```rust
pub trait AppPlugin: Sync {
    fn name(&self) -> &'static str;
    fn commands(&self) -> &'static [Command] { &[] }          // the core's Command type
    fn with_argument(&self, name: &str) -> Option<fn(&mut App, &str)> { None }
    fn settings(&self) -> &'static [&'static str] { &[] }     // keys config.rs keeps
    fn menu(&self) -> &'static [MenuEntry] { &[] }
    fn poll(&self, app: &mut App) -> bool { false }
    fn poll_after(&self, app: &App) -> Option<Duration> { None }
}
```

- **Plugins are static values.** Their state lives in the app, as
  `App::plugin_data::<T>()`, one value per type. A plugin with `&mut self`
  state would be borrowed by the app while `poll` borrowed the app.
- **Commands are the core's `Command` type.**
  - `commands::find` and `commands::all()` cover the core's and the
    plugins', so these need no change: ex commands, `:help`, completion,
    leogui's palette and hints.
  - `with_argument` runs a command given an argument on the `:` line.
- **Settings:** a key a plugin reads goes to `Config::plugin`. A key no
  plugin reads is still reported as unknown. The `entangled = PATH` key is
  unchanged for users.
- **Menus:** leogui adds a plugin's items at the end of the menu they
  name, after a separator.
- **Kind-level behaviour** goes through `leolib::ext::kind_at`. These are
  the read-only guard (`FileKind::read_only`), the rendered view
  (`FileKind::node_markdown`), and the rename message (`Rename::note`).
- **leo-entangled's `leoapp` cargo feature** builds the plugin. leo-plugins
  leaves leo-entangled out until it is published: crates.io refuses an
  unpublished dependency, even an optional one. `make ... ENTANGLED=1`
  builds and tests it.

**One plugin list per process (kept 2026-10-08).** `plugins::all()` is a `OnceLock`,
filled from the cargo features unless `plugins::register` ran first.
leolib's kinds are per outline, so a test can open a pure-Leo outline. App
plugins add commands, settings and menus, not file behaviour, and a binary
offers one set. A per-`App` list would also work. It would need the list
passed to `App::new` and to every lookup that has no `App` today:
`commands::find`, `minibuffer::completions` and `menus::entries`. Kept:
a test binary holds one set, and nothing needs different sets per window.
The hazard is order: the list is fixed on first use, so a lookup before
`register()` leaves the app silently without plugins. leotui and leogui
print to stderr if `register()` fails.

## Crates

```
leolib          Leo + leolib::ext
leo-markdown    scanner and writer + @qmd, @rmd       -> leolib
leo-entangled   @entangled (+ AppPlugin, feature)     -> leolib, leo-markdown
leo-wiki        @wiki, a TreeKind (+ AppPlugin, feature) -> leolib
leoapp          AppPlugin; no plugin crates
leotui, leogui  depend on the plugin crates they offer, and register them
```

**A tree kind (`@wiki`, 2026-10-08).** `leolib::ext::TreeKind` is a directive whose subtree is not a file: the `.leo` file stores it as any tree, where a `FileKind`'s children are never stored. It can fix its tree's language (`language_at` asks it first) and plan renames (`rename_block` asks it when no file kind does). leoapp's `AppPlugin` gained `open_url`, `complete` and `violations`; an edit that adds a violation is undone (`App::check_rules`), keyed on `Undoer::version` so nothing is checked while nothing is edited.

**Registration moves to the binaries (found in phase 2).** A plugin crate
that implements leoapp's `AppPlugin` depends on leoapp. leoapp then cannot
depend on the plugin crate to register it: Cargo refuses the cycle, even
through features. In phase 3, leotui and leogui register the plugins:
- leolib kinds through `Document::open_with`;
- app plugins through `leoapp::plugins::register`.

Decided 2026-10-08: a `leo-plugins` crate lists them once for both.

**`leo-markdown` contents:**
- the text layer: headings, fences, divs, front matter, and fence info
  by style;
- the tree layer: `build`, the writer, heading levels, and the ID map.

The text layer needs no `Outline`. leoapp's rendered view can use it in
place of its own `plain_fences`.

`include_text` moves to `leo-entangled`. The writer gets a fence's code
through a callback the kind supplies.

`@qmd` and `@rmd` stay in `leo-markdown` while they are mostly scanner
policy. Either moves to a crate of its own if it gains dependencies of
its own.

## Phases

All tests pass after each phase, with none edited.

1. **`leolib::ext` and the registry.** Done 2026-10-08.
   - `FileKind`, `Kinds` and `Rename` are in `leolib/src/ext.rs`.
     `Outline::kinds` holds the set; `open_outline_with_kinds` opens with
     a given one.
   - Until phase 3, the default set is `Kinds::builtin()`: `@entangled`
     and `@auto-cells`, still in leolib. Existing callers and tests are
     unchanged. Phase 3 makes the default empty.
   - `node::any_at_file_node_name` is Leo's directives only.
     `Outline::file_node_name` adds the registered kinds; the `Position`
     predicates, dirty marking and `language_at` use it.
   - Routed through `Kinds`: file collection, read order, read and write
     dispatch (`external.rs`); stored as external, `before_save`,
     `stores_body` (`leofile.rs`); rename (`document.rs`); nodes as
     documents (leolsp).
   - `entangled::BlockRename` is an alias of `ext::Rename`, so leoapp is
     unchanged until phase 2.
   - Tests: `crates/leolib/tests/plugins.rs`. With `Kinds::empty()`, both
     nodes keep their stored children and bodies, no file is read or
     written, and the `.leo` file saves them as ordinary nodes. A kind
     cannot claim Leo's directives.
2. **`leoapp::plugins`.** Route leoapp's call sites through it.
3. **Move into crates.** Done 2026-10-08.
   - `leo-markdown`: the scanner, writer, generic renaming (labels, adding
     one to an unnamed cell) and `AutoCells`; tests in
     `tests/auto_cells.rs`.
   - `leo-entangled`: `Entangled`, `include=` filling, and the rename of
     entangled's references across documents, which wraps
     `leo_markdown::plan_rename`. Its leoapp plugin is `src/app.rs`
     behind the `leoapp` feature. Tests and `tests/data/entangled/` moved
     here.
   - `leo-plugins`: `kinds()`, `app_plugins()` and `register()`. leotui and
     leogui call `register()` first. Its `tests/integration.rs` holds the
     tests that need the app and the real kinds: rename and read-only
     through keys, the rendered view, settings, and opening through
     `open_or_new`. The `@entangled` ones are now in leo-entangled's
     `tests/app.rs`, since leo-plugins no longer depends on it.
   - leolib opens with `Kinds::empty()` by default and has no
     `@entangled` or `@auto-cells` code or names. Its extension API gained
     `Outline::set_kinds`, `add_import_warning`,
     `keep_unsaved_after_read`, `ext::LANGUAGE` and `ext::kind_at`, and
     made `detach_subtree_keeping_clones` and `link_as_last_child_raw`
     public. `Document::rename_entangled_block` became `rename_block`.
   - leoapp has no plugin of its own. `plugins::register_kinds` and
     `plugins::kinds()` hold the leolib kinds it opens with: `open_or_new`
     reads with them, and `App::new` gives them to a document made with
     none. `config::parse` is public.
   - The leolsp test of nodes as documents uses a test kind: leolsp sits
     below every plugin crate.
   - Still in `leo-markdown`, though entangled's: `include_text` and the
     `INCLUDE` attribute, which the writer reads. Moving them needs a
     callback for a fence's code.
4. **Split `@auto-cells` into `@qmd` and `@rmd`.** Done 2026-10-08.
   - `leo_markdown::Cells` is one kind type; `QMD` and `RMD` are its two
     values, each with its directive and fence rules (Quarto, knitr),
     whatever the file's extension. `style_at` gives a root's rules for
     renaming: its kind's, or the extension's for `@entangled`.
   - `@auto-cells` was never released, so it is removed, and with it
     cells in a plain `.md` file (`@md` is deferred).
   - Before the split, `include=` moved to `leo-entangled`. The writer
     takes a `Code` callback for a fence's code; `@entangled` passes one
     that reads the included file, and marks its `include=` fences after
     the read with leo-markdown's public fence parser. The attribute is
     `leo-rs-entangled-include`, by the per-plugin prefix rule.
   - Tests: `leo-markdown/tests/cells.rs`, with a new test that the kind,
     not the extension, picks the rules.

## Risks

- **`leolib::ext` is a public API.**
  - A change to it breaks plugins, and leolib's version numbers then
    track it.
  - All plugins are in this workspace, so a change and its fixes land
    together.
- **With the `Outline` field, the open paths gain a parameter.** These
  are `Document::open`, `open_outline_with_report`, leomcp and leolsp.
  Each keeps a default form that takes the app's registered set, so
  callers outside the app change once.
