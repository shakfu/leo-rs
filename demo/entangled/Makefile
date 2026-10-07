# The README's examples, tested: `make test` tangles tests.md and runs pytest.

ENTANGLED ?= entangled
PYTHON    ?= python3

.PHONY: tangle test clean

tangle:
	$(ENTANGLED) tangle

test: tangle
	$(PYTHON) -m pytest -q test_readme.py

clean:
	rm -rf test_readme.py .entangled __pycache__ .pytest_cache
