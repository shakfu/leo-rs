# Plugins

`@qmd` and `@rmd` are in `leo-markdown`, and `@wiki` in `leo-wiki`,
registered in leotui and leogui by `leo-plugins`, from 0.8.0. The agents
(`:ask`, `@agent`) are in `leo-agent`, from 0.10.0, registered only with
`leo-plugins`'s `agent` feature, which is off. `@entangled` is in the
unpublished `leo-entangled`, registered only by the unpublished `leo-all`'s
`leotui-all` and `leogui-all` (`make run PLUGINS=all`). The design is in
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
- `include=lib.py` or `include=lib.py#name` fills a fence from a file you test on its own: all of it, or the lines between `ANCHOR: name` and `ANCHOR_END: name` comments. The file is the source, so the node is read-only, and the fence is refilled on every read and save; the path starts from the markdown file's directory, and must stay in it: an absolute path, `..`, or a symlink leading out is refused, and the fence keeps its text.
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

`demo/qmd/` and `demo/rmd/` show both kinds, each with labelled cells cloned into an `@clean` file that runs.


## `@wiki`

A node headlined `@wiki NAME` is a wiki; its descendants are pages, written in markdown. `[[Page]]` in a page links to the page headed `Page`; `[[Parent/Page]]` narrows it by its parent; `[[Page|text]]` shows `text`; `[[other:Page]]` and `[[other:]]` reach another wiki. `\/`, `\|` and `\]` escape. Links in code are text.

- A link in a page is drawn underlined, in the theme's `markup.link.url` colour, else light blue in leotui and egui's link colour in leogui.
- `Enter` in NORMAL, with the cursor on a link, follows it; `gd` is the same. In leogui, a click on a link in the body or the rendered view follows it too. `Enter` and `gd` also follow Leo's `gnx:GNX`, `unl:gnx://#GNX` and `unl://#Parent-->Page` links, anywhere.
- `Ctrl-o`, in either pane, goes back to the node a link was followed from, however it was followed. `L` in the outline goes forward again.
- In INSERT, `[[` offers the wiki's pages.
- Renaming a page rewrites every link to it or through it (`[[Guide/Setup]]` when Guide is renamed), from any wiki, as one undo step, and names the links the new headline makes ambiguous; renaming a wiki rewrites `[[old:...]]`.
- `:export-wiki` writes `NAME.md` beside the outline (or in the `@path` in effect): the root's body, then each page under a heading of its depth, links as `[text](#anchor)` with GitHub's anchors. Its first line is an `<!-- written by :export-wiki ... -->` comment, and it replaces only a file that starts with one. A link naming no page or several, or a page deeper than six levels, refuses the export and says which.
- `:convert-wikilinks-to-unls` rewrites the wiki's links as Leo's `unl:gnx://` links, for an outline shared with Leo.
- The rules: no wiki inside another, no page a clone, no page headline starting with `@`, no directive in a page, and a name without `:`, `/` or `\`. An edit that breaks one is undone and the message names it; `:check-wiki` lists what an outline opened from elsewhere breaks.

`demo/wiki/` holds two wikis that link to each other. The design is in `docs/dev/wiki.md`.

## `:ask`

`:ask PROMPT` sends the prompt to an agent and inserts its reply below the body cursor's line, as vim's `:r !cmd`. It is in `leo-agent`. `leo-plugins` registers it only with its `agent` feature, as a run costs money and sends text off the machine: `cargo run -p leotui --features leo-plugins/agent`.

```
:ask list the top five sort algorithms
:ask scope=1 model=opus summarise this section
:ask -- scope=all is part of this prompt
```

From VISUAL, `:ask` sends the selected lines with the prompt, and the reply goes below them; `reply=replace` puts it in their place, as vim's `:'<,'>!cmd`. Lines are whole, whichever VISUAL selected them. `lines=all` acts on the whole body, as if selected, and wins over a selection: `:ask lines=all reply=replace fix the grammar`. Typed ranges, `:3,5ask`, are not read.

Parameters come before the prompt; `--` ends them.

| Parameter | Values | Default |
|-|-|-|
| `scope` | `0`: the prompt only; `N`: also the subtree of the node's N-th ancestor, `1` being the parent, refused past the top level; `all`: also the whole outline | `0` |
| `agent` | a name in `agents.toml` | `claude` |
| `provider` | a provider of that agent in `agents.toml` | the agent's own account |
| `model` | a model the agent allows | the agent's own |
| `lines` | `all`: the whole body, as if selected | the selection, if any |
| `reply` | `insert`: below the cursor's line, or the selection; `replace`: in place of the selected lines | `insert` |

- The prompt, and the scope's text, go to the agent's stdin; its stdout is the reply. The agent runs in the outline's directory.
- An empty body becomes the reply. Undo takes the reply out in one step.
- The reply waits while you type, and goes in once you are back in NORMAL.
- If the body changed during the run, the reply goes to the unnamed register instead: `p` puts it.
- `:agent-cancel` stops a run. A run past `timeout` seconds, 300 by default, is stopped.
- The prompt and its scope may be at most 256 KB.

With no settings, `:ask` runs `claude -p`, and `model=` adds `--model NAME`. `~/.config/leo-rs/agents.toml` names other agents, providers and models. Each `:ask` reads it again if it changed since the last. Commands split on blanks; quotes do not group words.

```
default = claude
timeout = 300

