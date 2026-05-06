# Convenience targets for local development. The canonical build/test
# flow is plain `cargo`; this just wraps the multi-process net launcher.

SESSION ?= chess-local
PORT    ?= 7878
VARIANT ?= xiangqi

.PHONY: help build server play-local play-lobby stop-local stop-lobby check fmt clippy test wasm

help:
	@echo "Targets:"
	@echo "  build         build chess-net-server + chess-tui (debug)"
	@echo "  server        run a single chess-net-server on :$(PORT)"
	@echo "  play-local    tmux: window0=2 chess-tui --connect, window1=server"
	@echo "                (single hard-coded room — same as the MVP launcher)"
	@echo "  play-lobby    tmux: window0=3 chess-tui panes (2 picker + 1 --lobby"
	@echo "                watcher), window1=server. Exercises multi-room flow."
	@echo "  stop-local    kill the play-local tmux session"
	@echo "  stop-lobby    kill the play-lobby tmux session"
	@echo "  check         pre-push gates: fmt + clippy + test + wasm"
	@echo "  fmt | clippy | test | wasm   individual gates"
	@echo ""
	@echo "Vars: SESSION=$(SESSION) PORT=$(PORT) VARIANT=$(VARIANT)"
	@echo "Banqi example:        make play-lobby VARIANT=banqi"
	@echo "Custom port:          make play-lobby PORT=9000"

build:
	cargo build -p chess-net -p chess-tui

server: build
	./target/debug/chess-net-server --port $(PORT) $(VARIANT)

play-local:
	@scripts/play-local.sh $(SESSION) $(PORT) $(VARIANT)

play-lobby:
	@scripts/play-lobby.sh chess-lobby $(PORT) $(VARIANT)

stop-local:
	-tmux kill-session -t $(SESSION) 2>/dev/null

stop-lobby:
	-tmux kill-session -t chess-lobby 2>/dev/null

# Pre-push gates (mirrors CLAUDE.md / .github/workflows/ci.yml).
check: fmt clippy test wasm

fmt:
	cargo fmt --check

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace

wasm:
	cargo build --target wasm32-unknown-unknown -p chess-core
