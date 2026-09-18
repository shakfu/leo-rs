# demo

The conformance corpus. Every `.leo` file here is a case, beside the external
files it names, with a `<name>.expected.json` holding what Python Leo reads
from it. `crates/leolib/tests/corpus.rs` checks this port against those files
and leo-editor checks Python Leo against its own copy, so neither
implementation needs a checkout of the other.

`scripts/make_corpus.py` builds the cases under `cases/` and writes every
expected file:

```sh
make corpus LEO_EDITOR=~/projects/leo-editor        # compare, do not write
python3 scripts/make_corpus.py --leo-editor DIR --create   # rebuild cases/
```

The gnxs in `cases/` are fixed, so a rebuild changes no byte unless the script
changed. `--copy-to` puts the corpus where leo-editor keeps its copy.

## One case per feature

| case | what it holds |
|---|---|
| `at_file` | `@file`: an external file with sentinels |
| `at_thin` | `@thin`, `@file`'s older name |
| `at_clean` | `@clean`: no sentinels, read back by diffing |
| `at_nosent` | `@nosent`: written, never read |
| `at_asis` | `@asis`: the subtree's bodies, nothing added |
| `at_edit` | `@edit`: one file, one body, no children |
| `at_auto` | `@auto`: a file split by an importer |
| `at_path` | `@path`: the directory the file below it goes to |
| `at_others` | `@others`: where the descendants go, and at what indentation |
| `section_refs` | a `<< section >>` reference |
| `at_section_delims` | `@section-delims`: other brackets for a reference |
| `at_first` | `@first`: a line above the sentinel header |
| `at_last` | `@last`: a line below the closing sentinel |
| `at_all` | `@all`: every descendant's body, in outline order |
| `at_ignore` | `@ignore`: the `@file` below it is neither read nor written |
| `at_comment` | `@comment`: the delimiter the sentinels use |
| `at_delims` | `@delims`: the delimiters, changed part way through |
| `at_language` | `@language`, where the extension does not say |
| `at_tabwidth` | `@tabwidth`: the character `@others` indents with |
| `at_encoding` | `@encoding`: a file written in latin-1 |
| `at_lineending` | `@lineending`: files written with CRLF |
| `doc_parts` | `@doc` and `@c`: prose written as comments |
| `uas` | unknown attributes, one text and one pickled |
| `clones` | a node cloned inside an `@file` tree and outside it |
| `sentinel_lookalikes` | ordinary lines that look like sentinels |
| `auto_languages` | `@auto` in four languages |

Three cases hold a behaviour rather than a feature, where the two
implementations answer differently. Each is listed in `corpus.rs`'s `KNOWN` or
`KNOWN_TANGLE` with the reason:

| case | what it pins |
|---|---|
| `empty_auto` | an `@auto` file with nothing in it |
| `unreadable` | an `@file` whose file has no sentinels |
| `at_encoding` | a file that is not UTF-8 |

A case has to read as Python Leo reads it, rewrite its `.leo` file unchanged
and leave its external files alone: leo-editor's `test_leolib_corpus.py` holds
its own copy to all three. Behaviour that breaks one of those, such as
`@comment` in a file whose extension names a language, belongs in `TODO.md`
instead.

`demo.leo`, `workbook.leo` and `LeoPyRef.leo` are whole outlines, not cases
for one feature: `LeoPyRef.leo` is Leo's own outline, read without its
external files.
