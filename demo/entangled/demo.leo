<?xml version="1.0" encoding="utf-8"?>
<!-- Created by leo-rs -->
<leo_file xmlns:leo="http://leoeditor.com/namespaces/leo-python-editor/1.1" >
<leo_header file_format="2"/>
<vnodes>
<v t="a.1"><vh>About this demo</vh></v>
<v t="a.2"><vh>@entangled README.md</vh></v>
<v t="a.3"><vh>@entangled tests.md</vh></v>
<v t="a.4"><vh>@clean textstats.py</vh></v>
</vnodes>
<tnodes>
<t tx="a.1">The README's code examples are tested.

@entangled README.md documents textstats. Each named example, such as
#count, is a &lt;&lt; count &gt;&gt; node, coloured and served as Python.

@entangled tests.md places each example in a pytest function, by
reference: &lt;&lt;README.md#count&gt;&gt;. It tangles to test_readme.py.

@clean textstats.py is the library the examples use.

To run: :entangled-tangle, then pytest in this directory, or make test.
Change an example so it no longer holds, and its test fails.
</t>
<t tx="a.2"></t>
<t tx="a.3"></t>
<t tx="a.4">"""Word statistics for a piece of text."""

from collections import Counter


def words(text):
    """The words in text, lower-cased, punctuation stripped."""
    return [w.strip(".,;:!?\"'()").lower() for w in text.split() if w.strip(".,;:!?\"'()")]


def word_count(text):
    """How many words text has."""
    return len(words(text))


def top_words(text, n):
    """The n most common words, with their counts, most common first."""
    return Counter(words(text)).most_common(n)
</t>
</tnodes>
</leo_file>
