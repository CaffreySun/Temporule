# Temporule（拾律）唯一命令入口 —— 规范见 docs/CONVENTIONS.md §9

.PHONY: test lint verify build dev install-cli smoke

test:
	cargo test --workspace

lint:
	cargo fmt --all -- --check
	cargo clippy --all-targets -- -D warnings

verify: lint test

build:
	cd app && npm ci && npm run tauri build

dev:
	cd app && npm run tauri dev

install-cli:
	scripts/install-cli.sh

smoke:
	scripts/smoke-cli.sh
