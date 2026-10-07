"""Word statistics for a piece of text."""

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
