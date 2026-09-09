# leo-rs: leolib and its terminal front end.
#
# LEO_CORPUS points the corpus tests at a real .leo file. Without it they skip;
# they are the only tests that need anything outside this repo.

CORPUS ?= $(HOME)/projects/personal/leo-editor/leo/core/LeoPyRef.leo
CORPUS_DIR ?= $(HOME)/projects/personal/leo-editor/leo

.PHONY: test test-corpus build release fmt lint check run dump clean

test:
	cargo test --workspace

test-corpus:
	LEO_CORPUS=$(CORPUS) LEO_CORPUS_DIR=$(CORPUS_DIR) cargo test --workspace

build:
	cargo build --workspace

release:
	cargo build --workspace --release

fmt:
	cargo fmt --all

lint:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings

check: lint test-corpus

run:
	cargo run -p leotui -- $(FILE)

dump:
	cargo run -q -p leotui -- $(FILE) --dump

clean:
	cargo clean
