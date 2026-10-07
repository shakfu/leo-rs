# Tests for the README's examples

Each test runs one named example from `README.md`. A failing test means the
README no longer describes `textstats` as it is.

```python file=test_readme.py
def test_count():
    <<README.md#count>>


def test_top():
    <<README.md#top>>
```
