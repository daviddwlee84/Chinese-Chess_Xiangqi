# Convenience targets for local development. The canonical build/test
# flow is plain `cargo`; this just wraps the multi-process net launcher.

SESSION ?= chess-local
PORT    ?= 7878
ADDR    ?= 127.0.0.1:$(PORT)
VARIANT ?= xiangqi
SERVER_ARGS ?=
WEB_BASE ?= /Chinese-Chess_Xiangqi

.PHONY: help build server play-local play-lobby play-web play-spectator build-web build-web-server build-web-static serve-web-prod stop-local stop-lobby stop-web stop-spectator check fmt clippy test wasm

help:
	@echo "Targets:"
	@echo "  build         build chess-net-server + chess-tui (debug)"
	@echo "  server        run a single chess-net-server on :$(PORT)"
	@echo "  play-local    tmux: window0=2 chess-tui --connect, window1=server"
	@echo "                (single hard-coded room — same as the MVP launcher)"
	@echo "  play-lobby    tmux: window0=3 chess-tui panes (2 picker + 1 --lobby"
	@echo "                watcher), window1=server. Exercises multi-room flow."
	@echo "  play-web      tmux: window0=server, window1=trunk serve (SPA on :8080,"
	@echo "                proxies /ws + /lobby to chess-net :$(PORT)). Requires"
	@echo "                trunk (cargo install trunk) + wasm32-unknown-unknown."
	@echo "                Dev only: do not use trunk serve for remote users."
	@echo "  play-spectator tmux: 1 server + 2 chess-tui players + 1 spectator pane."
	@echo "                 Demo for the v3 chat ('t') + spectator (?role=spectator) flow."
	@echo "  build-web     trunk build --release (writes clients/chess-web/dist)"
	@echo "  build-web-server same as build-web (server/same-origin web build)"
	@echo "  build-web-static GitHub Pages build (writes clients/chess-web/dist-static)"
	@echo "  serve-web-prod build release web assets, then serve SPA + WS from chess-net"
	@echo "  stop-local    kill the play-local tmux session"
	@echo "  stop-lobby    kill the play-lobby tmux session"
	@echo "  stop-web      kill the play-web tmux session"
	@echo "  stop-spectator kill the play-spectator tmux session"
	@echo "  check         pre-push gates: fmt + clippy + test + wasm"
	@echo "  fmt | clippy | test | wasm   individual gates"
	@echo ""
	@echo "Vars: SESSION=$(SESSION) PORT=$(PORT) ADDR=$(ADDR) VARIANT=$(VARIANT) WEB_BASE=$(WEB_BASE)"
	@echo "Banqi example:        make play-lobby VARIANT=banqi"
	@echo "Remote web example:   make serve-web-prod ADDR=0.0.0.0:7878"
	@echo "GitHub Pages example: make build-web-static WEB_BASE=/Chinese-Chess_Xiangqi"
	@echo "Custom port:          make play-lobby PORT=9000"

build:
	cargo build -p chess-net -p chess-tui

server: build
	./target/debug/chess-net-server --port $(PORT) $(VARIANT)

play-local:
	@scripts/play-local.sh $(SESSION) $(PORT) $(VARIANT)

play-lobby:
	@scripts/play-lobby.sh chess-lobby $(PORT) $(VARIANT)

play-web:
	@scripts/play-web.sh chess-web $(PORT) $(VARIANT)

play-spectator:
	@scripts/play-spectator.sh chess-spec $(PORT) main $(VARIANT)

build-web:
	cd clients/chess-web && env -u NO_COLOR TRUNK_COLOR=never CHESS_WEB_HOSTING=server CHESS_WEB_BASE_PATH= trunk build --release

build-web-server: build-web

build-web-static:
	cd clients/chess-web && base="$(WEB_BASE)"; base="$${base%/}"; if [ -z "$$base" ]; then base="/"; fi; public_url="$$base/"; if [ "$$base" = "/" ]; then public_url="/"; fi; env -u NO_COLOR TRUNK_COLOR=never CHESS_WEB_HOSTING=static CHESS_WEB_BASE_PATH="$$base" trunk build --release --public-url "$$public_url" --dist dist-static
	cp clients/chess-web/dist-static/index.html clients/chess-web/dist-static/404.html

serve-web-prod: build-web
	cargo run -p chess-net -- --addr $(ADDR) --static-dir clients/chess-web/dist $(VARIANT) $(SERVER_ARGS)

stop-local:
	-tmux kill-session -t $(SESSION) 2>/dev/null

stop-lobby:
	-tmux kill-session -t chess-lobby 2>/dev/null

stop-web:
	-tmux kill-session -t chess-web 2>/dev/null

stop-spectator:
	-tmux kill-session -t chess-spec 2>/dev/null

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
