# Porting notes

Where this port departs from `leo/leolib`, and why. Everything not listed here follows Leo's code closely enough that the Python function name is the best comment; the module docstrings name it where it matters.

## Structure

**Vnodes live in an arena, not behind pointers.** A vnode may have several parents -- that is what a clone is -- so the tree is a DAG with cycles in the parent direction. `Outline` owns `Vec<Vnode>` and everything refers to a node by `VnodeId`. `VNode.context`, which pointed at the owning outline in Python, is that arena. `Position` methods therefore take `&Outline`.

**An invalid position is `Option<Position>`.** Leo's `move_to_*` mutate `p` and leave `p.v is None` on failure, which the caller must then test. Here they return a new position or `None`, so a traversal that runs off the end cannot be used by accident.

## Deliberate behaviour changes

**The reader detaches the whole subtree before rebuilding it.** `FastAtRead.read_into_root` clears only the root's children (`leoAtFile.py`, `read_into_root`). Re-reading an unchanged `@file` therefore appends a second parent link to every node below it, and `isCloned` starts answering true for all of them. `Outline::detach_subtree` removes the links first, so every link the scan makes is fresh. The resulting tree is the same; the parent lists are correct rather than accumulating.

**The write bit is recomputed on every save.** Leo sets `v.setWriteBit()` in `put_v_element` and never clears it, so a node that has since moved out of an `@file` tree keeps a stale answer. `put_v_elements` clears every write bit first. For an outline read from a file -- where the bits start clear -- the output is identical, which the corpus test checks.

**gnx timestamps are UTC.** Leo uses local time. Nothing compares a gnx to a clock; the timestamp only has to advance and to be equal for two gnxs minted in the same second, and UTC keeps that true across a DST change.

**Refusing to overwrite is unconditional.** `at.promptForDangerousWrite` asks the user; with no view there is nobody to ask, so `Outline::may_overwrite` refuses. `@nosent` and `@clean` are exempt, as in `at.shouldPromptForDangerousWrite`. A path counts as read only once its node holds the file, so a failed read leaves the file refused rather than exposed. `open_outline_with_report` returns those failures; `open_outline` drops them.

**UTF-8 only, and a file in any other encoding is left alone.** Leo carries an encoding per file: `at.readFileToUnicode` takes it from the BOM or the `-encoding=` field of the `@+leo` header (`leoAtFile.py`), directives supply it elsewhere, and `g.writeFile` encodes with it again. Python gets those codecs from its standard library; Rust's gives only UTF-8, and adding a decoder means a dependency. So `external::read_file_to_string` rejects bytes that are not UTF-8, and `external::encoding_is_supported` rejects an `@encoding` or header field that names anything else.

The `.leo` file itself is read the same way, in `leofile::read_leo_file`. Leo hands those bytes to an XML parser, which honours the encoding in the prolog; here a file that is not UTF-8 fails to open with `LeoFileError::NotUtf8` rather than opening with U+FFFD in its headlines. The prolog this writes always says utf-8, so the declaration and the bytes agree. That matches Leo, which writes with `leo_file_encoding`, a setting, not a property of the file it read (`leoFileCommands.py:1925`).

A rejected external file is reported in `ReadResult::errors` and never recorded as read, so `may_overwrite` refuses the write. Three further guards cover the paths that do not go through a read: `file_contents` checks the directive for `@nosent`, which is never read, and `@clean`, which is exempt from `may_overwrite`; `write_files` keeps a file that is not UTF-8 out of `WriteResult::refused`, since approving it would write UTF-8 over those bytes; and `replace_file` refuses to replace on-disk bytes it cannot decode. Leo, with its codecs, edits these files normally.

**Every fallible function answers with `leolib::Error`.** Leo reports through `g.error` and `g.es_exception` and returns a flag, which a front end cannot act on: `at.readFileAtPosition` prints and carries on. The variants here are the distinctions a caller acts on -- `NotFound`, `NotUtf8`, `UnsupportedEncoding`, `RefusedOverwrite`, `Import`, `Write` -- not the places that raise them. `external::FileReport` carries one whole, so a front end can prompt for a refused overwrite and stay silent about an encoding no prompt can fix.

**Writes are atomic.** `external::replace_file` writes a sibling temporary file and renames it over the target. Leo writes in place after a backup.

## The `@auto` importers

`importers/block.rs` is Leo's `base_importer.py`: comments and strings are blanked out to make **guide lines**, blocks are found in those, and the real lines are edited to insert `@others`. Each language is a `LanguageSpec` -- a table of patterns plus a choice of end-of-block rule -- which is how Leo's 34 importer modules reduce to one algorithm and a table.

Verified against Leo, file by file: 1,000 files across 8 languages produce identical trees, headline for headline and body for body. Two do not, both by choice; see the TypeScript entry below. `docs/dev/compare-importers.py` reproduces the comparison.

Four deliberate departures:

