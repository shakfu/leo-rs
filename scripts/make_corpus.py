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
# Each builder gets an empty outline and fills it. External files are then
# written by Leo's own writers, so every file has exactly the bytes Leo would
# give it; the @auto sources are the exception, and are written verbatim.

def build_directives(o, add):
    """One node for each of the six @<file> kinds."""
    p = add('@file file.py', '@others\n')
    child(p, 'f', 'def f():\n    return 1\n')
    p = add('@clean clean.py', '@others\n')
    child(p, 'g', 'def g():\n    return 2\n')
    add('@nosent nosent.py', 'x = 1\n')
    add('@edit edit.txt', 'Text for @edit.\n')
    p = add('@asis asis.txt', 'First part, as is.\n')
    child(p, 'more', 'Second part.\n')
    add('@auto auto.py', '')
    return {'auto.py': 'import os\n\n\ndef f():\n    return 1\n'}


def build_clones(o, add):
    """A node cloned twice inside an @file tree, and once outside it."""
    p = add('@file clones.py', '@others\n')
    shared = child(p, 'shared', 'def shared():\n    return 1\n')
    other = child(p, 'other', 'def other():\n    @others\n    return 2\n')
    shared.clone().moveToLastChildOf(other)
    outside = add('outside the file', 'A clone of `shared`, outside clones.py.\n')
    shared.clone().moveToLastChildOf(outside)
    return {}


def build_line_endings(o, add):
    """Files written with CRLF line endings, by @lineending."""
    p = add('@file crlf.py', '@lineending crlf\n@others\n')
    child(p, 'f', 'def f():\n    return 1\n')
    add('@clean crlf.txt', '@lineending crlf\nline one\nline two\n')
    return {}


def build_encoding(o, add):
    """A file written in latin-1, by @encoding."""
    p = add('@file latin.py', '@encoding latin-1\n@others\n')
    child(p, 'names', "name = 'café'\nother = 'naïve'\n")
    return {}


def build_sentinel_lookalikes(o, add):
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


def build_auto_languages(o, add):
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


BUILDERS = {
    'directives': build_directives,
    'clones': build_clones,
    'line_endings': build_line_endings,
    'encoding': build_encoding,
    'sentinel_lookalikes': build_sentinel_lookalikes,
    'auto_languages': build_auto_languages,
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

    sources = build(o, add)
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
