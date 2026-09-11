# leo-rs: leolib and its terminal front end.
#
# The tests need nothing outside this repo: the conformance corpus is in
# demo/. Checking its expected files against Python Leo needs a leo-editor
# checkout, so `make corpus` asks for one:
#
#     make corpus LEO_EDITOR=~/projects/leo-editor

.PHONY: test corpus build release fmt lint audit check run dump clean

test:
	cargo test --workspace

corpus:
	@test -n "$(LEO_EDITOR)" || { echo "set LEO_EDITOR to a leo-editor checkout"; exit 1; }
	python3 scripts/make_corpus.py --leo-editor $(LEO_EDITOR) --check

build:
	cargo build --workspace

release:
	cargo build --workspace --release

fmt:
	cargo fmt --all

lint:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings

# Kept out of `check`: it fetches the RustSec advisory database.
audit:
	@cargo audit --version >/dev/null 2>&1 || { echo "cargo install cargo-audit --locked"; exit 1; }
	cargo audit

check: lint test

run:
	cargo run -p leotui -- $(FILE)

dump:
	cargo run -q -p leotui -- $(FILE) --dump

clean:
	cargo clean
