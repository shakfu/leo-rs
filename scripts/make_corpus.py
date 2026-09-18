#!/usr/bin/env python3
"""
Build leolib's conformance corpus in demo/, from Python Leo.

Every .leo file under demo/ is a case: an outline, beside the external files
it names. For each one this writes <name>.expected.json, recording what
Python leolib reads from it. leo-rs's crates/leolib/tests/corpus.rs checks
the Rust leolib against those files and leo-editor's test_leolib_corpus.py
checks the Python one, so each implementation is held to the same answers
with no checkout of the other in the loop.

    python3 scripts/make_corpus.py --leo-editor ~/projects/leo-editor
        Rewrite every expected file.
    ... --check
        Compare instead of writing. Exits 1 if any case differs.
    ... --create
        First rebuild the hand-made cases under demo/cases/. Their gnxs are
        fixed, so rebuilding changes no byte unless this script changed.
    ... --copy-to DIR
        Afterwards copy the corpus to DIR. leo-editor keeps its copy in
        leo/unittests/leolib/corpus/.

An expected file is Python's answer, not the truth. Where the two
implementations differ on purpose, the Rust test says so, and why.

An expected file lists the outline in outline order: each position's level,
headline and body, and its gnx. A gnx is recorded only where it comes from a
file. The nodes an @auto importer builds get fresh gnxs on every read, in
either implementation, so under an @auto node the gnx is null.
"""

import argparse
import json
import os
import shutil
import sys
from pathlib import Path

DEMO = Path(__file__).resolve().parent.parent / 'demo'
CASES = DEMO / 'cases'


# --- the hand-made cases --------------------------------------------------
# Each builder gets an empty outline, an `add` for a top-level node and the
# case's directory, and covers one feature: a reader of the corpus should be
# able to point at a case and name what it is for. External files are written
# by Leo's own writers, so every file has exactly the bytes Leo would give it;
# the @auto sources are the exception, and are written verbatim.

def build_at_file(o, add, case):
    """@file: an external file with sentinels."""
    p = add('@file file.py', '@others\n')
    child(p, 'f', 'def f():\n    return 1\n')
    return {}


def build_at_thin(o, add, case):
    """@thin: @file's older name, and the same writer."""
    p = add('@thin thin.py', '@others\n')
    child(p, 'f', 'def f():\n    return 1\n')
    return {}


def build_at_clean(o, add, case):
    """@clean: an external file with no sentinels, read back by diffing it."""
    p = add('@clean clean.py', '@others\n')
    child(p, 'g', 'def g():\n    return 2\n')
    return {}


def build_at_nosent(o, add, case):
    """@nosent: written without sentinels and never read back."""
    add('@nosent nosent.py', 'x = 1\n')
    return {}


def build_at_asis(o, add, case):
    """@asis: the subtree's bodies, concatenated, with nothing added."""
    p = add('@asis asis.txt', 'First part, as is.\n')
    child(p, 'more', 'Second part.\n')
    return {}


def build_at_edit(o, add, case):
    """@edit: one file in one body, with no children."""
    add('@edit edit.txt', 'Text for @edit.\n')
    return {}


def build_at_auto(o, add, case):
    """@auto: a file split into a tree by an importer, with no sentinels."""
    add('@auto auto.py', '')
    return {'auto.py': 'import os\n\n\ndef f():\n    return 1\n'}


def build_at_path(o, add, case):
    """@path: the directory an @<file> node below it writes to."""
    (case / 'sub').mkdir(parents=True, exist_ok=True)
    p = add('@path sub', 'Files below here live in sub/.\n')
    q = child(p, '@file inside.py', '@others\n')
    child(q, 'f', 'def f():\n    return 1\n')
    return {}


def build_at_others(o, add, case):
    """@others: where the descendants' text goes, and at what indentation."""
    p = add('@file others.py', 'class C:\n    @others\n')
    child(p, 'm', 'def m(self):\n    return 1\n')
    child(p, 'n', 'def n(self):\n    return 2\n')
    return {}


def build_section_refs(o, add, case):
    """A << section >> reference: a child written where the reference is."""
    p = add('@file section.py', '<< imports >>\n\n\n@others\n')
    child(p, '<< imports >>', 'import os\n')
    child(p, 'f', 'def f():\n    return os.sep\n')
    return {}


def build_at_section_delims(o, add, case):
    """@section-delims: other brackets for section references."""
    p = add('@file section_delims.py', '@section-delims { }\n{ imports }\n\n\n@others\n')
    child(p, '{ imports }', 'import os\n')
    child(p, 'f', 'def f():\n    return os.sep\n')
    return {}


def build_at_first(o, add, case):
    """@first: a line written above the sentinel header."""
    add('@file first.py', '@first #!/usr/bin/env python3\nx = 1\n')
    return {}


def build_at_last(o, add, case):
    """@last: a line written below the closing sentinel."""
    add('@file last.py', 'x = 1\n@last # the last line\n')
    return {}


def build_at_all(o, add, case):
    """@all: every descendant's body, in outline order, section names included."""
    p = add('@file all.py', '@all\n')
    child(p, 'first', 'x = 1\n')
    child(p, 'second', 'y = 2\n')
    return {}


