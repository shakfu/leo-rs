<?xml version="1.0" encoding="utf-8"?>
<!-- Created by Leo: https://leo-editor.github.io/leo-editor/leo_toc.html -->
<leo_file xmlns:leo="https://leo-editor.github.io/leo-editor/namespaces/leo-python-editor/1.1" >
<leo_header file_format="2"/>
<globals/>
<preferences/>
<find_panel_settings/>
<vnodes>
<v t="a.1"><vh>About this demo</vh></v>
<v t="a.2" descendentVnodeUnknownAttributes="7d710058010000003071017d710258130000007374725f6c656f2d72732d63656c6c2d6964737103583500000064617461207374612e32303236313030393033333632382e320a7374617473207374612e32303236313030393033333632382e340a710473732e"><vh>@qmd rainfall.qmd</vh></v>
<v t="sta.20261009033628.6"><vh>@clean rainfall.py</vh>
<v t="sta.20261009033628.2"><vh>&lt;&lt; data &gt;&gt;</vh></v>
<v t="sta.20261009033628.4"><vh>&lt;&lt; stats &gt;&gt;</vh></v>
</v>
</vnodes>
<tnodes>
<t tx="a.1">A Quarto report whose cells are also a Python module.

@qmd rainfall.qmd reads the report: each heading is a node, and each
executable cell a &lt;&lt; name &gt;&gt; node in Python. The labelled cells are
&lt;&lt; data &gt;&gt; and &lt;&lt; stats &gt;&gt;; the unlabelled one is &lt;&lt; python cell 3 &gt;&gt;.
The display fence and the callout's heading stay in the prose.

@clean rainfall.py holds clones of &lt;&lt; data &gt;&gt; and &lt;&lt; stats &gt;&gt;. Edit
either clone and a save writes both files. The cells' #| lines are
Python comments, so the module runs as it is.

To run: python3 rainfall.py, or make test. quarto render rainfall.qmd
renders the report.
</t>
<t tx="a.2" str_leo-rs-cell-ids="data sta.20261009033628.2&#10;stats sta.20261009033628.4&#10;"></t>
<t tx="sta.20261009033628.2">#| label: data
RAIN = [78, 52, 61, 45, 50, 41, 38, 49, 55, 83, 90, 86]
</t>
<t tx="sta.20261009033628.4">#| label: stats
def mean(xs):
    return sum(xs) / len(xs)


def wettest(xs):
    """The 1-based month with the most rain."""
    return xs.index(max(xs)) + 1
</t>
<t tx="sta.20261009033628.6">@language python
&lt;&lt; data &gt;&gt;
&lt;&lt; stats &gt;&gt;

if __name__ == "__main__":
    assert mean([1, 2, 3]) == 2
    assert wettest([1, 3, 2]) == 2
    print(f"mean {mean(RAIN):.1f} mm, wettest month {wettest(RAIN)}")
</t>
</tnodes>
</leo_file>
