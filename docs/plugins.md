# Plugins

`@qmd` and `@rmd` are in `leo-markdown`, registered in leotui and leogui
by `leo-plugins`, from the release after 0.7.0. `@entangled` is in the
unpublished `leo-entangled`, and no binary registers it. The design is in
`docs/dev/plugins.md`. Its settings key, `entangled`, applies only when
the plugin is registered:

| Key | Meaning |
|-|-|
| `entangled` | the command `:entangled-tangle` and `:entangled-check` run; `entangled` on `PATH` when unset |

A registered `@entangled`, `@qmd` or `@rmd` heading also shows in leogui's
rendered view as its markdown, with the fences' code in place.

## `@entangled`

`@entangled PATH` reads a markdown file written in [entangled](https://github.com/shakfu/entangled-rs)'s literate syntax. A leo-rs kind: Leo has no equivalent, and opens such a node as plain.

````markdown
## Adding

```python #add file=hello.py
print(2 + 3)
```
````

becomes a heading node `Adding` whose body keeps the fence lines around a `<< add >>` reference, with a child node `<< add >>` holding `print(2 + 3)`.

- Headings are nodes. A fence entangled would name becomes a `<< name >>`: `#name` or `file=` in a `.md` file, knitr's `{python, label=..., file=...}` in `.Rmd`, Quarto's `#| label:` and `#| file:` in `.qmd`. Like entangled, a fence named in another style's way stays text, as do prose and unnamed fences.
- A fence node is in its fence's language (`python`, `py`, `{.rust}`), for colouring and the language servers, so an example gets completion and diagnostics.
- Saving writes the file back as it was, with edited code and headings in place. A file that would not read back exactly opens as one node, with an error. A heading or fence node deleted from the tree stops the write rather than drop text.
- The `.leo` file stores only the `@entangled` node; the tree is read from the markdown on every open.
- `include=lib.py` or `include=lib.py#name` fills a fence from a file you test on its own: all of it, or the lines between `ANCHOR: name` and `ANCHOR_END: name` comments. The file is the source, so the node is read-only, and the fence is refilled on every read and save; the path starts from the markdown file's directory.
- Moving a heading node to another depth changes its level by as much (`##` under `##` becomes `###`), its subheadings with it; a heading not moved is written as the file had it.
- A name is one block: a file whose fences share a name opens as one node with an error, though entangled would join them, as Leo refuses a section defined twice.
- Editing a fence node's headline renames the block, as one undo: its fence, and entangled's `<<name>>` and `<<doc.md#name>>` references in every `@entangled` document in the outline. Documents outside the outline are not checked: run `:entangled-check`.

`:entangled-tangle` writes the unsaved `@entangled` files, then runs `entangled tangle` in the outline's directory, so `file=` targets are written as entangled writes them; `:entangled-check` runs `entangled check`. Arguments pass through (`:entangled-tangle --force`), the output is in `:messages`, and leogui has both in the Body menu. Install entangled from [entangled-rs](https://github.com/shakfu/entangled-rs); set `entangled` in the settings to its full path if leogui, started from the desktop, does not find it on `PATH`. The design, and what comes next, is in `docs/dev/entangled_leo_backend.md`.

`demo/entangled/` shows the point of it: a README whose examples are tested. `tests.md` places each named example in a pytest function by reference (`<<README.md#count>>`); `make test` there tangles it and runs pytest, and an example that no longer holds fails its test. Open `demo.leo` to edit the README, the tests and the library in one outline.

## `@qmd` and `@rmd`

`@qmd PATH` reads a Quarto document and `@rmd PATH` an R Markdown (knitr) document, with their cells as nodes. The kind fixes the label rules, whatever the file's extension: Quarto's `#| label:` under `@qmd`, knitr's `{r setup}` or `label=` under `@rmd`. `@auto` and `@auto-md` stay Leo's markdown importer, which splits a plain `.md` file at its headings. These are leo-rs kinds, and Leo opens such a node as plain.

````markdown
## Load

```{python}
#| label: load
import pandas as pd
```

```{python}
df = pd.read_csv("x.csv")
```
````

becomes a heading node `Load` with two children, `<< load >>` and `<< python cell 2 >>`.

- Headings are nodes, and so are executable cells (`{python}`, `{r setup}`) and fences with a `#name` or label. Display fences (`python`, `{.python}`) stay in the prose. A heading inside a fenced div (a callout's title, a tabset's tabs) stays in the body, so the div's `:::` lines stay together.
- An unnamed cell is headlined by its language and its number among the document's cells. Renaming it gives it a label in its kind's syntax: `#| label: name` or `{r name}`. Renaming a labelled cell renames its label. Two cells with one label open as one node with an error, as knitr and Quarto refuse them.
- As in `@entangled`: the file is written back byte for byte, front matter stays in the root node, a cell node is in its cell's language, heading levels follow the tree, and the `.leo` file stores only the `@qmd` or `@rmd` node. `file=` and `include=` have no meaning here.

Working with one:

- **An existing file:** add a node headlined `@qmd report.qmd`, then `:refresh-from-disk`. It asks whether to discard the new node's edits; answer `y`, and the file's headings and cells become nodes.
- **A new file:** headline a node `@qmd new.qmd` and type the document in its body as markdown. A save writes the file; `:refresh-from-disk` then splits it into nodes.
- **A new cell:** type it in a heading's body as a fence, save, and `:refresh-from-disk`. A `<< name >>` node added by hand needs its fence lines and a `<< name >>` line in its parent's body, as the reader puts them; the save refuses one without.
- **Moving a cell** to another heading: move its two fence lines and its `<< name >>` line with it, or the save refuses the tree.
- **Renaming the root** to another path writes a new file there and leaves the old one.

Code-first literate programming: clone a labelled cell into an `@clean` tree. The same node is a fence in the markdown and code in the `.py` file, so an edit in either tree is saved to both, and `<<load>>` in a cell resolves as the `<< load >>` section. The `.leo` file keeps each labelled cell's gnx, so the clone survives a reopen; if both files changed on disk, the `@clean` file's text wins and the markdown's goes to Recovered Nodes. An unnamed cell is renumbered as cells are added, so label a cell before cloning it. This works for `@entangled` fence nodes too. The design is in `docs/dev/markdown_importer.md`.


## `@wiki`

A node headlined `@wiki NAME` is a wiki; its descendants are pages, written in markdown. `[[Page]]` in a page links to the page headed `Page`; `[[Parent/Page]]` narrows it by its parent; `[[Page|text]]` shows `text`; `[[other:Page]]` and `[[other:]]` reach another wiki. `\/`, `\|` and `\]` escape. Links in code are text.

- `gd` on a link follows it, and `Ctrl-o` comes back. `gd` also follows Leo's `gnx:GNX`, `unl:gnx://#GNX` and `unl://#Parent-->Page` links, anywhere.
- In INSERT, `[[` offers the wiki's pages.
- Renaming a page rewrites every link to it, from any wiki, as one undo step; renaming a wiki rewrites `[[old:...]]`.
- `:export-wiki` writes `NAME.md` beside the outline (or in the `@path` in effect): the root's body, then each page under a heading of its depth, links as `[text](#anchor)` with GitHub's anchors. A link naming no page or several, or a page deeper than six levels, refuses the export and says which.
- `:convert-wikilinks-to-unls` rewrites the wiki's links as Leo's `unl:gnx://` links, for an outline shared with Leo.
- The rules: no wiki inside another, no page a clone, no page headline starting with `@`, no directive in a page, and a name without `:`, `/` or `\`. An edit that breaks one is undone and the message names it; `:check-wiki` lists what an outline opened from elsewhere breaks.

The design is in `docs/dev/wiki.md`.