def build_at_ignore(o, add, case):
    """@ignore: an @<file> node below it is neither read nor written."""
    p = add('@ignore', 'Nothing below here reaches the disk.\n')
    q = child(p, '@file ignored.py', '@others\n')
    child(q, 'f', 'def f():\n    return 1\n')
    add('@file written.py', 'x = 1\n')
    return {}


def build_at_comment(o, add, case):
    """@comment: the comment delimiter the sentinels are written with."""
    p = add('@file comment.txt', '@comment ;\n@others\n')
    child(p, 'f', 'x = 1\n')
    return {}


def build_at_delims(o, add, case):
    """@delims: the sentinel delimiters, changed part way through a file."""
    p = add('@file delims.css', '@delims /* */\n@others\n')
    child(p, 'rule', 'body {\n    margin: 0;\n}\n')
    return {}


def build_at_language(o, add, case):
    """@language: the language a file is in, where its extension does not say."""
    p = add('@file language.txt', '@language python\n@others\n')
    child(p, 'f', 'x = 1\n')
    return {}


def build_at_tabwidth(o, add, case):
    """@tabwidth: the character the writer indents @others with, a tab here."""
    p = add('@file tabwidth.py', '@tabwidth 4\nclass C:\n    @others\n')
    child(p, 'm', 'def m(self):\n    return 1\n')
    return {}


def build_doc_parts(o, add, case):
    """@doc and @c: prose in a body, written as comments."""
    add('@file doc.py', '@doc\nProse, written as comments.\n@c\nx = 1\n')
    return {}


def build_uas(o, add, case):
    """Unknown attributes: a text one and a pickled one, a node each.

    Leo leaves a `str_` value as text and pickles anything else. Both have to
    reach the `.leo` file so that reading it again gives them back. One uA per
    node: Leo also writes them into a `descendentVnodeUnknownAttributes` blob,
    whose pickled dict comes back from a read in another key order, so a node
    with two of them is not rewritten unchanged.
    """
    p = add('a node with a text uA', 'Its uA is text, and needs escaping.\n')
    p.v.unknownAttributes = {'str_note': 'a value with " and < and a\nline break'}
    p = add('a node with a pickled uA', 'Its uA is a hexlified pickle.\n')
    p.v.unknownAttributes = {'plugin_data': {'count': 2, 'names': ['one', 'two']}}
    return {}


def build_clones(o, add, case):
    """A node cloned twice inside an @file tree, and once outside it."""
    p = add('@file clones.py', '@others\n')
    shared = child(p, 'shared', 'def shared():\n    return 1\n')
    other = child(p, 'other', 'def other():\n    @others\n    return 2\n')
    shared.clone().moveToLastChildOf(other)
    outside = add('outside the file', 'A clone of `shared`, outside clones.py.\n')
    shared.clone().moveToLastChildOf(outside)
    return {}


def build_line_endings(o, add, case):
    """Files written with CRLF line endings, by @lineending."""
    p = add('@file crlf.py', '@lineending crlf\n@others\n')
    child(p, 'f', 'def f():\n    return 1\n')
    add('@clean crlf.txt', '@lineending crlf\nline one\nline two\n')
    return {}


def build_encoding(o, add, case):
    """A file written in latin-1, by @encoding."""
    p = add('@file latin.py', '@encoding latin-1\n@others\n')
    child(p, 'names', "name = 'café'\nother = 'naïve'\n")
    return {}


def build_sentinel_lookalikes(o, add, case):
    """Ordinary lines that look like sentinels, in the three kinds that write text."""
    body = (
        'def f():\n'
        '    # @not a sentinel\n'
        '    s = """\n'
        '#@+node:not-a-node\n'
        '# @others\n'
        '"""\n'
        '    return s\n'
    )
    p = add('@file lookalike.py', '@others\n')
    child(p, 'f', body)
    add('@nosent lookalike_nosent.py', body)
    add('@clean lookalike_clean.py', body)
    return {}


def build_auto_languages(o, add, case):
    """@auto in four languages; the importers build the trees."""
    sources = {
        'a.py': 'import os\n\n\ndef f():\n    return 1\n\n\nclass C:\n    def m(self):\n        return 2\n',
        'b.js': 'function f() {\n    return 1;\n}\n\nfunction g() {\n    return 2;\n}\n',
        'c.md': '# Title\n\nText.\n\n## Section\n\nMore text.\n',
        'd.org': '* Heading\nText.\n** Sub\nMore.\n',
    }
    for name in sources:
        add(f'@auto {name}', '')
    return sources


def build_empty_auto(o, add, case):
    """An @auto file with nothing in it, beside one with something.

    Leo's `at.readFileAtPosition` raises `AttributeError` on the empty one and
    reports it unread; this port imports it as the empty tree it is.
    """
    add('@auto empty.py', '')
    add('@auto one.py', '')
    return {'empty.py': '', 'one.py': 'x = 1\n'}


def build_unreadable(o, add, case):
    """An @file whose file has no sentinels, which both report unread, and one that reads."""
    add('@file plain.py', '@others\n')
    add('@clean readable.txt', 'A file that reads.\n')
    return {'plain.py': 'x = 1\n'}


