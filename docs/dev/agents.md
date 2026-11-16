# Agents in an outline: options

Status: design record, 2026-11-16. Built, in `crates/leo-agent`: `:ask`, with the VISUAL selection but no typed ranges; the three `@agent` forms, run by `:agent-run`, leogui's double-click (`AppPlugin::activate`), and `Enter` on a body's `@agent` line. Also built: the MCP approach, for an agent whose command names `{mcp}` (section 5). Not built: typed ranges for `:ask`. It answers the `TODO.md` item "Explore how to use agents in a leo outline".

Terms:

- **Agent**: a program that takes a prompt and returns text, such as `claude -p`. It may also call tools.
- **One-shot**: one prompt in, one reply out, no tool calls.
- **Run**: one call to the agent, started by the user.
- **Scope**: the outline text sent with the prompt.
- **Reply**: the agent's output, as written into the outline.
- **Reply region**: the body text a run may change.

## 1. What exists

- **Background processes.** `crates/leo-entangled/src/app.rs` runs a command on a thread. `AppPlugin::poll` collects it, the output goes to `:messages`, and a `settings` key names the program. A one-shot run is this pattern with a different destination for stdout.
- **Link-following hook.** `AppPlugin::open_url` runs on `Enter` or `gd` in the body. `AppPlugin::links` marks byte ranges for the body to draw. leo-wiki uses both (`crates/leo-wiki/src/app.rs`).
- **MCP tools.** `crates/leoapp/src/app/mcp.rs` serves 11 tools over `leomcp`, gated by `mcp-edit` and `mcp-save`. An agent with the token can read and edit the outline today. leo-rs cannot start that agent (`scripting.md`, section 2).
- **Undo groups.** `Document::begin_group` and `end_group` make a run one undo step.
- **No node activation hook.** Double-click in leogui pins the tab and focuses the body (`crates/leogui/src/tree.rs:280`). leotui has no double-click.
- **Settings are flat.** `settings.toml` holds `key = value` lines, and a plugin declares a fixed list of keys it reads (`AppPlugin::settings`, `crates/leoapp/src/config.rs:231`). A key not on a list is reported as unknown.
- **The CLI.** `claude -p` prints a reply and exits. `--model` picks the model. `--output-format` takes `text`, `json` or `stream-json`. `--allowedTools`, `--permission-mode` and `--mcp-config` control tools. Checked against `claude --help` on 2026-11-16; see the [CLI reference](https://docs.claude.com/en/docs/claude-code/cli-reference).

## 2. Plugin boundary

Agents are a plugin, not core. The core gains no agent code: no `@agent` parsing, no agent settings, no process for it. It gains only generic extension points that any plugin could use.

- **Crate**: `leo-agent`, with its `AppPlugin` behind a `leoapp` cargo feature, as `leo-wiki` does.
- **Registration**: `leo-plugins` lists it behind an `agent` feature. The feature is off by default: a run costs money and sends outline text off the machine.
- **No leolib change**: `@agent` is neither a `FileKind` nor a `TreeKind`. Leo parity holds with or without the plugin.

What lives in `leo-agent`:

- the `:agent-run`, `:agent-cancel` and `:ask` commands, and their menu entries;
- `agents.toml`, its agent table (section 3);
- parsing the three forms and their parameters (section 4);
- building the scope text, and the size cap;
- the run thread, cancel and timeout;
- writing the reply as one undo group.

What it uses from the existing API:

| Need | Existing API |
|-|-|
| commands, menus | `AppPlugin::commands`, `with_argument`, `menu` |
| background run | `AppPlugin::poll`, `poll_after`, as `leo-entangled` |
| run from the body cursor | `AppPlugin::open_url`, `links` |
| run state | `App::plugin_data` |
| read the outline | `App::outline` |
| one undo step | `App::doc`: `Document::begin_group`, `end_group` |
| output | `App::message`, `App::log` |
| the settings directory | `leoapp::config::dir` |

Core changes, all generic:

| Change | Needed by | Why |
|-|-|-|
| `AppPlugin::activate(app, p) -> bool` (built) | double-click on an `@agent` node | leogui's double-click was hard-wired (`crates/leogui/src/tree.rs:280`) |
| a plugin confirmation prompt (built: `App::confirm`) | the prompts in section 7 | confirmations were a closed `MiniKind` enum (`crates/leoapp/src/app/prompt.rs`); a plugin could not add one |
| `App::mcp_tool_as`, `leoapp::app::mcp_tools`, `App::hold_new_files` (built) | the MCP approach (section 5) | the plugin serves the tools on its own server, with its own access |
| `Document::join_undo` (built) | the MCP approach | a run's edits arrive across frames, so one group cannot stay open around them |
| the ex range for a plugin command | typed ranges, `:3,5ask` (section 5) | `with_argument` receives the argument only; the VISUAL selection needs no change |

The three forms need only the confirmation prompt. Without it, the plugin can refuse a run and ask the user to repeat it with a `!`, as `:q!` does.

## 3. Choices every form makes

### Agents, providers and models

A run has three names:

| Name | Picks | Example |
|-|-|-|
| agent | the program | `claude`, `llm`, `ollama` |
| provider | the account the program bills: endpoint and key | `anthropic`, `openrouter`, `openai` |
| model | the model at that provider | `sonnet`, `anthropic/claude-sonnet-4.5` |

A user with keys for Anthropic, OpenAI and OpenRouter picks the provider per node. The outline names the three. The settings say what each name runs. The outline never holds a command, a flag or a key.

**Defaults, with no settings file:** agent `claude`, its logged-in account as the provider, and no `--model`, so `claude -p` uses its own default model. A user with `claude` on `PATH` needs no configuration.

**The table** lives in `~/.config/leo-rs/agents.toml`, which `leo-agent` reads itself. `settings.toml` cannot hold it: agent and provider names are the user's, and a plugin's keys are a fixed list. The lines are flat, as in `settings.toml`, and values split on blanks, as `entangled` does:

```
default = claude

claude = claude -p
claude.model = --model {model}

claude.openrouter.env = ANTHROPIC_BASE_URL=https://openrouter.ai/api ANTHROPIC_AUTH_TOKEN=${OPENROUTER_API_KEY} ANTHROPIC_API_KEY=
claude.openrouter.models = anthropic/claude-sonnet-4.5 anthropic/claude-opus-4.1

llm = llm
llm.model = -m {model}
llm.openrouter.model = -m openrouter/{model}
llm.openai.env = OPENAI_API_KEY=${OPENAI_API_KEY}
```

| Key | Meaning |
|-|-|
| `default` | the agent a node without `agent=` uses; `claude` when unset |
| `NAME` | the command: the prompt goes on stdin, the reply comes from stdout. `claude` defaults to `claude -p` |
| `NAME.model` | the words added when a model is given; `{model}` becomes the name |
| `NAME.default-model` | the model a node without `model=` uses; none when unset |
| `NAME.default-provider` | the provider a node without `provider=` uses; none when unset |
| `NAME.models` | the models a node may name; any when unset |
| `NAME.PROVIDER.env` | environment variables set for the run; `${VAR}` reads one of leo-rs's own |
| `NAME.PROVIDER.model`, `.models`, `.default-model` | as the agent's keys, for that provider |

Keys are per agent and provider because each program reaches a provider its own way:

- `claude` reaches OpenRouter through `ANTHROPIC_BASE_URL` and `ANTHROPIC_AUTH_TOKEN`, with `ANTHROPIC_API_KEY` empty ([OpenRouter: Claude Code integration](https://openrouter.ai/docs/guides/guides/claude-code-integration)). OpenRouter guarantees this only for Anthropic models.
- `llm` reaches it through the `llm-openrouter` plugin and an `openrouter/` model prefix ([llm-openrouter](https://github.com/simonw/llm-openrouter)).
- `claude` cannot reach OpenAI directly; it speaks Anthropic's protocol only.

Rules:

- A name with no entry refuses the run and lists the names: `agent=` with no `NAME`, `provider=` with no `NAME.PROVIDER.*` key.
- `model=` with no `.model` key for the agent or provider refuses the run.
- A model name holds only letters, digits and `.`, `_`, `-`, `:`, `/`, and does not start with `-`. It is one argument, never passed through a shell, so it cannot add a flag.
- `${VAR}` is expanded by the plugin, not a shell. Keys stay in the environment or the program's own key store (`llm keys set`), not in `agents.toml`. An unset `VAR` refuses the run and names it.
- leogui started from the desktop may not have the shell's environment. The message for an unset `VAR` says so, as `:entangled-tangle` does for `PATH`.
- The reply separator records agent, provider and model (section 4).
- Tests use an entry such as `echo = cat`.

The alternative to `agents.toml` is a generic core change: `AppPlugin::setting_prefixes`, so `settings.toml` accepts `agent.NAME = ...` lines. It keeps one settings file, at the cost of a core change for one plugin.

### Scope

What is sent with the prompt:

| Value | Sent |
|-|-|
| `0` (default) | the prompt only |
| `N` | the subtree of the node's N-th ancestor: `1` is the parent and its descendants |

`N` is the N-th ancestor's subtree, chosen 2026-11-16 over the N nearest ancestors' bodies alone: siblings are the usual context for a prompt. An `N` above the top-level ancestor stops there: only `all` sends the whole outline.
| `all` | the whole outline |

Shared rules:

- Each node goes as its headline, indented by depth, then its body.
- The node's own reply region is not sent. Earlier replies would otherwise feed every later run. A headline-form node goes as its headline alone, without its body or its children, which `reply=child` makes.
- Scope follows the current position. A clone has one parent per position, so its scope depends on where it was run from.
- A size cap refuses a run whose text exceeds it, naming the size. `scope=all` on a large outline otherwise sends megabytes.
- Scope text leaves the machine. A run shows its size and node count in `:messages`.

### Asynchrony

A run takes seconds to minutes. The node is held by gnx, as `mcp.rs` holds nodes, not by `Position`. Rules:

- The node is deleted during the run: the reply goes to `:messages`.
- The prompt changed during the run: the reply is written, and the message says the prompt changed.
- One run per node at a time. `:agent-cancel` kills the process. A timeout kills it too.

### Trigger

The primitive is a command, `:agent-run`, on the selected node. Menu entries, keys and double-click call it. It works in both front ends. Double-click works in leogui only, and it already means "pin and edit".

## 4. Syntax

One prompt per node. Three forms say where the prompt is:

| Form | `@agent` line in | Prompt | Reply region |
|-|-|-|-|
| headline | the headline | the headline after the parameters | the whole body |
| block | the body | the lines between `@agent` and `@agent-end` | the body after `@agent-end` |
| line | the body, with `prompt=headline` | the headline | the body after the `@agent` line |

### Headline form

```
headline:  @agent scope=1 top 5 sort algorithms
body:      (replies)
```

The default form, for prompts of one line. The node shows as an agent node in the tree, and its body holds only replies.

- **Trigger**: `:agent-run` on the selected node. Double-click needs the core `activate` hook.
- **Not in a wiki.** `@wiki` refuses a page headline that starts with `@`, and `App::check_rules` undoes the edit (`crates/leo-wiki/src/lib.rs:353`). The block and line forms work there: `@agent` is not a Leo directive, so the wiki's directive rule ignores it.

### Block form

```
headline:  Top sort algorithms
body:      @agent scope=1
           List the top five sort algorithms,
           with their average and worst-case cost.
           @agent-end
           (replies)
```

For prompts longer than a line.

- The headline is not sent. Nothing checks that it matches the prompt.
- Prose may come before the `@agent` line, and is kept. Text after `@agent-end` is the reply region.
- `@agent` with no `@agent-end` refuses the run, naming the line.
- **Trigger**: `:agent-run`, or `Enter` or `gd` on the `@agent` line through `open_url`.

### Line form

```
headline:  Top 5 sort algorithms
body:      @agent prompt=headline
           (replies)
```

Turns an existing headline into a prompt with one body line, and keeps the headline a title. It does what the headline form does, without the `@` in the tree.

- **Trigger**: as the block form.

### Parameters

`key=value` words. In the headline form they come first after `@agent`, and the prompt starts at the first word that is not a known `key=value`. In the body forms they fill the `@agent` line.

| Key | Values | Default |
|-|-|-|
| `reply` | `append`, `replace`, `child` | `append` |
| `scope` | `0`, `N`, `all` | `0` |
| `agent` | a name in `agents.toml` | `default`, else `claude` |
| `provider` | a provider of that agent | the agent's `default-provider`, else its logged-in account |
| `model` | a model the agent and provider allow | `default-model`, else the program's own |
| `prompt` | `body`, `headline`; body forms only | `body` |

An unknown key or value refuses the run and names it. So does `prompt=` in the headline form.

### Reply

| `reply` | A run |
|-|-|
| `append` (default) | adds a separator line, then the reply, at the end of the reply region |
| `replace` | replaces the reply region |
| `child` | writes the reply to a new last child; the body is unchanged |

- **Separator**: `-- 2026-11-16 14:03Z claude openrouter anthropic/claude-sonnet-4.5 --`: UTC to the minute, then the agent, provider and model as the run resolved them. UTC over local time: the standard library reads no time zone, and a time zone crate for one line is not worth its dependency; the `Z` says which. Without it, one reply runs into the next.
- **Child headline**: the same time, then the prompt's first non-blank line, cut at the last word boundary before 60 characters, with `...` added: `2026-11-16 14:03 List the top five sort algorithms,...`. The child's body starts with the separator line, so it records the agent, provider and model too. The time comes first, so the headline never starts with `@`: a prompt starting `@clean out.py` would otherwise make an `@clean` node that writes a file on save.
- **Undo**: each run is one undo group.
- **`replace` and edits**: the plugin keeps a hash of the last reply it wrote, in memory. A `replace` over a region edited since then refuses, naming the node. With no hash, after a reopen, it replaces; undo restores the text.
- **One prompt**: a second `@agent` line, or an `@agent` headline over an `@agent` body line, refuses the run and names the line.

## 5. Other approaches

### `:ask PROMPT`: reply at the cursor

```
:ask list the top five sort algorithms
:ask scope=1 model=opus summarise the parent node
V, select lines, then :ask reply=replace rewrite these lines as a table
```

An ex command, as vim's `:r !cmd`. Nothing is saved in the outline but the reply.

- **Prompt**: the argument after the parameters. It takes `scope`, `agent`, `provider` and `model` as in section 4.
- **Selection**: `:ask` from VISUAL sends the selected lines with the prompt, as `:'<,'>!cmd` does. The `:` line runs with the selection still set, as `:extract` reads it, so this needs no core change. Lines are whole, as vim's ex ranges are. Without a selection, only the scope is sent. `lines=all` acts on the whole body, as if selected, and wins over a selection. Typed ranges, `:3,5ask` and `:%ask`, are not parsed, and were skipped (2026-11-16): the ex parser passes a plugin command no range (`crates/leoapp/src/app/ex.rs:94`), and the selection and `lines=all` cover their use. Revisit when a second plugin command needs ranges.
- **Reply**: inserted as lines below the cursor's line, as `:r` does. A reply of several lines inside a line would split it. With a selection, it goes below its last line.
- **`reply`**: `insert` (default) or `replace`. `replace` needs a selection or `lines=all`, and replaces those lines, as `:[range]!cmd` in `ideas.md` does. `append` and `child` refuse: they place a reply by an `@agent` node.
- **No separator.** The reply is text the user asked for at that spot.
- **The cursor moves during the run.** The command records the node's gnx, the line, and the body's text. When the reply arrives:
  - body unchanged: the reply is inserted there, as one undo group;
  - body changed, or the node deleted: the reply goes to the unnamed register and `:messages`, and the message says `p` puts it. A line number in an edited body may name the wrong line.
- **Mode**: a reply that arrives in INSERT waits for NORMAL, as an MCP edit does, so it does not land inside typing.
- **Refused**: in the outline pane, and in a read-only node (`FileKind::read_only`).
- **No confirmation.** The user typed the prompt and the scope, so the prompts in section 7, which guard against a hostile outline, do not apply.
- **An empty body** becomes the reply, with no empty first line.
- **`--`** ends the parameters, for a prompt whose first word looks like one.
- **Effort**: 1 to 2 days over the runner. Typed ranges need a generic core change: the ex range passed to a plugin command.

### The agent edits the outline through leomcp

leo-rs starts the agent with `--mcp-config` pointing at its own `leomcp` server, and the prompt as its task. The agent reads and edits nodes through the 11 tools. Each edit is an undo step and is visible as it lands.

- **For**: multi-step work: restructure a subtree, split a node, write tests beside code. The three forms are its one-shot special cases.
- **Against**: see section 7. The `scripting.md` gaps apply: each MCP edit is its own undo step, and an edit waits for NORMAL mode.
- **Effort**: 3 to 5 days for the launch and token handoff. Grouping a run's edits into one undo step is the open design problem `scripting.md` lists under (d).

Built, 2026-11-16:

- **Trigger**: `{mcp}` in an agent's command in `agents.toml`. Every form runs such an agent. A new command or parameter was rejected: an outline must not grant tools, and `agent=` already names only agents the user defined.
- **Server**: one per run, on a free port of 127.0.0.1, with a new token, dropped when the run ends. The settings' server was rejected: it needs `mcp = true`, its token outlives the run, and `mcp-edit` and `mcp-save` apply to every client. A run may edit, with 9 of the 11 tools. `select_node` is not offered: it moves the user's selection and focus mid-run, and a run needs it for nothing. `save` is not offered, as a run never saves. `delete_node` is kept: `set_body` already erases text, so dropping it removes only restructuring, the reason to use this approach. A user who wants fewer tools has the agent's own flags, such as `claude --disallowedTools`; a per-agent `NAME.tools` key enforced by the server waits for an agent without such a flag. The descriptions drop "Needs mcp-edit", as a run reads no settings.
- **Handoff**: `{mcp}` becomes the path of a 0600 JSON file, created with `create_new` under an unguessable name and removed after the run. A JSON string on the command line was rejected: `ps` shows it to other users.
- **Undo**: each edit is its own step. `Document::join_undo` then joins it to the run's previous step, if the undo version has not changed between them. The reply joins the same way. A user's edit or undo during the run changes the version, so the steps split there. An undo group held open across frames was rejected: the user's edits would join it, and `check_rules` would undo the wrong step while it is open.
- **Rules**: each edit is checked as an MCP client's is, so a refused edit is undone alone. `hold_new_files` runs after each edit, so a later save asks before creating a file the agent named.
- **Typing**: an edit made while the user types is refused, as for any MCP client. Holding it until NORMAL was rejected: the agent's HTTP timeout could fire first, and the edit would land after the agent was told it failed.
- **Confirmation**: an `@agent` run of such an agent always asks, and names how many nodes it may read and edit. `:ask` does not ask, as the user typed it.
- **Reach**: the tools reach the subtree `scope` names, so the confirmation's count is what the agent can touch. `scope=0` is the node's own subtree; for an `@agent` headline node that is its replies. `Access::within` names the top node by vnode, as a position goes stale across edits; a clone's positions share their subtree, so any one serves. Insert or move beside the top node, and its deletion, are refused, as either would put a node outside. A rename whose kind plans edits outside is undone and refused. A clone with a copy outside the subtree is read-only: an edit to its headline, body or children is refused, since it would change the copy too. The check is per vnode, so a node under such a clone is read-only too. Copies all inside the subtree are editable. Refusing reads of such clones was rejected: their text is the same wherever they appear. With `scope=0`, a one-shot run sends no outline text, but an MCP run's agent may read the node's subtree.

### Fenced block (dropped)

An ` ```agent ` fence for the prompt and an ` ```agent-reply ` fence for the reply. It allowed several prompts per node, which the one-prompt rule rejects, and it puts markdown fences into code bodies. The block form covers its use.

## 6. Leo compatibility

- Leo opens `@agent` headlines and bodies as plain text. No form makes a `.leo` file unreadable in Leo.
- `@agent` and `@agent-end` are not in `GLOBAL_DIRECTIVES`, so `@file` writes them as text. A bare `@ ` end marker would open a doc part (`crates/leolib/src/atfile_write.rs:409`, a port of `at.directiveKind4`), and the reply would become a comment in the external file.
- In an `@file` or `@clean` body, the body forms write their prompt and replies to the external file. Leo does the same with any text. `reply=child` keeps replies out of the body. A headline-form node under `@others` writes its replies too, as any child's body.

## 7. Security

Attacker and goal as in `scripting.md`, section 4: whoever wrote a `.leo` file the user opens, aiming to act as the user.

`SECURITY.md` says "Nothing in a `.leo` file is executed". `scripting.md` rejects code-in-outline node kinds. An `@agent` prompt is code-in-outline once the agent may call tools.

| Agent | Risk from a hostile prompt |
|-|-|
| one-shot, no tools | the reply is text in the outline. Scope text goes to the agent's service, so a hostile prompt with `scope=all` sends the whole outline |
| tools allowed (`--allowedTools`, `--permission-mode`) | runs commands and edits files as the user |
| MCP approach | reads and edits the subtree its scope names, but not a clone with a copy outside it. It cannot save; a later save asks before creating a file it named |

Rules every form keeps:

- Nothing runs on open, on selection, or on save. Only `:agent-run` and its bindings run.
- Commands, flags and keys come from `agents.toml` and the environment, never from the outline. `agent=` and `provider=` pick names the user defined. `model=` is one checked argument.
- A hostile `provider=` can bill another of the user's accounts. It cannot reach a provider the user did not define.
- `.models` bounds cost: a hostile `model=` cannot pick a model the user did not list.
- One-shot is the default. Tool use is part of an agent's command in `agents.toml`, so only the user can add it.
- The first run in an outline, and any run with scope above `0`, asks first. It shows the size sent, the agent, the provider and the model. An outline is trusted, by file name, for the rest of the session once a run in it is confirmed. A plugin opens the y/n prompt with `App::confirm`, a generic core hook.

## 8. Summary

| | headline | block | line | `:ask` | MCP |
|-|-|-|-|-|-|
| Prompt length | one line | any | one line | any | any |
| Prompt saved | yes | yes | yes | no | yes |
| Core change | confirm; `activate` for double-click | confirm | confirm | confirm; ex range for `:3,5ask` | confirm; `mcp_tool_as`; `join_undo` |
| leotui | command | command, `Enter` | command, `Enter` | command | command |
| In a wiki | no | yes | yes | yes | yes |
| Text in external files | replies, under `@others` | prompt and replies | replies | no | depends |
| Effort, days, over the runner | 1-2 | 2 | 0.5 | 1-2 | 3-5+ |

The effort figures are estimates, from `leo-entangled/src/app.rs` and the `scripting.md` figures. None was measured.

## 9. Recommendation

1. The runner, with `:ask` as its first use: the agent table, scope, the thread, cancel, undo and the reply rules. 2 to 3 days, as `scripting.md` estimates `:!cmd`.
2. The headline form, the default.
3. The block form, for long prompts. The line form comes with it: the same parser, with the prompt taken from the headline.
4. The MCP approach. Built, with undo steps joined in place of an open group.

Alternative framing: an agent is a chat, not a filter. Then the outline holds a conversation, a node per turn, and scope is the conversation's ancestors. That fits the headline form with `reply=child`. It was not chosen: it needs session state between runs (`claude --resume`), which no form here keeps.

## 10. Open questions

- Should a body-form run in an `@file` or `@clean` body be refused unless `reply=child`?
- Should the agent see the scope as outline text, or as an `@clean`-style file with sentinels it can address?
- `agents.toml`, or a core `setting_prefixes` hook so the table lives in `settings.toml`?
- Do any outlines with `@agent` nodes get shared or opened from untrusted sources?