**Every import is checked before it is kept.** An `@auto` file is regenerated from its tree, so an importer that lost a line would overwrite the user's source with something shorter. `read_one_at_auto_node` writes the tree back and compares it with what the importer read; on a mismatch the whole file goes into the node's body and the read reports an error. Leo does not check, and two files in leo-editor itself fail this test -- one contains the literal text `@others`, the other loses the trailing blanks of a whitespace-only line through `move_blank_lines`. Both produce a wrong file in Leo.

**The `@verbatim` indent no longer leaks.** `at.putCodeLine` writes the indentation of an `@verbatim` sentinel before writing the sentinel itself, which `putSentinel` then suppresses when sentinels are off. Every line resembling a sentinel in an `@auto` or `@nosent` file therefore gained its indentation twice. The guard here is on `at.sentinels`, which covers `@auto`, `@nosent` and the `@clean` file itself.

**`@clean` gets `@verbatim` in the text it is read against.** Leo's #2996 left `@clean` out of `@verbatim` altogether. But reading an `@clean` file compares it against the tree written *with* sentinels, and without `@verbatim` there each line that only looks like a sentinel was taken for one, kept, and inserted again as text: reading such a file doubled those lines, even when it had not changed. This port dropped #2996. leo-editor replaced it with the `at.sentinels` guard above, which fixes the write it was protecting and keeps `@verbatim` for the read. The corpus case `demo/cases/sentinel_lookalikes` holds both.

**TypeScript headlines.** Leo's TypeScript table is `(group_number, pattern)` pairs, but `find_blocks` reads the first element as the block's *kind*, so its headlines come out as `1 class Config` and `2 async`. The name is taken from the pattern's last group here, giving `class Config` and the function's own name. Bodies are unaffected, so an `@auto` TypeScript file still round-trips.

**An empty file imports.** `ic.createOutline` returns `None` for an empty file and `readOneAtAutoNode` then raises `AttributeError`. Here an empty file is an empty node.

`@auto-rst` is not ported. Unlike the others it is not an importer: Leo falls back to `c.rstCommands.writeAtAutoFile`, a separate mechanism.

## Not ported

**`@shadow`.** Deprecated in Leo.

**Unknown attributes stay opaque.** Leo pickles uA values; `Ua::Opaque` holds the hexlified pickle exactly as the file spells it and writes it back unchanged. A round trip is lossless. Two consequences:

- A uA's *value* is not readable from Rust, except for the `str_` and `json_` prefixes, which Leo stores as plain text.

- `descendentTnodeUnknownAttributes` and `descendentVnodeUnknownAttributes` are kept on the node that carried them and written back verbatim, rather than regenerated from the descendants' uAs as `fc.putDescendentVnodeUas` does. That is exact while the uAs and the subtree shape are untouched, and this crate changes neither. A caller that restructures an `@auto` tree should expect Leo to rebuild the blob on its next save.

**`.leojs`.** The JSON outline format.

## `@auto` writers

Leo has six writers under `leo/plugins/writers` and falls back to a sentinel-free tangle for every other language. Only the four line-oriented formats need one, because their readers consume the structure lines they see; `importers/lines.rs` holds those four and the fallback is `atfile_write::write_to_string` with `allow_undefined_refs`.

Leo's writers could not run without a window: `BaseWriter.__init__` read `c.atFileCommands`, and `c` was `None` when leolib drove. leo-editor now builds them with the outline when no commander is acting, so both implementations write every `@auto` kind headless, and the corpus's `auto_languages` case checks the Markdown and Org files byte for byte on both sides.

## Fold and mark state

Neither is stored in the `.leo` file. Leo keeps both in a sqlite cache under `~/.leo/db`, keyed by file name. `state.rs` keeps them in a text file under `~/.leo/leo-rs/`, one record per line, deliberately separate so nothing here can corrupt Leo's cache. The two implementations do not share fold state.

## The `@clean` algorithm

`atclean.rs` is the one place where a diff must match Python exactly. `propagate_changed_lines` interleaves the old file's sentinels with the opcodes of `difflib.SequenceMatcher`; different opcodes put the sentinels in different places and rebuild a different outline. `seqmatch.rs` is therefore a port of `SequenceMatcher`, including its autojunk heuristic, not a wrapper over an existing Rust diff crate.

## Byte offsets

Scanning uses byte offsets into `&str` throughout. Every index comes from searching for an ASCII character -- a newline, a comment delimiter, an `@` -- so byte and character offsets agree wherever the code slices. Two places needed an explicit guard, both marked in the source: `util::matches_at` compares bytes rather than slicing, and `atfile_read::strip_indent` checks the boundary before removing an `@others` indent.

## Config

There is no settings file. `outline::Config` spells out the code defaults Leo falls back to when a setting is unset. One is worth watching: `page_width` is Leo's code default of 132, while `leoSettings.leo` ships 80. Nothing currently reachable from the writer depends on it -- all 381 files round-trip byte for byte -- but check any setting that can reach a file before adding to that surface.
