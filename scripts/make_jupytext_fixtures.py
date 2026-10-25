#!/usr/bin/env python3
"""
Build the @jupytext fixtures in crates/leolib/tests/jupytext/ with jupytext.

    python3 scripts/make_jupytext_fixtures.py

Needs jupytext 1.19.6 (`pip install jupytext==1.19.6`). For each case this
writes:

    <case>.ipynb          the notebook, as nbformat writes it
    <case>.py             its py:percent text, as Leo's @jupytext read makes it
    <case>.<edit>.py      that text, edited
    <case>.<edit>.ipynb   the notebook a write of the edited text gives:
                          `combine_inputs_with_outputs`, as `jupytext --update`
    <case>.new.ipynb      the notebook the text gives with no notebook on disk

A new cell's id is random; it is written as NEW. leo-rs gives no id to a new
cell of a notebook older than nbformat 4.5, where jupytext gives one that
nbformat then reports as invalid; such ids are removed here.
"""

import io
import re
import sys
from pathlib import Path

import jupytext
import nbformat
from jupytext.combine import combine_inputs_with_outputs
from nbformat.v4 import new_code_cell, new_markdown_cell, new_notebook, new_output, new_raw_cell

OUT = Path(__file__).resolve().parent.parent / 'crates' / 'leolib' / 'tests' / 'jupytext'
VERSION = '1.19.6'


def to_text(nb):
    """Leo's jtm.read: jupytext.write to a stream."""
    f = io.StringIO()
    jupytext.write(nb, f, fmt='py:percent')
    return f.getvalue()


def to_json(nb):
    """jupytext.write to an .ipynb path, without the file."""
    return jupytext.writes(nb, fmt='ipynb') + '\n'


def mark_new_ids(text, old):
    old_ids = {c.get('id') for c in old.cells}
    new = nbformat.reads(text, as_version=4)
    for c in new.cells:
        if c.get('id') and c['id'] not in old_ids:
            if old.nbformat_minor < 5:
                text = re.sub(r'\n *"id": "%s",' % c['id'], '', text)
            else:
                text = text.replace('"id": "%s"' % c['id'], '"id": "NEW"')
    return text


def merged(text, old):
    nb = jupytext.reads(text, fmt='py:percent')
    nb = combine_inputs_with_outputs(nb, old, fmt='py:percent')
    return mark_new_ids(to_json(nb), old)


def python_case():
    ks = {'display_name': 'Python 3 (ipykernel)', 'language': 'python', 'name': 'python3'}
    nb = new_notebook(metadata={'kernelspec': ks, 'language_info': {'name': 'python', 'version': '3.12.3'}})
    nb.cells = [
        new_markdown_cell('# Title\n\nSome *text*.'),
        new_code_cell('import os\n%matplotlib inline\nx = 1', execution_count=1, outputs=[
            new_output('stream', name='stdout', text='one\ntwo\n'),
        ]),
        new_code_cell('def f():\n    return 1', metadata={'tags': ['a', 'b'], 'collapsed': True}),
        new_code_cell('f()', execution_count=2, outputs=[
            new_output('execute_result', data={'text/plain': '1'}, execution_count=2),
        ]),
        new_raw_cell('raw text\nline2'),
        new_code_cell('%%bash\necho hi', execution_count=3, outputs=[
            new_output('stream', name='stdout', text='hi\n'),
        ]),
        new_code_cell(''),
        new_markdown_cell('A note', metadata={'key': 'v', 'n': 3, 'none': None}),
        new_code_cell('y = 2\n\n', metadata={'title': 'Second'}),
        new_code_cell('!ls\na = !pwd', execution_count=4, outputs=[]),
    ]
    ids = ['c1a2b3d4', 'e5f60718', '293a4b5c', '6d7e8f90', 'a1b2c3d4', 'e5f6a7b8', 'c9d0e1f2', 'a3b4c5d6', 'e7f8a9b0', 'c1d2e3f4']
    for c, i in zip(nb.cells, ids):
        c['id'] = i
    text = to_text(nb)
    edits = {
        'edit': text.replace('\nf()\n', '\nf() + 1\n'),
        'add': text.replace('\nf()\n', '\nf()\n\n# %%\nz = 3\n') + '\n# %% [markdown]\n# The end\n',
        'delete': re.sub(r'# %% \[raw\]\n# raw text\n# line2\n\n', '', text).replace('# %% language="bash"\n# echo hi\n\n', ''),
        'reorder': text.replace('# %% [markdown]\n# # Title\n#\n# Some *text*.\n\n', '').replace(
            '# %%\nf()\n', '# %% [markdown]\n# # Title\n#\n# Some *text*.\n\n# %%\nf()\n'),
        'meta': text.replace('# %% tags=["a", "b"]', '# %% tags=["c"] seen=true'),
    }
    return nb, text, edits


def r_case():
    nb = new_notebook(metadata={'kernelspec': {'display_name': 'R', 'language': 'R', 'name': 'ir'}})
    nb.nbformat_minor = 4
    nb.cells = [
        new_markdown_cell('Caf\u00e9 au lait'),
        new_code_cell('x <- c(1, 2)\nmean(x)', execution_count=1, outputs=[
            new_output('execute_result', data={'text/plain': '[1] 1.5'}, execution_count=1),
        ]),
        new_code_cell('%%python\nprint(1)'),
    ]
    for c in nb.cells:
        del c['id']
    text = to_text(nb)
    edits = {'add': text + '\n# %%\ny <- 3\n'}
    return nb, text, edits


def header_case():
    md = {
        'kernelspec': {'display_name': 'Python 3', 'language': 'python', 'name': 'python3'},
        'jupytext': {'formats': 'ipynb,py:percent'},
    }
    nb = new_notebook(metadata=md)
    nb.cells = [
        new_raw_cell('---\ntitle: A notebook\n---'),
        new_code_cell('x = """\n# %% not a cell\n"""', execution_count=1, outputs=[]),
        new_markdown_cell('Text\n\n```\n%% not a cell either\n```'),
        new_code_cell('    indented = 1'),
    ]
    for n, c in enumerate(nb.cells):
        c['id'] = 'h%07d' % n
    text = to_text(nb)
    edits = {'edit': text.replace('title: A notebook', 'title: Renamed')}
    return nb, text, edits


def main():
    if jupytext.__version__ != VERSION:
        sys.exit(f'needs jupytext {VERSION}, not {jupytext.__version__}')
    OUT.mkdir(parents=True, exist_ok=True)
    for name, build in [('python', python_case), ('r', r_case), ('header', header_case)]:
        nb, text, edits = build()
        raw = to_json(nb)
        old = nbformat.reads(raw, as_version=4)
        (OUT / f'{name}.ipynb').write_text(raw)
        (OUT / f'{name}.py').write_text(text)
        new = jupytext.reads(text, fmt='py:percent')
        (OUT / f'{name}.new.ipynb').write_text(mark_new_ids(to_json(new), new_notebook(cells=[])))
        for edit, edited in edits.items():
            assert edited != text, (name, edit)
            (OUT / f'{name}.{edit}.py').write_text(edited)
            (OUT / f'{name}.{edit}.ipynb').write_text(merged(edited, old))


if __name__ == '__main__':
    main()
