all: runtime cli fetchd proxy

runtime:
	cd librt && npm run build
	cd rusty-workers-runtime && cargo build --release

cli:
	cd rusty-workers-cli && cargo build --release

fetchd:
	cd rusty-workers-fetchd && cargo build --release

proxy:
	cd rusty-workers-proxy && cargo build --release

.PHONY: runtime cli fetchd
