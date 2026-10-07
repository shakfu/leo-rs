# Security

## Threat model

A `.leo` file is as dangerous as a Makefile from the same source. Opening one and writing its external files can write anywhere the user can write: an `@<file>` headline or an `@path` directive may name an absolute path, climb out with `..`, or start with `~`. Leo behaves the same way.

The guards, and what they leave open, are in the README under "A `.leo` file names the paths it writes". In short:

- `Outline::may_overwrite` refuses a file the outline has not read. `@clean` and `@nosent` are exempt, as in Leo, so either can overwrite an existing file the outline never read.

- Nothing stops a new file in an existing directory.

A front end that opens untrusted outlines must check `Outline::full_path` against a directory of its own choosing before every write. leotui does not; it is meant for outlines you trust.

Reading is not meant to be dangerous. These are parsers of untrusted input, and a crash, hang or unbounded allocation in any of them is a bug worth reporting:

- the `.leo` XML reader (`leofile`);

- the `@file` sentinel reader (`atfile_read`);

- the pickle reader for unknown attributes (`pickle`);

- the `@auto` importers (`importers`).

`fuzz/` holds `cargo-fuzz` targets for the sentinel reader and the pickle reader. Nothing in a `.leo` file is executed: leo-rs has no scripting, so `@button` and `@command` nodes are text.

## Reporting

Report a vulnerability privately, with GitHub's "Report a vulnerability" on the repository's Security tab, rather than in a public issue. Include the input that triggers it if you can.

Only the latest release is fixed.
