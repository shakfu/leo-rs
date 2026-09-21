# `@wiki`: wikilinks in a markdown subtree

Status: **proposed**. Nothing here is implemented. Leo has no equivalent; `@wiki` is a leo-rs extension.

## 0. Summary

A node whose headline is `@wiki <name>` is a *wiki root*. Its descendants are *pages*. Page bodies are markdown, and `[[...]]` in them is a link to another page. `export-wiki` writes the subtree to `<name>.md` and turns each link into a markdown link.

Wikilinks are live only inside a wiki. Links from anywhere else use Leo's own syntax (section 3), which Leo can also follow.

---

## 1. Terms

- **Wiki root**: a node with headline `@wiki <name>`.

- **Page**: any descendant of a wiki root.

- **Wiki**: the root and its pages.

- **Name**: the `<name>` in the root headline. It is the export filename stem and the cross-link namespace.

---

## 2. Link syntax

| Link | Target |
|-|-|
| `[[Page]]` | the page headed `Page` in this wiki |
| `[[Parent/Page]]` | the page headed `Page` whose parent is headed `Parent` |
| `[[A\/B]]` | the page headed `A/B` |
| `[[Page\|text]]` | `Page`, displayed and exported as `text` |
| `[[other:Page]]` | the page headed `Page` in the wiki named `other` |
| `[[other:]]` | the root of the wiki named `other` |

(`\|` escapes the pipe for this table; in a body it is a plain `|`.)

Grammar:

```
link    = "[[" [ name ":" ] [ path ] [ "|" label ] "]]"
path    = segment *( "/" segment )
segment = 1*( char / "\/" / "\|" / "\]" / "\\" )
```

Matching follows Leo's `g.findUnl` (`leo/core/leoGlobals.py:5150`):

- Headlines compare after stripping surrounding whitespace.

- The path matches a *suffix* of the page's ancestor chain. `[[Page]]` matches any page headed `Page`; `[[Parent/Page]]` narrows it.

- The ancestor chain stops at the wiki root. The root headline is never a segment.

Links are recognised anywhere in a page body, except inside fenced code blocks and inline code spans.

---

## 3. Resolution

| Matches | Editor | Export |
|-|-|-|
| 1 | follow | emit a link |
| 0 | draw as broken | refuse |
| 2 or more | pick from a minibuffer list | refuse |

Leo resolves duplicates silently, taking the last match in outline order (`leoGlobals.py:5213`). leo-rs does not, because a silent choice makes a link point somewhere its author did not see.

### Links from outside a wiki

`[[...]]` is not parsed outside a wiki, so `[[` in code needs no special case. A code comment reaches a page with a Leo link:

- `unl:gnx://#<gnx>`

- `unl://#Parent-->Page`

`follow-link` handles these, and `gnx:<gnx>` and `<< section >>`, in every body, as Leo's `openUrlHelper` does (`leoGlobals.py:5457`). A UNL has no closing delimiter: `unl_regex` runs to the end of the line or the next quote or backtick (`leo/leolib/util.py:506`). Write it last on its line or in backticks.

---

## 4. Constraints

| # | Rule | Reason |
|-|-|-|
| C1 | No `@wiki` below another `@wiki`. | One wiki per page keeps resolution and export unambiguous. |
| C2 | No page is a clone: every vnode in a wiki has one parent. | A clone has several ancestor chains, so its links and anchors have several meanings. |
| C3 | No page headline starts with `@`. | The file readers and writers act on `@file`, `@clean` and `@auto` wherever they appear. An `@file` page would still write its external file on save. |
| C4 | No body directives in pages. Use fenced blocks, not `@language`. | Markdown has its own code syntax. Directive lines would be exported as text or silently dropped. |
| C5 | Names are unique in the outline and contain no `:`, `/`, `\` or control characters. | A name is a namespace and a filename stem. |

Pages are markdown: `language_at` returns `md` for every page, whatever its ancestors declare.

### Enforcement

One function, `wiki::check(outline) -> Vec<Violation>`, is called from three places:

| When | Action |
|-|-|
| Load | Warn with the list. Leo does not know these rules, so an outline edited in Leo can break them. |
| Edit | Refuse any operation that would add a violation: clone, paste retaining clones, move, promote, demote, a headline edit, a body edit adding a directive. |
| Export | Refuse, and list every violation. |

---

## 5. Editor

| Key | Pane | Command |
|-|-|-|
| `gf` | body | `follow-link` |
| `Ctrl-o` | both | `jump-back` |
| `[[` | body, insert mode | headline completion |

- `gf` follows vim's "go to file under cursor". `Ctrl-]` is taken by `demote` in the tree (`crates/leotui/src/bindings.rs:91`). `gd` stays free for a later go-to-definition.

- There is no forward jump. `Ctrl-i` sends the same byte as `Tab`, and `Tab` is `focus-to-tree` in the body.

- Completion inserts `\/` for a `/` in a headline.

- Renaming a page rewrites every link to it, in its own wiki and in `[[name:...]]` links from other wikis, as one undo step. Renaming a wiki root rewrites `[[old:...]]` links to `[[new:...]]`.

---

## 6. Export

`export-wiki` at a wiki root writes `<name>.md`.

- **Directory.** The `@path` in effect at the root, else the directory of the `.leo` file.

- **Overwrite check.** Refuse if the target file belongs to an `@<file>` node in the outline.

- **Layout.** The root body comes first, then each page in outline order. A page's heading is its headline, with `#` repeated to its depth below the root.

- **Body headings.** Headings inside a page body move down by the page's depth.

- **Links.**

  - `[[Page|text]]` becomes `[text](#anchor)`.

  - `[[other:Page]]` becomes `[Page](other.md#anchor)`.

  - `[[other:]]` becomes `[other](other.md)`.

  - Cross-links assume both wikis export to the same directory.

`write_markdown` (`crates/leolib/src/importers/lines.rs:346`) already writes a subtree as headings and drops directive lines. Export can reuse its walk. It does not cap heading depth: depth 7 writes `#######`, which CommonMark reads as a paragraph, not a heading.

### `convert-wikilinks-to-unls`

This command rewrites one wiki's links as Leo links, for outlines shared with Leo users.

- It writes `` `unl:gnx://#<gnx>` ``. A headline UNL matches across the whole outline, not one wiki, so it could resolve to a different node.

- It wraps each link in backticks, because of the delimiter rule in section 3.

- It drops labels. Leo has no link-label syntax.

---

## 7. Open questions

- **Q1. Anchors.** GitHub-style heading anchors, or `<a id="...">` made from the gnx? GitHub-style anchors read well and match GitHub and pandoc's `gfm_auto_identifiers`, but not MkDocs' `toc`. `<a id>` anchors work in every renderer but put raw HTML in the output.

- **Q2. Fence colouring.** `highlight.rs` colours code after `@language`. C4 bans `@language`, so the md highlighter must colour fenced blocks by their info string to avoid a regression. Is that part of this feature, or a prerequisite?

- **Q3. Depth over 6.** Refuse at export, flatten to `######`, or emit bold text?

- **Q4. Transclusion.** C2 removes the Leo idiom of cloning a code node into documentation. `![[Page]]` or `![[#gnx]]` could inline a node's body as a fenced block at export. Defer until asked for.

- **Q5. Links into a wiki from outside.** Section 3 covers them with Leo links. Should `[[name:Page]]` also be live in code comments once cross-links exist?
