# Convenience targets for local development. The canonical build/test
# flow is plain `cargo`; this just wraps the multi-process net launcher.

SESSION ?= chess-local
PORT    ?= 7878
ADDR    ?= 127.0.0.1:$(PORT)
VARIANT ?= xiangqi
SERVER_ARGS ?=
WEB_BASE ?= /Chinese-Chess_Xiangqi

.PHONY: help build server play-local play-lobby play-web play-spectator build-web build-web-server build-web-static serve-web-prod stop-local stop-lobby stop-web stop-spectator check fmt clippy test wasm install-trunk install-wasm-target setup

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
	@echo "                trunk + wasm32-unknown-unknown — run 'make setup' first."
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
	@echo "  setup            install build prerequisites (trunk + wasm32 target)"
	@echo "  install-trunk    install trunk (prebuilt binary on macOS/Linux,"
	@echo "                   else cargo install --locked trunk)"
	@echo "  install-wasm-target  rustup target add wasm32-unknown-unknown"
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

# Local PWA verification: build the GitHub-Pages-style dist (with the
# correct base path baked in) and serve it from a static HTTP server,
# staged under the same subpath GitHub Pages would use. Without the
# stage step, `python3 -m http.server` would serve dist-static at /
# but every asset in the build expects /Chinese-Chess_Xiangqi/...,
# so nothing loads and the SW precache 404s. We mirror the layout
# by copying dist-static/ into a tempdir under WEB_BASE.
#
# `localhost` counts as a secure context, so service workers register
# and we can drive the install / offline / update flows in DevTools.
# Override WEB_BASE if you're testing a different repo subpath.
serve-web-static: build-web-static
	@base="$(WEB_BASE)"; base="$${base#/}"; base="$${base%/}"; \
	tmp=$$(mktemp -d -t chess-web-pwa-serve); \
	target="$$tmp"; if [ -n "$$base" ]; then target="$$tmp/$$base"; fi; \
	mkdir -p "$$target"; \
	cp -R clients/chess-web/dist-static/. "$$target/"; \
	echo "Staged dist-static under $$target"; \
	python3 clients/chess-web/scripts/serve-spa.py "$$tmp" 4173 "/$$base/"

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

# ----------------------------------------------------------------------------
# Toolchain prerequisites for the web build.
#
# `trunk` is the bundler used by build-web / play-web / serve-web-prod.
# `cargo install --locked trunk` works everywhere but compiles from source
# (~5–10 min). On macOS/Linux x86_64/aarch64 we fetch the official prebuilt
# tarball from github.com/trunk-rs/trunk releases (~10 s) and drop the binary
# into $$CARGO_HOME/bin (defaults to ~/.cargo/bin), which is already on PATH
# for any rustup-managed install.
#
# Override TRUNK_VERSION to pin a different release; override TRUNK_INSTALL=cargo
# to force the from-source path (e.g. on platforms without a prebuilt binary).
# ----------------------------------------------------------------------------

TRUNK_VERSION  ?= 0.21.14
TRUNK_INSTALL  ?= auto
CARGO_BIN_DIR  ?= $(or $(CARGO_HOME),$(HOME)/.cargo)/bin

setup: install-wasm-target install-trunk

install-wasm-target:
	rustup target add wasm32-unknown-unknown

install-trunk:
	@if command -v trunk >/dev/null 2>&1; then \
		echo "trunk already installed: $$(trunk --version)"; \
		exit 0; \
	fi; \
	mode="$(TRUNK_INSTALL)"; \
	uname_s=$$(uname -s); uname_m=$$(uname -m); \
	target=""; \
	if [ "$$mode" = "auto" ] || [ "$$mode" = "binary" ]; then \
		case "$$uname_s/$$uname_m" in \
			Darwin/x86_64)  target="x86_64-apple-darwin" ;; \
			Darwin/arm64)   target="aarch64-apple-darwin" ;; \
			Linux/x86_64)   target="x86_64-unknown-linux-gnu" ;; \
			Linux/aarch64)  target="aarch64-unknown-linux-gnu" ;; \
		esac; \
	fi; \
	if [ -n "$$target" ]; then \
		url="https://github.com/trunk-rs/trunk/releases/download/v$(TRUNK_VERSION)/trunk-$$target.tar.gz"; \
		echo "Installing trunk v$(TRUNK_VERSION) prebuilt binary for $$target"; \
		echo "  -> $(CARGO_BIN_DIR)/trunk"; \
		mkdir -p "$(CARGO_BIN_DIR)"; \
		tmp=$$(mktemp -d); \
		curl -fsSL --max-time 180 "$$url" -o "$$tmp/trunk.tar.gz" || { echo "download failed"; rm -rf "$$tmp"; exit 1; }; \
		tar -xzf "$$tmp/trunk.tar.gz" -C "$$tmp" || { echo "extract failed"; rm -rf "$$tmp"; exit 1; }; \
		mv "$$tmp/trunk" "$(CARGO_BIN_DIR)/trunk"; \
		chmod +x "$(CARGO_BIN_DIR)/trunk"; \
		rm -rf "$$tmp"; \
		"$(CARGO_BIN_DIR)/trunk" --version; \
	else \
		echo "No prebuilt trunk binary for $$uname_s/$$uname_m; falling back to cargo install --locked trunk"; \
		cargo install --locked trunk; \
	fi
