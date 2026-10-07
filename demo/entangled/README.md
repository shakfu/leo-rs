# textstats

Word statistics for a piece of text.

## Counting words

`word_count` counts the words, ignoring punctuation:

```python #count
from textstats import word_count

assert word_count("The cat sat. The cat ran!") == 6
```

## The most common words

`top_words` lists the most common words with their counts, most common
first. Case does not matter:

```python #top
from textstats import top_words

assert top_words("a b A c a b", 2) == [("a", 3), ("b", 2)]
```

## Running the examples

Every named example above is tested. `tests.md` places each in a test
function; tangle it and run pytest:

```sh
entangled tangle
pytest
```
