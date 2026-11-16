# leo-rs: leolib and its terminal front end.
#
# The tests need nothing outside this repo: the conformance corpus is in
# demo/. Checking its expected files against Python Leo needs a leo-editor
# checkout, so `make corpus` asks for one:
#
#     make corpus LEO_EDITOR=~/projects/leo-editor

# PLUGINS=all builds and runs leotui-all and leogui-all (crates/leo-all): every
# plugin, the unpublished leo-entangled and the agents included. It sets ENTANGLED=1.
ifeq ($(PLUGINS),all)
ENTANGLED := 1
TUI := -p leo-all --bin leotui-all
GUI_BIN := -p leo-all --bin leogui-all
else
TUI := -p leotui
GUI_BIN := -p leogui --bin leogui
endif

# leo-entangled, and leo-all, which depends on it, are left out unless
# ENTANGLED=1, e.g. `make test ENTANGLED=1`.
ifeq ($(ENTANGLED),1)
WS := --workspace --features leo-entangled/leoapp
else
WS := --workspace --exclude leo-entangled --exclude leo-all
endif

.PHONY: test corpus bench build release install uninstall fmt lint audit check publish run tui gui gui-glow agent-demo dump clean

test:
	cargo test $(WS)

corpus:
	@test -n "$(LEO_EDITOR)" || { echo "set LEO_EDITOR to a leo-editor checkout"; exit 1; }
	python3 scripts/make_corpus.py --leo-editor $(LEO_EDITOR) --check

# With LEO_EDITOR set, also times leo-editor's own outline and its files.
bench:
	LEO_EDITOR=$(LEO_EDITOR) cargo bench -p leolib

build:
	cargo build $(WS)

release:
	cargo build $(WS) --release

# leotui-all and leogui-all, release builds with every plugin, installed as
# leotui and leogui. BINDIR=... installs elsewhere.
BINDIR ?= $(HOME)/.local/bin
install:
	cargo build --release -p leo-all
	mkdir -p $(BINDIR)
	install -m 755 target/release/leotui-all $(BINDIR)/leotui
	install -m 755 target/release/leogui-all $(BINDIR)/leogui

uninstall:
	rm -f $(BINDIR)/leotui $(BINDIR)/leogui

fmt:
	cargo fmt --all

# Each crate ships a copy of LICENSE, not a symlink: Windows checkouts break those.
lint:
	@for d in crates/*/; do cmp -s LICENSE "$${d}LICENSE" || { echo "$${d}LICENSE differs from LICENSE or is missing: cp LICENSE $$d"; exit 1; }; done
	cargo fmt --all -- --check
	cargo clippy $(WS) --all-targets -- -D warnings

# Kept out of `check`: it fetches the RustSec advisory database.
audit:
	@cargo audit --version >/dev/null 2>&1 || { echo "cargo install cargo-audit --locked"; exit 1; }
	cargo audit

check: lint test

# Not $(WS): ENTANGLED=1 must not publish leo-entangled or leo-all.
publish:
	cargo publish --workspace --exclude leo-entangled --exclude leo-all

run:
	cargo run $(TUI) -- $(FILE)

tui: run

# Release: egui's debug build is slow to draw.
gui:
	cargo run --release $(GUI_BIN) -- $(FILE)

# The same, drawn with OpenGL (glow) instead of wgpu, to compare.
gui-glow:
	cargo run --release -p leogui --bin leogui-glow -- $(FILE)

# demo/agent with the agent plugin, which is off by default; GUI=1 for leogui.
# Its agents need the lines in demo/agent/agents.toml.
agent-demo:
ifeq ($(GUI),1)
	cargo run --release -p leogui --bin leogui --features leo-plugins/agent -- demo/agent/demo.leo
else
	cargo run -p leotui --features leo-plugins/agent -- demo/agent/demo.leo
endif

dump:
	cargo run -q -p leotui -- $(FILE) --dump

clean:
	cargo clean
