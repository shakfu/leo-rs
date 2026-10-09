# The report's cells, run as a module. `make render` runs them with Quarto,
# in the repository's uv environment, which has Jupyter: `uv sync` first.

PYTHON ?= python3
QUARTO ?= uv run quarto

.PHONY: test render clean

test:
	$(PYTHON) rainfall.py

render:
	$(QUARTO) render rainfall.qmd

clean:
	rm -rf rainfall.html rainfall_files __pycache__