# One case per feature, named for it. The cases after the blank line cover a
# feature no single directive names: how the readers and writers behave.
BUILDERS = {
    'at_file': build_at_file,
    'at_thin': build_at_thin,
    'at_clean': build_at_clean,
    'at_nosent': build_at_nosent,
    'at_asis': build_at_asis,
    'at_edit': build_at_edit,
    'at_auto': build_at_auto,
    'at_path': build_at_path,
    'at_others': build_at_others,
    'section_refs': build_section_refs,
    'at_section_delims': build_at_section_delims,
    'at_first': build_at_first,
    'at_last': build_at_last,
    'at_all': build_at_all,
    'at_ignore': build_at_ignore,
    'at_comment': build_at_comment,
    'at_delims': build_at_delims,
    'at_language': build_at_language,
    'at_tabwidth': build_at_tabwidth,
    'at_encoding': build_encoding,
    'at_lineending': build_line_endings,
    'doc_parts': build_doc_parts,
    'uas': build_uas,

    'clones': build_clones,
    'sentinel_lookalikes': build_sentinel_lookalikes,
    'auto_languages': build_auto_languages,
    'unreadable': build_unreadable,
    'empty_auto': build_empty_auto,
}


def child(p, h, b):
    c = p.insertAsLastChild()
    c.h, c.b = h, b
    return c


def all_positions(o):
    """Every position in outline order, clones included each time."""
    p = o.rootPosition()
    while p:
        yield p.copy()
        p.moveToThreadNext()


def create(leolib, name, build):
    """Rebuild demo/cases/<name>/ from nothing."""
    case = CASES / name
    shutil.rmtree(case, ignore_errors=True)
    case.mkdir(parents=True)
    o = leolib.new_outline(str(case / f'{name}.leo'))
    first = [True]
    last = [o.rootPosition()]

    def add(h, b):
        p = last[0] if first[0] else last[0].insertAfter()
        first[0] = False
        p.h, p.b = h, b
        last[0] = p
        return p

    sources = build(o, add, case)
    # Fixed gnxs, in outline order, so a rebuild is byte for byte the same.
    seen = []
    for p in all_positions(o):
        if p.v not in seen:
            seen.append(p.v)
    for i, v in enumerate(seen, 1):
        v.fileIndex = f'corpus.20260911000000.{i}'
    for p in all_positions(o):
        p.setDirty()
    leolib.write_external_files(o)
    for filename, text in sources.items():
        (case / filename).write_text(text, encoding='utf-8', newline='')
    leolib.save(o)


# --- the expected files -----------------------------------------------------

def describe(leolib, leo_path):
    """What Python leolib reads from leo_path."""
    probe = leolib.open_outline(str(leo_path), read_external=False)
    at = probe.atFileCommands
    targets = [probe.fullPath(p) for p in at.findFilesToRead(probe.rootPosition(), all=True)]
    # A case whose external files are not all beside it -- demo/LeoPyRef.leo,
    # a copy of Leo's own outline without Leo's sources -- is read without
    # them, or what it reads would depend on what else is on the disk.
    read_external = bool(targets) and all(os.path.exists(t) for t in targets)
    o, report = leolib.open_outline_with_report(str(leo_path), read_external)
    positions = []
    for p in all_positions(o):
        imported = any(q.isAtAutoNode() for q in p.parents())
        positions.append({
            'level': p.level(),
            'gnx': None if imported else p.gnx,
            'h': p.h,
            'b': p.b,
        })
    return {
        'read_external': read_external,
        'unread': sorted(e.headline for e in report.errors),
        'positions': positions,
    }


def dump(data):
    return json.dumps(data, indent=1, ensure_ascii=False) + '\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    parser.add_argument('--leo-editor', required=True, help='a leo-editor checkout')
    parser.add_argument('--check', action='store_true', help='compare, do not write')
    parser.add_argument('--create', action='store_true', help='rebuild demo/cases/ first')
    parser.add_argument('--copy-to', help='copy the corpus here afterwards')
    args = parser.parse_args()
    sys.path.insert(0, str(Path(args.leo_editor).expanduser().resolve()))
    from leo import leolib

    if args.create:
        for name, build in BUILDERS.items():
            create(leolib, name, build)
            print(f'created demo/cases/{name}/')

    differ = []
    for leo_path in sorted(DEMO.rglob('*.leo')):
        expected = leo_path.with_name(leo_path.stem + '.expected.json')
        text = dump(describe(leolib, leo_path))
        name = leo_path.relative_to(DEMO)
        if args.check:
            if not expected.exists() or expected.read_text(encoding='utf-8') != text:
                differ.append(str(name))
        else:
            expected.write_text(text, encoding='utf-8', newline='')
            print(f'wrote {expected.relative_to(DEMO)}')
    if differ:
        print('differ:', ', '.join(differ))
        return 1

    if args.copy_to:
        target = Path(args.copy_to).expanduser()
        shutil.copytree(DEMO, target, dirs_exist_ok=True)
        print(f'copied demo/ to {target}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
