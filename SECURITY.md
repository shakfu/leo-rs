# Security

## Threat model

A `.leo` file is as dangerous as a Makefile from the same source. Opening one and writing its external files can write anywhere the user can write: an `@<file>` headline or an `@path` directive may name an absolute path, climb out with `..`, or start with `~`. Leo behaves the same way.

The guards, and what they leave open, are in the README under "A `.leo` file names the paths it writes". In short:

- `Outline::may_overwrite` refuses a file the outline has not read, `@clean` and `@nosent` included. A front end asks the user before overwriting it.

- A write refuses a directory that does not exist. Only `myLeoSettings.leo` can lift this; an outline's own `@settings` cannot.

- Nothing stops a new file in an existing directory, except when an MCP client asks for the save: then each new file waits for the user's approval.

A front end that opens untrusted outlines must check `Outline::full_path` against a directory of its own choosing before every write. leotui and leogui do not; they are meant for outlines you trust.

The MCP server (`leomcp`, off by default) is the only network-facing part. It listens on 127.0.0.1 only, refuses a request whose `Host` or `Origin` is not local, and needs a bearer token kept in a file only the user can read. A client reads the outline; it edits only with `mcp-edit` and saves only with `mcp-save` as well. A client is as trusted as the text it reads: a prompt-injected agent with both settings can change any node and ask to save it.

Reading is not meant to be dangerous. These are parsers of untrusted input, and a crash, hang or unbounded allocation in any of them is a bug worth reporting:

- the `.leo` XML reader (`leofile`);

- the `@file` sentinel reader (`atfile_read`);

- the pickle reader for unknown attributes (`pickle`);

- the `@auto` importers (`importers`);

- the markdown readers (`leo-markdown`, and `leo-entangled`, which also reads the files an `include=` names);

- the language-server protocol reader (`leolsp`), which reads what a server sends.

A read refuses nodes nested deeper than 1000 levels (`outline::MAX_DEPTH`).

`fuzz/` holds `cargo-fuzz` targets for the `.leo` reader, the sentinel reader, the pickle reader and the markdown reader. Nothing in a `.leo` file is executed: leo-rs has no scripting, so `@button` and `@command` nodes are text.

## Reporting

Report a vulnerability privately, with GitHub's "Report a vulnerability" on the repository's Security tab, rather than in a public issue. Include the input that triggers it if you can.

Only the latest release is fixed.