claude.models = sonnet opus haiku
claude.openrouter.env = ANTHROPIC_BASE_URL=https://openrouter.ai/api ANTHROPIC_AUTH_TOKEN=${OPENROUTER_API_KEY} ANTHROPIC_API_KEY=

llm = llm
llm.model = -m {model}
llm.openrouter.model = -m openrouter/{model}
```

| Key | Meaning |
|-|-|
| `default` | the agent `:ask` uses without `agent=` |
| `timeout` | seconds a run may take |
| `stamp` | `utc` (the default) or `local`: the time zone of an `@agent` reply's separator |
| `confirm-reply` | which replies ask y/n before they go in: `never` (the default), `replace` (a reply that replaces text) or `always`. On n, the reply goes to the unnamed register |
| `NAME` | the agent's command |
| `NAME.model` | the words `model=` adds; `{model}` is the name |
| `NAME.default-model` | the model used without `model=` |
| `NAME.models` | the models `model=` may name |
| `NAME.default-provider` | the provider used without `provider=` |
| `NAME.confirm-reply` | as `confirm-reply`, for this agent |
| `NAME.timeout` | as `timeout`, for this agent |
| `NAME.reply` | an `@agent` node's `reply` when it names none: `append`, `replace` or `child`. `:ask` keeps `insert` |
| `NAME.preamble` | a line sent before every prompt, as `Reply in plain text.`; it counts towards the 256 KB |
| `NAME.PROVIDER.env` | `VAR=value` words set for the run; `${VAR}` reads leo-rs's environment |
| `NAME.PROVIDER.model`, `.default-model`, `.models` | as the agent's, for that provider |

A model name holds only letters, digits and `. _ - : /`, and does not start with `-`, so it cannot add a flag. Keep keys in the environment, not in the file. leogui started from the desktop may not have the shell's environment; an unset `${VAR}` refuses the run and names it. The design is in `docs/dev/agents.md`.

## `@agent` nodes

A node headlined `@agent [params] PROMPT` holds a prompt, and its body holds the replies. `:agent-run` runs the selected one, from the outline or the body; in leogui, so does a double-click on its row. It is in `leo-agent`, with `:ask`.

```
@agent top 5 sort algorithms
@agent scope=1 reply=child compare the nodes beside this one
@agent agent=llm provider=openrouter model=anthropic/claude-sonnet-4.5 hi
```

The parameters are `:ask`'s, with `reply` taking other values:

| `reply` | A run |
|-|-|
| `append` (default) | adds a separator line, then the reply, at the end of the body |
| `replace` | replaces the body with the separator and the reply |
| `child` | adds a last child holding the separator and the reply; the body is unchanged |

- The separator records when and what ran: `-- 2026-11-16 14:03Z claude opus --`. The time is UTC, or local with its offset under `stamp = local`: `16:03+0200`.
- A child's headline is the time and the prompt's first line, cut at 60 characters.
- `scope` sends the node as its headline alone, never its body or children: earlier replies would feed every later run.
- The first run in an outline asks first, as does every run with a scope: a shared outline's prompt could send the outline to one of your accounts. The question names the size, the agent, the provider and the model. Once you answer y, prompt-only runs in that outline stop asking until you quit.
- `reply=replace` refuses if you edited the body since its last reply.
- A body edited during a `replace` run, or a node deleted during any run, gets the reply in the unnamed register instead.
- Each reply is one undo step. The reply waits while you type, as `:ask`'s does.
- `:agent-cancel` stops a run.
- A wiki page cannot be an `@agent` node: `@wiki` refuses a page headline that starts with `@`. A body form works there.

The prompt can be in the body instead, so the headline stays a title:

```
headline:  Sorting notes
body:      Merge sort is stable; heapsort is not.
           @agent reply=replace
           Say which of these sorts are stable.
           @agent-end
           (replies)

