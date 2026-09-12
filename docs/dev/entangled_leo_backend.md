# An @auto importer for entangled markdown

A sketch of 2026-09-12. Nothing here is scheduled. It is judgement unless a
line says it was measured.

## The proposal

Add `@auto-entangled`: an importer that reads a markdown document written for
[entangled](https://github.com/shakfu/entangled-rs), builds an outline whose
nodes are its headings and its fenced code blocks, and writes the document
back unchanged. leotui then edits entangled documents as outlines, and
`leolib` tangles them with the machinery `@file` already uses.

## Why it is plausible

entangled and leolib solve the same problem with the same primitive. Both
expand `<<name>>` references into generated files. entangled's delimiters are
`Markers{open, close}`, defaulting to `<<` and `>>`; leolib's are
`@section-delims`, defaulting to the same. `model/tangle.rs` and
`atfile_write.rs` are the same expansion with the same cycle check over
different containers.

So the code that turns a tree of named blocks into a source file already
exists here. What is missing is a reader that gets entangled's blocks into a
tree, and a writer that puts them back.

## What already exists

`@auto-md` reads markdown today. `importers/lines.rs::markdown` builds nodes
from `#` headings and from `===`/`---` underlines, and tracks fenced code
only to avoid reading a `#` inside a fence as a heading. Fence content stays
in the surrounding node's body.

Two properties of that importer block reuse:

1. Code blocks are not nodes. The outline sees prose structure, not the
   literate program.
2. `write_markdown` does not round-trip. `import_string` sets
   `round_trips = false` for every `LineImporter`, so the check in
   `importers::import_string` is skipped. The writer emits `#` headings
   whatever the file used, and drops `!Declarations` and placeholder nodes.

Property 2 is the one that must change. An entangled document is the user's
source of truth. A writer that reformats it loses work on the first save.

## The node mapping

One node per heading, as now, plus one child node per fenced code block that
carries an entangled attribute.

~~~text
# Hello                              -> node "Hello"
```python #main file=hello.py        -> child node "<<main>>", @language python
~~~

The child's headline is the block's reference name in section-reference form.
Its body is the fence content, verbatim. Directives carry the rest:

| entangled | node |
|-|-|
| `file=hello.py` | `@file hello.py` on a wrapper node, or `@path` plus headline |
| `#main` | headline `<<main>>` |
| language | `@language python` in the body |
| other attributes | `@entangled-attr k=v` lines in the body |

A block with a `file=` target becomes an `@file` node whose body is
`@others` over the blocks that build it. Leo then writes that file with its
own tangle, and the result is what entangled would have produced, provided
the sentinel question below is answered.

Alternative mapping, worth costing before choosing: leave code fences in
place and add nodes only for blocks with a `file=` target. Cheaper, and the
outline still lists the program's outputs, but `<<name>>` references stay
invisible to the tree, which removes most of the value.

## Round-tripping the document

The importer must satisfy the contract in `importers.rs`: the tree, written
back, reproduces the file it read. For fenced blocks that means recording
what the fence looked like, not just what it meant.

Per block, the writer needs:

- the fence characters and their count (``` vs ~~~~),
- the info string verbatim,
- the indentation of the fence,
- the blank lines around it.

Store them in the node body as directive lines the writer strips, the way
`@noheader` is handled in `write_markdown`, or in an unl-keyed side table on
the root. The first is simpler and survives a `.leo` save with no new file
format.

Once the writer is exact, set `round_trips = true` for this importer alone
and let `import_string` check it. That is the whole safety argument: a
document that fails the check falls into `parent.b` and is never written
back.

## Semantic mismatches

These are the parts that will not map, and the cost of each.

1. **Continuation blocks.** entangled concatenates every block with the same
   name; `ReferenceMap` keeps `Name -> Vec<ReferenceId>` for exactly that.
   Leo treats a second definition of a section name as an error. Either
   merge continuations into one node at import, and lose the document's
   block boundaries, or keep them as siblings and teach the writer to emit
   several fences from what Leo considers one section. Merging is lossy;
   keeping is a change to `atfile_write`'s duplicate handling.

2. **Reference scope.** entangled names are global across all documents in
   the project; a `ReferenceMap` is combined from many files. A Leo section
   reference resolves within the `@file` tree that uses it. A document that
   references a block defined in a sibling document has no leolib
   equivalent. Cross-document projects are out of scope, or need an
   `@auto-entangled` per file under one outline plus global resolution.

3. **Annotations vs sentinels.** entangled writes `~/~ begin <<f#name>>`
   into the tangled file so `stitch` can read it back. Leo writes its own
   sentinels for the same reason. If leolib tangles the file, the file
   carries Leo sentinels and `entangled stitch` no longer reads it. Three
   ways out, in increasing cost: use `@clean` and give up sentinel-based
   untangle; make the two tools agree on one marker format; or accept that
   a document is owned by one tool at a time.

4. **Targets.** entangled resolves `file=` relative to the project root.
   Leo resolves `@file` against `@path` and the outline's own directory.
   A `@path` on the `@auto-entangled` node covers the common case.

## Work

| piece | where | size |
|-|-|-|
| fence parser with verbatim capture | `importers/lines.rs` or a new module | medium |
| info-string parser for 4 entangled styles | new; port `entangled/src/style.rs` | small |
| node mapping and directive emission | new | medium |
| exact writer | `importers/lines.rs::write_markdown` sibling | medium |
| `LanguageSpec` entry and `spec_for` wiring | `importers.rs` | small |
| corpus cases | `demo/`, `crates/leolib/tests/corpus.rs` | small |

The four entangled fence styles (`entangled-rs`, `pandoc`, `quarto`,
`knitr`) are a table, not four parsers. Only the style actually used needs
to exist first.

## Open questions

- Which tool owns tangling? If leolib does, item 3 above must be settled
  before anything is written to disk. If entangled does, leo-rs is an editor
  over the document and never writes source files, which is a much smaller
  and safer feature.
- Is there a user? Leo has had markdown outlining for years without this.
  The case rests on someone who writes entangled documents and wants an
  outline view.

## The smaller version

If the answer to the first question is "entangled owns tangling", the
feature shrinks to: import, edit, export the markdown, and never touch a
tangled file. That drops items 1 through 4 to item 1 alone, and the
round-trip writer is still the bulk of the work. Start there.
