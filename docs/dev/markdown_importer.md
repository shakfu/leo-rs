# A markdown, R Markdown and Quarto importer

Status: M1 to M4 done 2026-10-07. On 2026-10-08 `@auto-cells` was split into `@qmd` and `@rmd`, which fix the fence rules by kind rather than by extension; cells in a plain `.md` file are no longer read (`docs/dev/plugins.md`, phase 4). Read `@auto-cells` below as those two. Since 2026-10-08 the code is in `crates/leo-markdown` (tests in its `tests/auto_cells.rs`); the paths below are where it was built (`docs/dev/plugins.md`).

## Goal

A new `@auto` variant, `@auto-cells PATH`, imports `.md`, `.markdown`,
`.Rmd` and `.qmd` files:
- headings as nodes;
- executable cells and named fences as child nodes, with their language set;
- the rest as body text.

Writing the file back reproduces it byte for byte, as `@entangled` does
(`entangled_leo_backend.md`).

A later phase adds code-first literate programming:
- a cell node can be cloned into an `@clean` tree;
- Leo writes the code file from that tree;
- the same node is written as a fence in the markdown.

Prose-first literate programming stays with `@entangled` and the entangled
CLI.

Readers: CI, agents and Quarto read these files, so the markdown stays the
file of record, and it must stay valid Quarto and R Markdown.

## Decisions (2026-10-07)

- **`@auto` is unchanged.** `@auto`, `@auto-md` and `@auto-markdown` keep
  Leo's importer, so existing outlines read as before and leo-rs stays
  Leo-like. The new importer is opt-in, as `@auto-cells`.
