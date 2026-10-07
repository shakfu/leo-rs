# leo-rs: leolib and its terminal front end.
#
# The tests need nothing outside this repo: the conformance corpus is in
# demo/. Checking its expected files against Python Leo needs a leo-editor
# checkout, so `make corpus` asks for one:
#
#     make corpus LEO_EDITOR=~/projects/leo-editor

.PHONY: test corpus bench build release fmt lint audit check run gui gui-glow dump clean

test:
	cargo test --workspace

corpus:
	@test -n "$(LEO_EDITOR)" || { echo "set LEO_EDITOR to a leo-editor checkout"; exit 1; }
	python3 scripts/make_corpus.py --leo-editor $(LEO_EDITOR) --check

# With LEO_EDITOR set, also times leo-editor's own outline and its files.
bench:
	LEO_EDITOR=$(LEO_EDITOR) cargo bench -p leolib

build:
	cargo build --workspace

release:
	cargo build --workspace --release

fmt:
	cargo fmt --all

# Each crate ships a copy of LICENSE, not a symlink: Windows checkouts break those.
lint:
	@for d in crates/*/; do cmp -s LICENSE "$${d}LICENSE" || { echo "$${d}LICENSE differs from LICENSE or is missing: cp LICENSE $$d"; exit 1; }; done
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings

# Kept out of `check`: it fetches the RustSec advisory database.
audit:
	@cargo audit --version >/dev/null 2>&1 || { echo "cargo install cargo-audit --locked"; exit 1; }
	cargo audit

check: lint test

run:
	cargo run -p leotui -- $(FILE)

# Release: egui's debug build is slow to draw.
gui:
	cargo run --release -p leogui --bin leogui -- $(FILE)

# The same, drawn with OpenGL (glow) instead of wgpu, to compare.
gui-glow:
	cargo run --release -p leogui --bin leogui-glow -- $(FILE)

dump:
	cargo run -q -p leotui -- $(FILE) --dump

clean:
	cargo clean
