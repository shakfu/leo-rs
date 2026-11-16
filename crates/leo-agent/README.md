# leo-agent

Agents for [leo-rs](https://github.com/shakfu/leo-rs), as a leoapp plugin. `:ask PROMPT` sends a prompt, and optionally outline text, to an agent such as `claude -p`, and inserts the reply below the cursor's line. `~/.config/leo-rs/agents.toml` names the agents, providers and models a prompt may pick. An agent whose command there names `{mcp}` may also edit the outline during its run, over an MCP server of its own.

leotui and leogui register it through `leo-plugins` when built with its `agent` feature. What it does: [docs/plugins.md](https://github.com/shakfu/leo-rs/blob/main/docs/plugins.md); the design: [docs/dev/agents.md](https://github.com/shakfu/leo-rs/blob/main/docs/dev/agents.md).

License: MIT.
