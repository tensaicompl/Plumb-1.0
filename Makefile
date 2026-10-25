.PHONY: check test ui-check ui-test lint placeholders cov mutants openapi e2e all

check:
	cargo check --workspace
	$(MAKE) ui-check

# Includes the HR pilot simulation and oracle (E0.2) and the CLI/API end-to-end suites.
test:
	cargo test --workspace

ui-check:
	cd ui && npm run typecheck && npm run lint

ui-test:
	cd ui && npm test

lint:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings

placeholders:
	scripts/check-placeholders.sh

# Line coverage per crate against the E0.4 thresholds (85% core crates, 70% others).
cov:
	cargo llvm-cov --workspace --summary-only > target/coverage-summary.txt
	python3 scripts/check-coverage.py target/coverage-summary.txt

mutants:
	cargo mutants --workspace

# The committed contract must equal the Rust-generated one, and the UI client must match it.
openapi:
	cargo test -q -p plumb-api --test openapi
	cd ui && npm run api:check

# The complete HR pilot in the browser against the real backend (E0.3).
e2e:
	scripts/run-pilot-e2e.sh

all: lint check test ui-test placeholders openapi
