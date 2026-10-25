.PHONY: package check test ui-check ui-test ui-cov lint placeholders cov mutants bench openapi e2e all

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

# Vitest line/statement/function/branch coverage >= 80% for src/features and src/components
# (thresholds in ui/vite.config.ts).
ui-cov:
	cd ui && npm run test:coverage

lint:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings

placeholders:
	scripts/check-placeholders.sh

# Line coverage per crate against the E0.4 thresholds (85% core crates, 70% others).
cov:
	cargo llvm-cov --workspace --summary-only > target/coverage-summary.txt
	python3 scripts/check-coverage.py target/coverage-summary.txt

# cargo-mutants >= 80% caught for the PlumbExpr evaluator, the interpreter (plumb-sim), intake
# and the validation NOW rules (scripts/run-mutants.sh documents the scopes).
mutants:
	scripts/run-mutants.sh

# Performance release gate (E0.4 item 12): representative scale and customer-zero within the
# 120 s / 4 GiB / 4 s limits; a failure makes the matching POST.0 item a pre-release blocker.
bench:
	scripts/benchmark-representative-scale.sh

# The committed contract must equal the Rust-generated one, and the UI client must match it.
openapi:
	cargo test -q -p plumb-api --test openapi
	cd ui && npm run api:check

# The complete HR pilot in the browser against the real backend (E0.3).
e2e:
	scripts/run-pilot-e2e.sh

all: lint check test ui-cov placeholders openapi

# A local-install package in dist/ (release CLI with the Anthropic provider, built UI, scripts).
package:
	scripts/package-local.sh
