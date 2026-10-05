#  upstream leo-editor bugs

Leo's bugs, which this port does not reproduce. Nothing to change here.

- [ ] **Leo's `descendentVnodeUnknownAttributes` blob is not stable across a read.** Its pickled dict comes back in another key order, so opening a `.leo` file with two uAs on one node and saving it rewrites the file with no edit. `demo/cases/uas` puts one uA per node to stay inside leo-editor's "rewritten unchanged" test. This port rebuilds the blob from the tree, in the key order the tree gives.

- [ ] **Leo reads a section reference back with its delimiters regex-escaped** when `@section-delims` set them (`leoAtFile.py:4016` assigns `re.escape`'d delims to `section_delim1`), so `{ imports }` becomes `\{ imports \}` in the body it hands back. This port keeps what the file spells. Report upstream; `corpus.rs`'s `KNOWN` holds it meanwhile.