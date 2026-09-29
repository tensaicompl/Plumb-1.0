.PHONY: check test ui-check cov mutants openapi e2e all

check:
	cargo check --workspace
	$(MAKE) ui-check

test:
	cargo test --workspace

ui-check:
	cd ui && npm run typecheck

cov:
	cargo llvm-cov --workspace --summary-only

mutants:
	cargo mutants --workspace

openapi:
	test -f api/modeller.openapi.json

e2e:
	test -f tests/e2e_hr.rs

all: check test
