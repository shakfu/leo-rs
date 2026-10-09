#| label: data
RAIN = [78, 52, 61, 45, 50, 41, 38, 49, 55, 83, 90, 86]
#| label: stats
def mean(xs):
    return sum(xs) / len(xs)


def wettest(xs):
    """The 1-based month with the most rain."""
    return xs.index(max(xs)) + 1

if __name__ == "__main__":
    assert mean([1, 2, 3]) == 2
    assert wettest([1, 3, 2]) == 2
    print(f"mean {mean(RAIN):.1f} mm, wettest month {wettest(RAIN)}")
