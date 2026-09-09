"""
Digest the @auto tree Leo's own importers produce, for comparison with this
crate's. Reads file names on stdin, writes `path<TAB>digest` on stdout.

    find ~/leo-editor -name '*.py' > /tmp/corpus.txt
    PYTHONPATH=~/leo-editor python3 docs/dev/compare-importers.py < /tmp/corpus.txt > /tmp/py.digests
    cargo run --example autotrees < /tmp/corpus.txt > /tmp/rs.digests
    diff /tmp/py.digests /tmp/rs.digests

The digest is FNV-1a over `level|headline|len(body)` and each node's body, so
the two implementations compare without a hash library on either side. Set
PYTHONPATH to a leo-editor checkout.
"""

import sys, os
from leo import leolib
import io, contextlib
for path in sys.stdin.read().split():
    path = os.path.abspath(path)
    try:
        o = leolib.new_outline('/tmp/x.leo')
        o.mFileName = os.path.join(os.path.dirname(path), 'x.leo')
        root = o.rootPosition()
        root.h = '@auto ' + path
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            o.atFileCommands.readFileAtPosition(root)
        text = ''.join(f"{p.level()}|{p.h}|{len(p.b)}\n{p.b}\x01\n" for p in root.self_and_subtree())
        h = 0xcbf29ce484222325
        for b in text.encode('utf8'):
            h ^= b
            h = (h * 0x100000001b3) & 0xFFFFFFFFFFFFFFFF
        print(f"{path}\t{h:016x}")
    except Exception as e:
        print(f"{path}\tERROR {type(e).__name__}")
