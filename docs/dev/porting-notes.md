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

## Not ported

**`@auto`.** Its structure comes from one of the 34 importers under
`leo/plugins/importers`, dispatched through tables `LoadManager` builds. None
is ported. `read_file_at_position` returns an error for an `@auto` node rather
than guessing a structure, which would rewrite the user's tree.

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
