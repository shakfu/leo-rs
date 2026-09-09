# Porting notes

Where this port departs from `leo/leolib`, and why. Everything not listed here
follows Leo's code closely enough that the Python function name is the best
comment; the module docstrings name it where it matters.

## Structure

**Vnodes live in an arena, not behind pointers.** A vnode may have several
parents -- that is what a clone is -- so the tree is a DAG with cycles in the
parent direction. `Outline` owns `Vec<Vnode>` and everything refers to a node
by `VnodeId`. `VNode.context`, which pointed at the owning outline in Python,
is that arena. `Position` methods therefore take `&Outline`.

**An invalid position is `Option<Position>`.** Leo's `move_to_*` mutate `p`
and leave `p.v is None` on failure, which the caller must then test. Here they
return a new position or `None`, so a traversal that runs off the end cannot
be used by accident.

## Deliberate behaviour changes

**The reader detaches the whole subtree before rebuilding it.**
`FastAtRead.read_into_root` clears only the root's children
(`leoAtFile.py`, `read_into_root`). Re-reading an unchanged `@file` therefore
appends a second parent link to every node below it, and `isCloned` starts
answering true for all of them. `Outline::detach_subtree` removes the links
first, so every link the scan makes is fresh. The resulting tree is the same;
the parent lists are correct rather than accumulating.

**The write bit is recomputed on every save.** Leo sets `v.setWriteBit()` in
`put_v_element` and never clears it, so a node that has since moved out of an
`@file` tree keeps a stale answer. `put_v_elements` clears every write bit
first. For an outline read from a file -- where the bits start clear -- the
output is identical, which the corpus test checks.

**gnx timestamps are UTC.** Leo uses local time. Nothing compares a gnx to a
clock; the timestamp only has to advance and to be equal for two gnxs minted
in the same second, and UTC keeps that true across a DST change.

**Refusing to overwrite is unconditional.** `at.promptForDangerousWrite` asks
the user; with no view there is nobody to ask, so `Outline::may_overwrite`
refuses. `@nosent` and `@clean` are exempt, as in
`at.shouldPromptForDangerousWrite`.

**Writes are atomic.** `external::replace_file` writes a sibling temporary
file and renames it over the target. Leo writes in place after a backup.

## The `@auto` importers

`importers/block.rs` is Leo's `base_importer.py`: comments and strings are
blanked out to make **guide lines**, blocks are found in those, and the real
lines are edited to insert `@others`. Each language is a `LanguageSpec` -- a
table of patterns plus a choice of end-of-block rule -- which is how Leo's 34
importer modules reduce to one algorithm and a table.

Verified against Leo, file by file: 1,000 files across 8 languages produce
identical trees, headline for headline and body for body. Two do not, both by
choice; see the TypeScript entry below. `docs/dev/compare-importers.py`
reproduces the comparison.

Four deliberate departures:

**Every import is checked before it is kept.** An `@auto` file is regenerated
from its tree, so an importer that lost a line would overwrite the user's
source with something shorter. `read_one_at_auto_node` writes the tree back
and compares it with what the importer read; on a mismatch the whole file goes
into the node's body and the read reports an error. Leo does not check, and
two files in leo-editor itself fail this test -- one contains the literal text
`@others`, the other loses the trailing blanks of a whitespace-only line
through `move_blank_lines`. Both produce a wrong file in Leo.

**The `@verbatim` indent no longer leaks.** `at.putCodeLine` writes the
indentation of an `@verbatim` sentinel before writing the sentinel itself,
which `putSentinel` then suppresses when sentinels are off. Every line
resembling a sentinel in an `@auto` or `@nosent` file therefore gained its
indentation twice. Leo guards the case for `@clean` only (#2996); the guard
here is on `at.sentinels`, which covers all three.

**TypeScript headlines.** Leo's TypeScript table is `(group_number, pattern)`
pairs, but `find_blocks` reads the first element as the block's *kind*, so its
headlines come out as `1 class Config` and `2 async`. The name is taken from
the pattern's last group here, giving `class Config` and the function's own
name. Bodies are unaffected, so an `@auto` TypeScript file still round-trips.

**An empty file imports.** `ic.createOutline` returns `None` for an empty
file and `readOneAtAutoNode` then raises `AttributeError`. Here an empty file
is an empty node.

`@auto-rst` is not ported. Unlike the others it is not an importer: Leo falls
back to `c.rstCommands.writeAtAutoFile`, a separate mechanism.

## Not ported

**`@shadow`.** Deprecated in Leo.

**Unknown attributes stay opaque.** Leo pickles uA values; `Ua::Opaque` holds
the hexlified pickle exactly as the file spells it and writes it back
unchanged. A round trip is lossless. Two consequences:

- A uA's *value* is not readable from Rust, except for the `str_` and `json_`
  prefixes, which Leo stores as plain text.
- `descendentTnodeUnknownAttributes` and `descendentVnodeUnknownAttributes`
  are kept on the node that carried them and written back verbatim, rather
  than regenerated from the descendants' uAs as `fc.putDescendentVnodeUas`
  does. That is exact while the uAs and the subtree shape are untouched, and
  this crate changes neither. A caller that restructures an `@auto` tree
  should expect Leo to rebuild the blob on its next save.

**`.leojs`.** The JSON outline format.

## `@auto` writers

Leo has six writers under `leo/plugins/writers` and falls back to a
sentinel-free tangle for every other language. Only the four line-oriented
formats need one, because their readers consume the structure lines they see;
`importers/lines.rs` holds those four and the fallback is
`atfile_write::write_to_string` with `allow_undefined_refs`.

Leo's writers cannot run without a window at all: `BaseWriter.__init__` reads
`c.atFileCommands`, and `c` is `None` when leolib drives. So an `@auto-org`
node cannot be written by headless Leo, and can be here.

## Fold and mark state

Neither is stored in the `.leo` file. Leo keeps both in a sqlite cache under
`~/.leo/db`, keyed by file name. `state.rs` keeps them in a text file under
`~/.leo/leo-rs/`, one record per line, deliberately separate so nothing here
can corrupt Leo's cache. The two implementations do not share fold state.

## The `@clean` algorithm

`atclean.rs` is the one place where a diff must match Python exactly.
`propagate_changed_lines` interleaves the old file's sentinels with the
opcodes of `difflib.SequenceMatcher`; different opcodes put the sentinels in
different places and rebuild a different outline. `seqmatch.rs` is therefore a
port of `SequenceMatcher`, including its autojunk heuristic, not a wrapper
over an existing Rust diff crate.

## Byte offsets

Scanning uses byte offsets into `&str` throughout. Every index comes from
searching for an ASCII character -- a newline, a comment delimiter, an `@` --
so byte and character offsets agree wherever the code slices. Two places
needed an explicit guard, both marked in the source: `util::matches_at`
compares bytes rather than slicing, and `atfile_read::strip_indent` checks the
boundary before removing an `@others` indent.

## Config

There is no settings file. `outline::Config` spells out the code defaults Leo
falls back to when a setting is unset. One is worth watching:
`page_width` is Leo's code default of 132, while `leoSettings.leo` ships 80.
Nothing currently reachable from the writer depends on it -- all 381 files
round-trip byte for byte -- but check any setting that can reach a file before
adding to that surface.
