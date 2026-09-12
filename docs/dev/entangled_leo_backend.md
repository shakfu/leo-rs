# An outline view of an entangled document

A sketch of 2026-09-12. Nothing here is scheduled. It is judgement unless a
line says it was measured.

## The proposal

Show an [entangled](https://github.com/shakfu/entangled-rs) markdown document
as a Leo outline in leotui, and write edits back to the markdown. Split the
work three ways:

| layer | owner | why |
|-|-|-|
| structure: nodes, levels, order, identity | new code in `leolib` | entangled has no document model |
| semantics: block names, targets, references, cycles | the `entangled` crate | `leolib` would be reimplementing it |
| writeback: edited node to edited file | the `entangled` crate's span splice | already exact |

Neither `atfile_write` nor `atfile_read` is used. leolib never writes a
tangled file in this mode, so Leo sentinels never meet entangled
annotations.

## Why the split falls this way

### The semantics already match

`atfile_write.rs:597`: a node headlined `<<name>>` is skipped by `@others`
and written where it is referenced. entangled resolves `<<name>>` through
`ReferenceMap.name_index` and expands it at the reference site. Same rule,
same default delimiters, same cycle check. There is nothing to port.

### The document model does not exist

`readers/markdown.rs:18`:

~~~rust
pub struct ParsedDocument {
    pub refs: ReferenceMap,
    pub frontmatter: Option<String>,
    pub source_path: Option<PathBuf>,
}
~~~

Code blocks, frontmatter, path. No headings, no prose, no order beyond
`IndexMap` insertion. `BlockLocation` gives each block's first and last
content line; the lines between blocks are prose the model never sees.

An outline of a literate document is mostly prose nodes. So the view cannot
be built from `ReferenceMap`, and `leolib` needs its own markdown parse.

### The writeback is already exact

`stitch_files` (`interface/document.rs:320`) does not regenerate markdown.
It collects `(content_start, content_end, replacement)` per block and splices
those line ranges into the file it read. Everything outside an edited range
is untouched by construction. That is the property an importer writer would
otherwise have to earn, and it is already shipped.

## The structure parse

A line scanner over one markdown file, in `leolib`. Close to
`importers/lines.rs::markdown`, with three additions.

Nodes:

- one per `#` heading and per `===`/`---` underlined heading, nested by level;
- one per fenced code block, as a child of the enclosing heading;
- prose runs stay in the enclosing heading's body.

Each node records the line range it came from. A heading node's body carries
`@others` at each point where its child fences sat. `visited`
(`atfile_write.rs:92`) already lets several `@others` share one body, each
claiming the not-yet-written descendants, so prose/fence/prose/fence is
expressible.

Headlines: a fence node's headline is `<<name>>` when the fence names a
block, and the target path when it names one. An unnamed fence gets its
language and an ordinal.

## Identity

`ReferenceId{name, count}` (`model/reference_id.rs:11`) counts instances
among same-named blocks. It is positional: inserting a block named `main`
above an existing one renumbers every later `main[n]`. Cursor, marks,
expand state, undo and clone identity in leotui are all gnx-keyed, so a
positional id is not usable as a gnx.

An unnamed fence has no `ReferenceId` at all, and prose has none either.

Proposal: allocate gnx at parse time and keep it for the session. The
outline is authoritative while leotui holds it, and edits go to both the
outline and the text, so nothing reparses during normal editing. A reparse
happens only when the file changes on disk. Reconcile then by structural
match -- same heading chain and ordinal -- and allocate fresh gnx for what
does not match.

Consequence: gnx is not durable. Marks and expand state are session state in
this mode, which is what a file with no `.leo` can offer.

## Joining the two parses

Parse the file twice: once for structure, once with
`entangled::readers::markdown::parse_markdown`. Join on
`(source_path, content_start)`, since `BlockLocation` is 1-indexed line
numbers and the structure parse is line-based too.

The join is asymmetric, which is what makes it decidable: every entangled
block must join to exactly one fence node, and a fence node with no block is
a plain fence. A block that does not join is a disagreement between the two
parses; reject the document rather than show a view that tangles differently
from the CLI.

Parsing twice rather than teaching `leolib` the info-string syntax costs one
extra pass over one file. `importers/lines.rs::markdown` already tracks
`in_code`, because a `#` inside a fence is not a heading, so the structure
parse finds every fence whatever else it does. Only interpreting the info
string is avoided, and that is four styles (`style.rs`) that will keep
changing. Headlines fill in after the join, so `leolib` never reads one.

## Writeback

A node body edit becomes a splice of that node's line range, by the same
route `stitch_files` uses. Nodes are held in document order, so after a
splice every later node's range shifts by the line delta.

Prose nodes splice the same way. That is the reason prose gets nodes at all
rather than being carried opaquely.

## Where this attaches to Document

Measured on 2026-09-12: leotui reaches `Outline` directly 101 times --
`p.vis_next(app.outline())`, `app.doc.outline.promote(&p)`, expand, contract,
navigation, rendering. It calls `Document`'s mutations at 22 sites over 21
methods.

So a backend trait would have to cover 101 accesses, or hand out an
`&Outline` and cover only the 22. The second is no abstraction: the
markdown backend has to build a real `Outline` either way, which the
structure parse does. There is no second model here, only a second origin
and a second writeback path.

That makes the change one field:

~~~rust
pub struct Document {
    pub outline: Outline,
    pub undoer: Undoer,
    pub read_report: ReadResult,
    clipboard: Option<VnodeId>,
    origin: Origin,   // Leo { path } | Markdown { path, ranges }
}
~~~

`save` (`document.rs:374`) dispatches on `origin`. `write_external_files`
does nothing for `Markdown`. The commands below check it. leotui's 101
outline accesses are untouched.

Revisit a trait when a third origin exists. Nothing suggests one.

## What a markdown origin does not support

| command | why |
|-|-|
| `clone_node` | entangled has no second position for a block |
| `move_left`, `move_right` | a markdown heading level change is a different edit from a tree move |
| save as `.leo` | there is no outline file; the document is the file |
| `write_external_files` | `entangled tangle` writes them |

`insert_node`, `delete_node`, `move_up` and `move_down` are expressible as
text splices, but each needs its markdown meaning defined first. Defer them.

## Continuation blocks

entangled concatenates `name[0..n]`; `ReferenceMap` keeps
`Name -> Vec<ReferenceId>` for it. Leo treats a second definition of a
section name as an error.

The view can show n sibling nodes headlined `<<name>>`, because the view
never tangles -- entangled does, from its own map, where n blocks are
correct. The Leo duplicate rule only binds `atfile_write`, which is not in
this path.

So this stops being a semantic problem and becomes a display one: n nodes
with the same headline. Number them in the view if that reads badly.

## Plan

Three stages. The first is worth doing whether or not the rest is.

### 1. The structure parse and an exact markdown writer

In `leolib`. No new dependency, no new crate, no entangled involvement.

`import_string` sets `round_trips = false` for every `LineImporter`, so the
check it performs for other importers is skipped for markdown.
`write_markdown` earns that: it emits `#` headings whatever the file used,
drops `!Declarations`, and drops placeholder nodes. Import a markdown file
and write it back and it is reformatted.

A parse that records each node's line range, and a writer that splices those
ranges, makes `round_trips = true` possible for markdown. That is a `leolib`
correctness fix on its own terms.

Deliverable: fences are nodes, prose is nodes, the file round-trips, and the
corpus checks it.

### 2. Look at a real document

Open an entangled document in leotui with stage 1 and nothing else. The
outline will be mostly prose nodes. Whether that reads as a useful view of a
literate program is the one question this design cannot answer on paper.

Cost: an afternoon. It gates stage 3 entirely.

### 3. leo-entangled

Only if stage 2 reads well.

~~~text
crates/leolib           structure parse, from stage 1
crates/leo-entangled    join, tangle, writeback. Depends on leolib + entangled.
crates/leotui           optional dependency on leo-entangled
~~~

`leolib` has three runtime dependencies. Adding `entangled` to it for a
feature most callers will not use is the wrong trade, so the join lives
outside it.

| piece | where | size |
|-|-|-|
| structure parse with line ranges | stage 1, `leolib` | medium |
| gnx allocation and reparse reconcile | stage 1, `leolib` | medium |
| exact writer and corpus cases | stage 1, `leolib` | medium |
| `Origin` field and its guards | stage 3, `leolib` + `leotui` | small |
| span join and its rejection check | stage 3, `leo-entangled` | small |
| splice writeback and range shifting | stage 3, `leo-entangled` | small |

Stage 1 is most of the work and none of the speculation.

## The remaining question

Is there a user? Leo has had markdown outlining for years without this, and
`comparison.md` puts leo-rs's next work at the `leolib` API rather than more
TUI commands. This feature is option 3 there, a different product on Leo's
formats.

Stage 1 does not depend on the answer. Stages 2 and 3 do.
