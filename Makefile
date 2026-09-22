default: build

test: build
	cargo test --all --tests

build:
	cargo rustc --manifest-path=pool-factory/Cargo.toml --crate-type=cdylib --target=wasm32-unknown-unknown --release
	cargo rustc --manifest-path=backstop/Cargo.toml --crate-type=cdylib --target=wasm32-unknown-unknown --release
	cargo rustc --manifest-path=pool/Cargo.toml --crate-type=cdylib --target=wasm32-unknown-unknown --release
	
	mkdir -p target/wasm32-unknown-unknown/optimized
	stellar contract optimize \
		--wasm target/wasm32-unknown-unknown/release/pool_factory.wasm \
		--wasm-out target/wasm32-unknown-unknown/optimized/pool_factory.wasm
	stellar contract optimize \
		--wasm target/wasm32-unknown-unknown/release/backstop.wasm \
		--wasm-out target/wasm32-unknown-unknown/optimized/backstop.wasm
	stellar contract optimize \
		--wasm target/wasm32-unknown-unknown/release/pool.wasm \
		--wasm-out target/wasm32-unknown-unknown/optimized/pool.wasm
	cd target/wasm32-unknown-unknown/optimized/ && \
		for i in *.wasm ; do \
			ls -l "$$i"; \
		done

# Explicit base-versus-fork differential (ADR 0008 step 8 / ADR 0011 step 5).
# Not part of `make test`: builds the pinned stock baseline from git, then runs the
# #[ignore] runner against both optimized artifact pairs. Evidence lands in
# target/adr8-differential.
ADR8_BASE_COMMIT ?= 6cf6dd1ca712ea3cf17a767fe49b7da01f80b909
ADR8_BASE_DIR = target/adr8-base
ADR8_OPT = target/wasm32-unknown-unknown/optimized

differential: build
	rm -rf $(ADR8_BASE_DIR) && mkdir -p $(ADR8_BASE_DIR)
	git archive $(ADR8_BASE_COMMIT) | tar -x -C $(ADR8_BASE_DIR)
	$(MAKE) -C $(ADR8_BASE_DIR) build
	ADR8_BASE_POOL_WASM=$(abspath $(ADR8_BASE_DIR)/$(ADR8_OPT)/pool.wasm) \
	ADR8_BASE_BACKSTOP_WASM=$(abspath $(ADR8_BASE_DIR)/$(ADR8_OPT)/backstop.wasm) \
	ADR8_FORK_POOL_WASM=$(abspath $(ADR8_OPT)/pool.wasm) \
	ADR8_FORK_BACKSTOP_WASM=$(abspath $(ADR8_OPT)/backstop.wasm) \
	ADR8_DIFF_OUTPUT_DIR=$(abspath target/adr8-differential) \
	cargo test -p test-suites --test adr8_differential -- --ignored --nocapture

fmt:
	cargo fmt --all

clean:
	cargo clean

generate-js:
	stellar contract bindings typescript --overwrite \
		--contract-id CBWH54OKUK6U2J2A4J2REJEYB625NEFCHISWXLOPR2D2D6FTN63TJTWN \
		--wasm ./target/wasm32-unknown-unknown/optimized/backstop.wasm --output-dir ./js/js-backstop/ \
		--rpc-url http://localhost:8000 --network-passphrase "Standalone Network ; February 2017" --network Standalone
	stellar contract bindings typescript --overwrite \
		--contract-id CBWH54OKUK6U2J2A4J2REJEYB625NEFCHISWXLOPR2D2D6FTN63TJTWN \
		--wasm ./target/wasm32-unknown-unknown/optimized/pool_factory.wasm --output-dir ./js/js-pool-factory/ \
		--rpc-url http://localhost:8000 --network-passphrase "Standalone Network ; February 2017" --network Standalone
	stellar contract bindings typescript --overwrite \
		--contract-id CBWH54OKUK6U2J2A4J2REJEYB625NEFCHISWXLOPR2D2D6FTN63TJTWN \
		--wasm ./target/wasm32-unknown-unknown/optimized/pool.wasm --output-dir ./js/js-pool/ \
		--rpc-url http://localhost:8000 --network-passphrase "Standalone Network ; February 2017" --network Standalone