headline:  Top 5 sort algorithms
body:      @agent prompt=headline
           (replies)
```

- A block is an `@agent [params]` line, the prompt on the lines below it, and an `@agent-end` line. Text above the block stays; the replies follow it, and `replace` replaces only them.
- `@agent prompt=headline` takes the headline as the prompt, and the replies follow the line.
- An `@agent` line starts its line, as a directive does, and holds parameters only. It is drawn as a link: `Enter` or `gd` on it runs the node.
- A node holds one prompt: a second `@agent` line, or an `@agent` headline over an `@agent` line, refuses the run and names the lines.
- In an `@file` or `@clean` body, the block and its replies are written to the external file, as any text is.

## Agents that edit the outline

An agent whose command in `agents.toml` names `{mcp}` may read and edit the outline during its run. `:ask agent=edit` and `@agent agent=edit` run it:

```
edit = claude -p --strict-mcp-config --mcp-config {mcp} --allowedTools mcp__leo
```

- Each run starts an MCP server of its own on 127.0.0.1, with a new token, and stops it when the run ends. `{mcp}` becomes the path of a config file naming that server. The file is readable by you alone, and is removed after the run. The settings' `mcp` keys play no part.
- The agent gets 9 of the 11 tools the settings' server offers (the README's MCP section): not `select_node`, which would move your selection during the run, and not `save`. The prompt names the gnx of its node.
- To withhold more, use the agent's own flags: `claude --disallowedTools mcp__leo__delete_node`.
- The tools reach only the subtree `scope` names: the node itself for `scope=0`, its N-th ancestor's subtree for `scope=N`, the whole outline for `scope=all`. A node outside is not found, the selection outside reads as none, and nothing may be inserted or moved beside the subtree's top node, or delete it. A rename whose kind would rewrite nodes outside, as a wiki rename rewrites links, is refused.
- A clone with a copy outside the subtree may be read but not changed: its headline, body and children are refused, as an edit would change the copy too. Unlinking it from the subtree is allowed, as the copy outside stays.
- A run's edits, and its reply, are one undo step. An edit or undo of yours during the run splits it there.
- `confirm-reply` asks about the reply alone: the agent's edits are made during the run.
- An edit made while you type is refused, and the agent is told to try again.
- A save after the run asks before creating any file an edit named, as an MCP client's save does.
- An `@agent` run of such an agent asks first every time. The question says how many nodes it may read and edit.
- Only `agents.toml` grants the access: an outline cannot add `{mcp}`, or the tools the agent may call.
