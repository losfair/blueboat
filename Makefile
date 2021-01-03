all: runtime cli

runtime:
	cd librt && npm run build
	cd rusty-workers-runtime && cargo build --release

cli:
	cd rusty-workers-cli && cargo build --release

.PHONY: runtime cli