- **One variant; the extension picks the fence rules.** `.qmd` gets
  Quarto's, `.Rmd` knitr's, and anything else Markdown's, as in
  `@entangled`. (`@auto-md` was already taken by Leo's importer.)
- **In Leo,** `@auto-cells` is not an importer name. Leo shows the node
  with an empty body and does not touch the file, as it does for
  `@entangled`.
- **Executable cells and named fences become nodes.** Display fences stay
  in the prose.
  - Executable cells: `` ```{python} `` or `` ```{r label} ``.
  - Named fences: `#name`, `label=`, or `#| label:`.
  - Display fences: `` ```python ``, `` ```{.python} ``.
- **Code-first literate programming comes in a later phase** (M4).

## Today

`importers/lines.rs` ports Leo's markdown importer:
- It splits at `#` and underlined headings.
- Text before the first heading goes in a `!Declarations` node.
- Fences stay in bodies.
- Its writer always writes `#` headings, so it does not reproduce files
  (`round_trips: false`).
- `.Rmd` is read as plain markdown. `.qmd` has no importer.

`markdown.rs` (split from `entangled.rs` in M1) has most of what the new importer needs:
- a CommonMark scan for front matter, ATX and underlined headings, and
  fences;
- three fence styles (Markdown, knitr, Quarto);
- fence nodes headlined `<< name >>`, with the language set;
- an exact writer;
- heading levels that follow the tree;
- unique names;
- renaming;
- `node_markdown` for the rendered view.

## Design

**One scanner and writer, two policies.** The scanner and writer move from
`entangled.rs` into a shared `markdown.rs`. A policy says which fences
become nodes:

| Policy | Used by | Fences that become nodes |
|-|-|-|
| `Named` | `@entangled` | fences with a name or `file=`/`include=` target |
| `Cells` | `@auto-cells` | executable cells, and fences with a name |

Only `@entangled` reads `file=` and `include=`. Under `@auto-cells` they are
ordinary attributes.

**Headlines:**
- A named cell or fence is headlined `<< name >>`, as in `@entangled`.
  Leo's section matching (`node::match_headline`) ignores case and spaces,
  so `<<name>>` in code refers to it.
- An unnamed cell is headlined `<< python cell 3 >>`, numbered among the
  document's cells. The plan had no `<< >>`; the parent body's reference
  line must name the node, and one form keeps the writer to one rule. The
  name has spaces, which no label can have, so it never clashes with a
  label. Inserting a cell renumbers the later ones, so only labelled cells
  can be cloned (M4).
- knitr's positional label (`{r setup}`) names a cell under `@auto-cells`
  only: entangled reads `label=`, and `@entangled` follows it.
- Renaming an unnamed cell gives it a label in the file's own syntax:
  - `#| label: name` in `.qmd`;
  - `{r name}` in `.Rmd`;
  - `#name` in `.md`.
- A label used by two cells is refused on read and on rename. knitr and
  Quarto also reject duplicate labels.

**Quarto and R Markdown:**
- **Front matter** (`---` YAML) stays in the root node's body, as in
  `@entangled`. Leo's `!Declarations` node is not used.
- **`#| option` lines** stay in the cell's body, where Quarto reads them.
- **Fenced divs** (`:::`), such as callouts, are not split: a heading
  inside one stays in the body, because splitting there would put the
  `:::` lines in different nodes.
- **Inline code** (`` `{r} x` ``) and shortcodes
  (`{{< include _x.qmd >}}`) are text.
- **Language:** a cell's language sets the node's language, for colouring
  and LSP. `{ojs}`, `{mermaid}` and `{dot}` get Leo's language when it has
  one, and `plain` otherwise.

**Checks on read:** the importer's existing check, that the tree writes
back the text it came from, applies. `round_trips` becomes true for these
files. A file that does not reproduce is imported whole into the root
body, with an error, as `@entangled` does.

## Code-first literate programming (M4)

The aim: one cell node, headlined `<< load-data >>`, with two parents.
- Under `@auto-cells analysis.qmd`, it is written as a fence.
- Under `@clean analysis.py`, it is written as code, as a Leo section or
  under `@others`.
- An edit in either tree changes both files.
- leo-rs's clone-conflict report covers both files changing on disk.

**Prerequisite: stable identity.** `@auto-cells` and `@entangled` trees are
rebuilt on every read with new vnodes and new gnxs (`o.new_child_vnode`).
A clone into `@clean` would be lost the next time the outline is opened.
The proposal:
- the `@auto-cells` node stores a label-to-gnx map in its uA, which the `.leo`
  file saves;
- on read, a labelled cell reuses the vnode with that gnx if the outline
  has one;
- only labelled cells can be cloned. An unnamed cell has no stable key.

Alternative: store the markdown tree in the `.leo` file and update it from
disk, as `@clean` does. That keeps every node's identity, but the `.leo`
file then holds a copy of the markdown, and the update needs Leo's
`@clean` diff on top of the new reader.

Other points for M4:
- `<<name>>` in a cell's code resolves as a Leo section in the `@clean`
  tree.
- A namespaced reference such as `<<doc.md#name>>` does not resolve. Leo
  reports an undefined section and does not write the file.
- `#| option` lines are Python and R comments, so they are written into
  the `.py` file unchanged. Stripping them is an option to add if it is
  needed.

## Phases

Each phase ends with `make test` and `make lint` passing.

- **M1. Extract.** Done 2026-10-07. The scanner, the fence-info parser
  and the writer moved from `entangled.rs` to `markdown.rs`.
  - `entangled.rs` keeps what is entangled's own: reading and writing the
    file, `include=` filling and checks, renaming, and `node_markdown`.
  - `@entangled`'s 31 tests pass unchanged.
  - The policy is left to M2, where `Cells` first has a user; a policy
    with one variant would be dead code.
- **M2. `@auto-cells`.** Done 2026-10-07, with M3's labels.
  - `@auto-cells` is a kind of its own, added where `@entangled` is
    (`node.rs`, `position.rs`, `external.rs`, `leofile.rs`, leolsp). It is
    not in `AT_AUTO_NAMES`, so Leo's `@auto` path is untouched and the
    corpus does not change.
  - `markdown::read` does the shared read and checks, with the `Policy`;
    `@entangled` fills its `include=` fences after it. `node_markdown`,
    `is_fence_node` and `write_string` moved to `markdown`, which is now
    public, and cover both kinds.
  - Unnamed-cell headlines, adding a label on rename, and refusing
    duplicate labels came with it: without them a headline edit on a cell
    would have stopped the write.
  - An R fence, in either kind, is now `r`: Leo's extension table reads
    `.r` as REBOL.
  - Tests: `crates/leolib/tests/auto_cells.rs` (11), round trips over
    `tests/data/entangled/`, `demo/entangled/` and Quarto, R Markdown and
    markdown samples; a rendered-view test in leoapp.
- **M3. Fenced divs.** Done 2026-10-07. A heading inside a Pandoc div
  (`::: {.callout-note}`, `::: aside`) stays in the body; divs nest, a
  bare `:::` with no div open is text, and cells inside a div are still
  nodes. The scanner does this for `@entangled` too: the rule is about
  where a heading belongs, not about which fences are nodes.
- **M4. Literate.** Done 2026-10-07, with the label-to-gnx map, for both
  `@auto-cells` and `@entangled`.
  - Saving the `.leo` file writes the markdown root's `<t>` with no body
    and a `str_leo-rs-cell-ids` uA, `name gnx` a line. A read gives each
    named fence node that gnx's vnode, or a new vnode with that gnx.
  - The reader detaches the old tree without taking a clone's children
    away from its other parents (`Outline::detach_subtree_keeping_clones`).
  - `leo-rs-` attributes, which describe the file a node was read from,
    are never saved: on a cloned cell they would reach Leo as an
    unreadable pickle.
  - Markdown roots are read before other external files. A clone's
    conflict is found only when the second file read compares its text
    with the first's; read after the `@clean` file, the markdown would link
    its cell too late. The `@clean` file is read last, so its text wins and
    the markdown's goes to Recovered Nodes, as Leo does for any clone.
  - The writer refuses a fence node with children. A clone can gain them
    in the `@clean` tree, and the fence would leave their code out.
  - Tests: an edit in either tree reaching both files; a clone surviving
    reopen; a conflict when both files changed; `<<name>>` resolving as a
    section; a namespaced reference refused; a fence node with children
    refused.

## Risks

- **A Leo user opening the outline** sees `@auto-cells` nodes empty. The
  file on disk is safe, because Leo does not write a node it does not
  know as a file. The README must say the kind is leo-rs only.
- **Edge cases in the scanner.** Examples: HTML blocks, indented code, and
  nested fences of four or more backticks. `@entangled` covers some of
  these. M2 adds tests for each, and anything not handled falls back to
  the whole file in the root body.

## Open questions

- Should a cell's `#| echo: false` or `eval: false` show in the outline,
  for example as an icon in leogui?
