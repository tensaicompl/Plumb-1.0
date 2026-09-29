# Plumb 1.0 — Implementation Plan v3.0

**Status:** Claude Code CLI build contract  
**Plan version:** `3.0-draft.1`  
**Default execution scope:** `pilot-v3-foundation`  
**Canonical semantic model:** Plumb Specification Graph (PSG)  
**Default active profile:** `profile:plumb-software-2026.1`

This document is an implementation contract. It is intentionally prescriptive. The executing AI agent is not authorized to invent architecture, add dependencies, change scope, reinterpret standards, or fill missing product decisions.

The pilot implemented by this plan proves the v3 foundation plus gates **I0, F1, F2, F3 and F4**. The v3 semantic types needed by later compiler stages exist in the model, but stages Q1 through C1 are not implemented under the default scope.

---

## 1. Mandatory execution algorithm

Claude Code CLI SHALL follow this algorithm exactly.

1. Open repository root.
2. Read `CLAUDE.md`.
3. Read this document completely.
4. Read `docs/plan/EXECUTION-SCOPE.yaml`.
5. Read `docs/plan/PLUMB-IMPLEMENTATION-TASKS-v3.json` completely. The YAML file is a human-readable mirror.
6. Run `sha256sum -c docs/plan/SUPPORTING-DOCS.sha256`.
7. Resolve every current-task `source_refs` ID through `docs/plan/SOURCE-REFERENCE-INDEX.json`; read the identified local file and locator before editing code.
8. Read `docs/plan/EXECUTION-STATE.yaml`.
9. Select the first `pending` task in manifest order whose `scope` is allowed and whose dependencies are all `done`.
10. Execute **only that task**.
11. Modify only files listed in that task, plus:
    - `Cargo.lock` or `ui/package-lock.json` only in task `P0.2`;
    - `docs/plan/EXECUTION-STATE.yaml`;
    - a blocker file under `docs/blockers/` when blocked;
    - generated artifact files explicitly named by the task.
12. Run every command and test listed by the task.
13. Verify every acceptance condition.
14. If successful, mark the task `done` in `EXECUTION-STATE.yaml` and commit exactly with the task's `commit_message`.
15. Stop after one task unless the human invocation explicitly says to continue through multiple ready tasks.
16. Never execute a `LATER` task while `EXECUTION-SCOPE.yaml` allows only `NOW`.

A failed test is not permission to change the specification. A missing specification is not permission to infer a default.

---

## 2. Absolute no-guess policy

The following behavior is forbidden:

- inventing a field, enum value, relation, API endpoint, rule ID, workflow step, dependency, test expectation, requirement, architecture choice or product behavior;
- silently choosing between contradictory supporting documents;
- adding a dependency because it is convenient;
- replacing a specified library or format with an alternative;
- weakening a test to make code pass;
- auto-accepting LLM semantic output;
- treating an LLM response as source evidence;
- treating a readiness percentage as a gate pass;
- using requirement document order as accepted process order;
- treating `functional.yaml` as canonical state;
- collapsing `BusinessRole` and `SecurityRole`;
- fabricating missing scenario values instead of returning `Undecidable`;
- applying an asynchronous proposal whose base revision is stale.

When a required decision is genuinely absent or contradictory, create `docs/blockers/<TASK_ID>.md` from `docs/plan/BLOCKER-TEMPLATE.md`, mark the task `blocked` in `EXECUTION-STATE.yaml`, and stop. Do not continue to the next task.

Core governance statements:

- **PSG is canonical.** `functional.yaml`, diagrams, reports and API descriptions are projections or contracts over PSG.
- **LLM output is a persisted proposal artifact.** It has no direct authority to mutate accepted PSG state.
- **Stale async proposals cannot commit.** Base revision/hash comparison is mandatory before application.


---

## 3. Authoritative documents and precedence

All paths below are repository-relative and mandatory.

| Domain | Authoritative source | Rule |
|---|---|---|
| Task order, exact files, commands, acceptance | `docs/plan/PLUMB-IMPLEMENTATION-PLAN-v3.md` and `docs/plan/PLUMB-IMPLEMENTATION-TASKS-v3.json` | They must agree. JSON is machine authority; YAML is a human-readable mirror. Any disagreement is `BLOCKED-SPEC-CONFLICT`. |
| Semantic types, relations, invariants, hashes, views, patch model | `docs/architecture/PLUMB-METAMODEL-v3.md` | This source wins for semantic definitions. |
| Compiler stage boundary, AI/artifact boundary, replay, concurrency | `docs/architecture/PLUMB-COMPILER-ARCHITECTURE-v3.md` | This source wins for compiler execution semantics. |
| Rule IDs, gate, severity, class, waiver policy | `config/profiles/plumb-software-2026.1-rules.yaml` | Machine metadata is exact and must not be rewritten. |
| Rule rationale/pass semantics | `docs/standards/PLUMB-VALIDATION-RULEBOOK-2026.1.md` | If it materially contradicts the YAML, stop with a spec-conflict blocker. |
| v2→v3 migration decisions | `docs/plan/PLUMB-v2-to-v3-IMPLEMENTATION-DELTA.md` | Use to resolve known v2 design changes. |
| Inherited pilot behavior and endpoint/UI inventory | `docs/plan/merged-implementation-plan-v2.md` | Use only where this plan explicitly says to inherit behavior. It never overrides v3 semantics. |
| HR pilot test semantics | `fixtures/hr-leave/fixture-contract.yaml` plus the other checksummed fixture inputs in that directory | These inputs are immutable during implementation. Derived reference artifacts must conform to them. |
| Machine summary of compiler pipeline | `config/compiler/plumb-compiler-pipeline-2026.1.yaml` | Consistency aid only; compiler architecture Markdown is normative on conflict. |
| Machine-resolvable supporting references | `docs/plan/SOURCE-REFERENCE-INDEX.json` | Every task `source_refs` entry must resolve here; free-form task references are forbidden. YAML is a human-readable mirror. |

No external website, blog, README, package example or model memory may override these documents.

---

## 4. Scope boundary

`docs/plan/EXECUTION-SCOPE.yaml` is authoritative for which tasks may run.

Default scope implements:

- typed PSG and all v3 NodePayload types;
- immutable revisions and branch heads;
- semantic patches;
- evidence/provenance/inference artifacts;
- standards profile and deterministic validation engine;
- `functional.yaml` as compatibility projection;
- source ingestion and requirement compilation;
- functional/domain/process/rule/calculation/authorization semantics;
- question/resolution workflow;
- functional scenarios and deterministic interpreter;
- gates I0/F1/F2/F3/F4;
- patch-centric intake and chat sessions;
- v2 pilot API/CLI/UI behavior backed by PSG;
- HR pilot and deterministic replay.

Default scope explicitly does **not** implement production behavior for Q1/A1/A2/A3/A4/D1/D2/C1, architecture candidate generation, OpenAPI/AsyncAPI/Arazzo reconciliation, BPMN/DMN XML interchange, SysML v2 API integration, ArchiMate/TOGAF integration, code binding, proof receipts, implementation slicing, or C1 conformance.

### 4.1 Authoritative HR pilot fixture

The v2 plan supplied fixture counts and named ambiguities but not the exact source content. This build contract therefore fixes the missing test input in the checksummed files under `fixtures/hr-leave/`.

Claude must not rewrite these files to make implementation tests pass.

`fixture-contract.yaml` is the semantic oracle. It fixes:

- all 32 requirement statements;
- the 9 entities and their attributes;
- 13 operations and their read/write/outcome semantics;
- 6 processes;
- 5 invariants;
- 2 calculations;
- 3 decision/business rules;
- 6 events;
- business/security role reference data;
- exact answers to the three deliberately unresolved ambiguities;
- exact expected counts.

`scenarios.yaml` fixes all 44 scenario contracts and pre-/post-answer expected results.


---

## 5. Dependency policy

Only the packages below may exist in the pilot dependency graph. Versions/features in the Rust block are an explicit plan decision made to eliminate agent choice. If dependency resolution fails, the correct result is `BLOCKED-DEPENDENCY`; Claude must not substitute another crate or version.

### 5.0 Exact build environment

The pilot build baseline is:

- Rust `1.98.1` with `rustfmt`, `clippy`, `llvm-tools-preview`.
- Rust edition `2021`.
- Node.js `24.21.0` LTS.
- npm `11.19.0`.
- Native Linux build toolchain: a working C compiler/linker available through the command `cc`. On Debian/Ubuntu environments this requirement is satisfied by the `build-essential` package. P0.2 must verify `cc --version` succeeds before Cargo compilation. If `cc` is unavailable, the task is `BLOCKED-ENVIRONMENT`.
- GitHub Actions runner `ubuntu-24.04`.

Local execution with a different Node/npm version is not authorized. Claude creates a `BLOCKED-ENVIRONMENT` record instead of silently using another runtime. Rust is selected by the committed `rust-toolchain.toml`.

### 5.1 Exact Rust workspace dependencies

Root `Cargo.toml` SHALL contain this dependency block exactly:

```toml
[workspace.dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
serde_yaml = "0.9"
serde_json_canonicalizer = "0.3.2"
jsonschema = { version = "0.18", default-features = false }
rusqlite = { version = "0.32", features = ["bundled"] }
petgraph = "0.6"
rand = "0.8"
rand_chacha = "0.3"
pest = "2.7"
pest_derive = "2.7"
rust_decimal = { version = "1.36", features = ["serde"] }
time = { version = "0.3", features = ["serde", "formatting", "parsing", "macros"] }
unicode-normalization = "0.1"
regex = "1.10"
zip = "2.2"
quick-xml = { version = "0.37", features = ["serialize"] }
thiserror = "2.0"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
axum = { version = "0.8", features = ["ws", "multipart"] }
tokio = { version = "1.0", features = ["rt-multi-thread", "macros", "signal", "sync", "fs"] }
tower-http = { version = "0.6", features = ["fs", "trace", "cors"] }
utoipa = { version = "5.0", features = ["axum_extras"] }
utoipa-swagger-ui = { version = "9.0", features = ["axum"] }
clap = { version = "4.5", features = ["derive"] }
sha2 = "0.10"
hex = "0.4"
ulid = { version = "1.1", features = ["serde"] }
reqwest = { version = "0.12", default-features = false, features = ["blocking", "json", "rustls-tls"] }

[workspace.dependencies.insta]
version = "1.41"
features = ["yaml", "json"]

[workspace.dependencies.proptest]
version = "1.6"

[workspace.dependencies.criterion]
version = "0.5"

[workspace.dependencies.assert_cmd]
version = "2.0"

[workspace.dependencies.predicates]
version = "3.1"

[workspace.dependencies.tempfile]
version = "3.10"
```

Each crate uses `{ workspace = true }` for the dependencies it needs. It must not declare a different version or feature set.

`cargo-llvm-cov` and `cargo-mutants` are CLI quality tools, not Cargo dependencies. Versions are pinned:

```bash
cargo install cargo-llvm-cov --version 0.9.1 --locked
cargo install cargo-mutants --version 27.1.0 --locked
```

### 5.2 Exact UI runtime dependency versions

The scaffold task SHALL run one install command containing exactly these exact-version runtime packages:

```bash
npm install --save-exact --strict-peer-deps --strict-allow-scripts react@19.3.0 react-dom@19.3.0 @tanstack/react-router@1.170.40 @tanstack/react-query@5.104.0 zustand@5.0.15 @radix-ui/react-dialog@1.1.23 @radix-ui/react-dropdown-menu@2.1.24 @radix-ui/react-tabs@1.1.21 @radix-ui/react-tooltip@1.2.16 @radix-ui/react-popover@1.1.23 @radix-ui/react-select@2.3.7 @radix-ui/react-checkbox@1.3.11 @radix-ui/react-radio-group@1.4.7 @radix-ui/react-toast@1.2.23 tailwindcss@4.3.3 @tailwindcss/vite@4.3.3 elkjs@0.12.0 openapi-fetch@0.17.0 date-fns@4.4.0 zod@4.6.5
```

### 5.3 Exact UI development dependency versions

```bash
npm install --save-dev --save-exact --strict-peer-deps --strict-allow-scripts typescript@5.9.3 vite@8.3.1 vitest@5.0.2 jsdom@30.1.1 @types/react@19.3.0 @types/react-dom@19.3.0 @types/node@26.6.3 @testing-library/react@16.3.3 @testing-library/dom@10.4.2 @testing-library/user-event@14.6.7 msw@2.15.0 openapi-typescript@7.13.0 playwright@1.63.0 @axe-core/playwright@4.13.0 eslint@9.39.5 typescript-eslint@8.71.0 eslint-plugin-jsx-a11y@6.10.2 eslint-plugin-react-hooks@7.1.1 prettier@3.9.9
```

P0.2 resolves no direct JavaScript dependency versions dynamically. Direct runtime and development dependency versions are fixed by this plan. Transitive versions are frozen by ui/package-lock.json. No direct version or lockfile may be changed after P0.2 except through a human-approved plan revision.

The resulting `ui/package-lock.json` is committed by `P0.2` and is authoritative thereafter. No later task may run `npm update`, change package versions, or add another package.

`@stoplight/prism-cli` is not an allowed Plumb dependency; the v3 build does not use Prism. MSW is the approved API mocking dependency for the pilot.

No other Rust or JavaScript dependency is permitted.

### 5.3.1 JavaScript dependency installation policy

All P0.2 npm installation commands use `--save-exact --strict-peer-deps --strict-allow-scripts`. `ui/package.json` declares the install-script policy `"allowScripts": {"msw": false}`; MSW's postinstall worker synchronization is not required by the P0.2 skeleton and must not execute.

A P0.2 JavaScript dependency installation is blocking on:

- npm `ERESOLVE` or peer dependency conflict;
- unsupported Node/npm engine;
- package integrity/checksum failure;
- an install script that is neither explicitly allowed nor explicitly denied;
- npm audit finding of HIGH or CRITICAL severity;
- install command, `npm ci`, `npm ls --depth=0`, `npm audit --audit-level=high` or `npm run typecheck` returning non-zero.

The following are non-blocking but must be reported:

- deprecation notices for dependencies in the exact human-approved frozen graph;
- funding notices;
- ordinary package-maintenance notices;
- LOW or MODERATE npm audit findings.

`eslint@9.39.5` is an explicitly approved temporary pilot dev-tool version despite its deprecation/EOL notice.

Do not use `--ignore-scripts`, `--force`, `--legacy-peer-deps`, `--dangerously-allow-all-scripts` or npm overrides. No warning may be suppressed merely to obtain a successful installation.

### 5.4 Exact Rust crate dependency matrix

Crate `Cargo.toml` files SHALL contain only the path/workspace dependencies below. A package listed in §5.1 but absent from a crate row may not be added to that crate.

| Crate | Path dependencies | Workspace runtime dependencies | Optional | Dev dependencies |
|---|---|---|---|---|
| `plumb-core` | — | `serde`, `serde_json`, `serde_json_canonicalizer`, `sha2`, `hex`, `time`, `thiserror` | — | `proptest` |
| `plumb-artifacts` | `plumb-core` | `serde`, `rusqlite`, `time`, `thiserror` | — | `tempfile` |
| `plumb-psg` | `plumb-core` | `serde`, `serde_json`, `petgraph`, `thiserror` | — | `proptest`, `insta` |
| `plumb-patch` | `plumb-core`, `plumb-psg` | `serde`, `serde_json`, `petgraph`, `thiserror` | — | `proptest` |
| `plumb-store` | `plumb-core`, `plumb-artifacts`, `plumb-psg`, `plumb-patch` | `serde`, `serde_json`, `rusqlite`, `time`, `thiserror` | — | `tempfile`, `proptest` |
| `plumb-validation` | `plumb-core`, `plumb-artifacts`, `plumb-psg` | `serde`, `serde_json`, `serde_yaml`, `sha2`, `hex`, `thiserror` | — | `insta` |
| `plumb-inference` | `plumb-core`, `plumb-artifacts` | `serde`, `serde_json`, `jsonschema`, `sha2`, `thiserror` | `reqwest` | `tempfile` |
| `plumb-compiler` | `plumb-core`, `plumb-artifacts`, `plumb-psg`, `plumb-patch`, `plumb-validation`, `plumb-inference` | `serde`, `serde_json`, `thiserror` | — | — |
| `plumb-import` | `plumb-core`, `plumb-artifacts`, `plumb-psg`, `plumb-inference` | `serde`, `serde_json`, `regex`, `unicode-normalization`, `zip`, `quick-xml`, `thiserror` | — | `tempfile`, `insta` |
| `plumb-lint` | `plumb-core`, `plumb-psg` | `serde`, `regex`, `thiserror` | — | `insta` |
| `plumb-expr` | `plumb-core` | `serde`, `pest`, `pest_derive`, `rust_decimal`, `time`, `thiserror` | — | `proptest`, `insta` |
| `plumb-functional` | `plumb-core`, `plumb-psg`, `plumb-patch`, `plumb-validation`, `plumb-inference`, `plumb-expr` | `serde`, `serde_json`, `regex`, `unicode-normalization`, `rust_decimal`, `time`, `thiserror` | — | `insta`, `proptest` |
| `plumb-sim` | `plumb-core`, `plumb-psg`, `plumb-expr`, `plumb-functional` | `serde`, `serde_json`, `thiserror` | — | `proptest`, `insta` |
| `plumb-intake` | `plumb-core`, `plumb-psg`, `plumb-patch`, `plumb-validation` | `serde`, `serde_json`, `regex`, `thiserror` | — | `proptest` |
| `plumb-session` | `plumb-core`, `plumb-artifacts`, `plumb-psg`, `plumb-patch`, `plumb-inference`, `plumb-intake`, `plumb-compiler` | `serde`, `serde_json`, `thiserror` | — | `insta` |
| `plumb-api` | `plumb-core`, `plumb-artifacts`, `plumb-psg`, `plumb-patch`, `plumb-store`, `plumb-validation`, `plumb-inference`, `plumb-compiler`, `plumb-import`, `plumb-functional`, `plumb-sim`, `plumb-intake`, `plumb-session` | `serde`, `serde_json`, `axum`, `tokio`, `tower-http`, `utoipa`, `tracing`, `thiserror` | — | `tempfile` |
| `plumb-cli` | `plumb-core`, `plumb-store`, `plumb-validation`, `plumb-inference`, `plumb-compiler`, `plumb-import`, `plumb-functional`, `plumb-sim`, `plumb-intake`, `plumb-session`, `plumb-api` | `serde_json`, `clap`, `tracing`, `tracing-subscriber`, `thiserror` | — | `assert_cmd`, `predicates`, `tempfile` |

Feature declarations are exact:

```toml
# crates/plumb-inference/Cargo.toml
[features]
default = []
anthropic = ["dep:reqwest"]

# crates/plumb-api/Cargo.toml
[features]
default = []
llm = ["plumb-inference/anthropic"]

# crates/plumb-cli/Cargo.toml
[features]
default = []
llm = ["plumb-inference/anthropic", "plumb-api/llm"]
```

All other crates have `default = []` and no feature flag unless a later human-approved plan revision adds one.

---

## 6. Canonical persistence schema for the pilot

The pilot uses one SQLite database. Initialization SHALL execute the following logical schema. Column types/names may not be renamed by the implementation.

```sql
CREATE TABLE schema_meta (
  version INTEGER NOT NULL
);

CREATE TABLE artifacts (
  hash TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  media_type TEXT NOT NULL,
  bytes BLOB NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE graph_revisions (
  id TEXT PRIMARY KEY,
  version INTEGER NOT NULL UNIQUE,
  parent_id TEXT NULL,
  semantic_hash TEXT NOT NULL,
  evidence_hash TEXT NOT NULL,
  profile_ref TEXT NOT NULL,
  profile_hash TEXT NOT NULL,
  rule_pack_hash TEXT NOT NULL,
  patch_artifact_hash TEXT NULL,
  decision_refs_json TEXT NOT NULL,
  created_by TEXT NOT NULL,
  created_at TEXT NOT NULL
);

CREATE TABLE revision_nodes (
  revision_id TEXT NOT NULL,
  node_id TEXT NOT NULL,
  node_json TEXT NOT NULL,
  PRIMARY KEY (revision_id, node_id),
  FOREIGN KEY (revision_id) REFERENCES graph_revisions(id)
);

CREATE TABLE revision_edges (
  revision_id TEXT NOT NULL,
  edge_id TEXT NOT NULL,
  edge_json TEXT NOT NULL,
  PRIMARY KEY (revision_id, edge_id),
  FOREIGN KEY (revision_id) REFERENCES graph_revisions(id)
);

CREATE TABLE branch_heads (
  name TEXT PRIMARY KEY,
  revision_id TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  FOREIGN KEY (revision_id) REFERENCES graph_revisions(id)
);

CREATE TABLE ledger_events (
  seq INTEGER PRIMARY KEY AUTOINCREMENT,
  revision_id TEXT NULL,
  event_json TEXT NOT NULL
);
```

Connection initialization SHALL enable:

```sql
PRAGMA foreign_keys = ON;
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
```

The pilot stores a full node/edge snapshot per revision. Do not substitute delta storage.

---

## 6.1 Exact pilot identifier and hash policy

All hashes use lowercase SHA-256 hex.

- Artifact hash: `sha256:<64 hex>`.
- Semantic hash: `psg:sha256:<64 hex>`.
- Evidence hash: `ev:sha256:<64 hex>`.
- Source ID: `src:<first16 hex of source content SHA-256>`.
- EvidenceFragment ID: `evd:<first16 hex of SHA-256(source_id || "|" || canonical locator JSON)>`.
- Requirement with a source identifier matching `[A-Za-z0-9._-]+`: `req:<source_identifier>`. Otherwise use the compiler-derived rule below.
- Finding ID: `fnd:<first16 hex of finding_key>`, where finding_key is defined by the validation rulebook.
- Question ID: `q:<first16 hex of SHA-256(finding_id || "|" || template_id || "|" || canonical slots JSON)>`.
- Proposal ID: `prop:<first16 hex of SHA-256(base_semantic_hash || "|" || stage_id || "|" || canonical patch JSON)>`.
- ResolutionDecision ID: `dec:<first16 hex of SHA-256(question_or_proposal_ref || "|" || canonical answer JSON || "|" || decided_by || "|" || RFC3339 decision timestamp)>`.
- Revision version: global positive integer, starting at 1 and incrementing by 1 for every successful new revision.
- Revision ID: `rev:<version>:<first16 hex from semantic_hash>`.
- Session ID: `ses:<first16 hex of SHA-256(first_user_turn_evidence_id || "|" || RFC3339 creation timestamp)>`.

For any compiler-derived semantic node that has no explicit source identifier:

```text
<prefix>:<first16 SHA-256(project_id | node_type | primary_evidence_id | creation_semantic_key)>
```

`creation_semantic_key` is fixed as follows:

- named node types: the NFKC-normalized, trimmed, case-preserving initial `name`;
- `Attribute`: owning Entity ID + `|` + initial normalized attribute name;
- `State`: owner ID + `|` + initial normalized state name;
- `Transition`: stateful ID + `|` + from-state ID + `|` + to-state ID + `|` + trigger ID;
- `Scenario`: first sorted requirement reference + `|` + operation reference + `|` + scenario kind + `|` + zero-based deterministic ordinal within that derivation batch;
- types without one of the definitions above: canonical JSON of the required payload fields defined by the metamodel.

IDs are assigned once on creation and are never recomputed after rename/edit.

### 6.2 Exact semantic and evidence hash input

`semantic_hash` canonicalizes:

1. project ID and active profile ID;
2. every node whose status is `Accepted`, `Suspect`, `Superseded` or `Deprecated`;
3. every edge whose status is `Accepted`, `Suspect`, `Superseded` or `Deprecated`;
4. for those nodes/edges: ID, status, payload, evidence references, standards mappings, tags and semantic extensions.

It excludes:

- `Proposed` and `Rejected` nodes/edges;
- audit timestamps/actors;
- `DerivationRecord` and inference artifact bytes;
- compile/job/session operational metadata;
- view layout/style coordinates;
- cached projections.

`evidence_hash` canonicalizes the sorted `(SourceArtifact ID, source content hash)` set plus sorted `(EvidenceFragment ID, source_ref, locator, fragment content hash)` set.

If an implementation choice would change these hash inputs, stop with `BLOCKED-SPEC-CONFLICT`; do not improvise.

### 6.3 Exact core primitive contracts

These primitive contracts are normative for `plumb-core` (task `F0.1`) and for every structure that uses `Id`, `Hash` or `Timestamp`.

**`Id`** is a validated string newtype. The exact accepted grammar is:

```regex
^[a-z][a-z0-9_-]*:[A-Za-z0-9._-]+(?::[A-Za-z0-9._-]+)*$
```

- the namespace begins with lowercase `[a-z]`; remaining namespace characters are lowercase letters, digits, `_`, `-`;
- at least one body segment is mandatory; body segment characters are `[A-Za-z0-9._-]+`;
- additional non-empty body segments may be separated by `:`; empty segments are invalid;
- whitespace is invalid; leading/trailing whitespace is rejected, not trimmed;
- parsing performs no normalization; the exact validated string is preserved;
- there is no F0.1 maximum length constraint.

Examples that MUST validate:

```text
src:0123456789abcdef
req:HR-001
req:hr:leave-request
rev:42:0123456789abcdef
project:leave-management
profile:plumb-software-2026.1
resolution:01K123456789ABCDEFGHJKMNPQ
```

Examples that MUST fail (the first is the empty string; two contain a leading or trailing space):

```text
""
req
Req:ABC
req:
req::ABC
req:ABC:
req:ABC DEF
 req:ABC
req:ABC 
req:ABC/DEF
```

`Id` serialization and deserialization preserve validation: deserializing an invalid string fails rather than constructing an invalid `Id`. `Id` provides validated `FromStr` and `TryFrom<String>`, plus `Display`, `AsRef<str>` and `as_str()`.

**`Hash`** is one validated string newtype accepting exactly three forms:

```regex
^sha256:[0-9a-f]{64}$
^psg:sha256:[0-9a-f]{64}$
^ev:sha256:[0-9a-f]{64}$
```

| Prefix | Meaning | `HashKind` |
|---|---|---|
| `sha256:` | Generic/content/artifact/config/rule/output hash | `Generic` |
| `psg:sha256:` | PSG semantic hash | `Semantic` |
| `ev:sha256:` | Evidence hash | `Evidence` |

`Hash` provides `pub enum HashKind { Generic, Semantic, Evidence }`, `Hash::kind() -> HashKind`, and the byte-hashing constructors `Hash::content_sha256(bytes: &[u8]) -> Hash`, `Hash::semantic_sha256(bytes: &[u8]) -> Hash` and `Hash::evidence_sha256(bytes: &[u8]) -> Hash`. All SHA-256 hex output is lowercase. `Hash` provides validated `FromStr`, `TryFrom<String>`, `Display`, `AsRef<str>` and `as_str()`; serde deserialization validates. Uppercase digest hex, incorrect digest length, unknown prefixes, whitespace and additional trailing material are rejected. No separate `SemanticHash` or `EvidenceHash` field types exist in F0.1; compiler structures use `Hash`.

**`Timestamp`** is `pub struct Timestamp(time::OffsetDateTime);`:

- every stored `Timestamp` is normalized to UTC;
- parsing/deserialization accepts valid RFC 3339 timestamps; non-UTC offsets are converted to the equivalent UTC instant;
- `T` is accepted as the date/time separator; a single ASCII space separator accepted by the RFC 3339 profile/parser is also accepted;
- surrounding whitespace is rejected;
- nanosecond precision is retained;
- `Timestamp` values whose UTC-normalized year is outside `0000..=9999` are rejected, because the canonical representation requires exactly four decimal year digits; normalization that would leave that range is a rejection, never a panic;
- `Display` and serialization use one canonical form: UTC with uppercase `T`, uppercase `Z` and exactly nine fractional-second digits, e.g. `2026-09-29T12:34:56.123456789Z`; an accepted space-separated input serializes back using `T`;
- `2026-09-29T14:34:56.123456789+02:00` canonicalizes to `2026-09-29T12:34:56.123456789Z`.

**`Clock`** has `fn now(&self) -> Timestamp;`. `SystemClock` is the only F0.1 implementation permitted to read system time. `FixedClock` is constructed with a `Timestamp` and always returns that same timestamp. No other F0.1 code may access wall-clock time directly.

**Canonical JSON.** The F0.1 canonical JSON primitive applies RFC 8785 JCS through `serde_json_canonicalizer` (`serde_json_canonicalizer::to_vec`) to the value supplied to it. The semantic contract is cross-language RFC 8785 conformance; the dependency is an implementation of that contract, and a canonicalizer whose output differs from RFC 8785 is a defect, not an accepted deviation. F0.1 does not implement Plumb domain-specific ordering of semantically unordered arrays; that ordering is applied by later semantic-envelope construction before calling the canonical JSON primitive. No approximate/custom JCS serializer is permitted.

RFC 8785 conformance fixtures. Each line is `input JSON text` → `exact canonical output`. The outputs are fixed RFC 8785 results derived from RFC 8785 §3.2 (UTF-16 code-unit key ordering, minimal string escaping, ECMAScript number serialization); tests compare against these literal bytes, never against values generated by the implementation under test. Canonicalizing any expected output again must return it unchanged. In expected outputs, `<U+XXXX>` denotes that single character emitted as raw UTF-8, not an escape sequence.

```text
ASCII keys:           {"b":1,"a":2,"aa":3,"B":4}                → {"B":4,"a":2,"aa":3,"b":1}
quote/backslash keys: {"b\\":1,"b\"":2,"b":3}                   → {"b":3,"b\"":2,"b\\":1}
control-char keys:    {"A":1,"\n":2,"\u001f":3}                 → {"\n":2,"\u001f":3,"A":1}
BMP keys:             {"z":1,"\u00e9":2,"\u20ac":3}              → {"z":1,"é":2,"€":3}
non-BMP keys:         {"\ufb00":1,"\ud83d\ude00":2}              → {"😀":2,"ﬀ":1}
string escaping:      {"s":"\u001f\u007f/\u00e9\u2028"}          → {"s":"\u001f<U+007F>/é<U+2028>"}
numbers:              [1.0,2.50,1e21,-0.0,1e-7,100,0.000001]    → [1,2.5,1e+21,0,1e-7,100,0.000001]
```

## 7. v3 overrides to the v2 plan

These overrides are mandatory wherever v2 text differs.

1. `PSG` is canonical; `functional.yaml` is a projection.
2. Core semantics use typed `NodePayload`; arbitrary `kind:String + props` is not the product model.
3. `Origin` is insufficient provenance; use `DerivationRecord`, `Agent`, `InferenceRequest` and `InferenceArtifact`.
4. `Box<dyn Patch>` is forbidden; use serialized `SemanticPatch`.
5. Stage purity means `plan()` and `evaluate()` are pure. Network/model/validator execution is artifact acquisition outside the stage.
6. Reproducibility uses persisted inference/validation artifacts. It never requires recalling a live model.
7. Async jobs capture base revision/hash and stale results cannot commit.
8. Store semantics are immutable revisions plus branch-head CAS.
9. `BusinessRole` and `SecurityRole` are distinct.
10. Canonical NFR semantics are `QualityCharacteristic`, `Measure`, `QualityScenario`; legacy NFR bags exist only in compatibility projection.
11. Requirement document order is not accepted process order.
12. F4 does not require a passing scenario for every requirement. Verification method is applicability-aware.
13. Readiness is advisory; deterministic gate rules decide pass/fail.
14. Pagination responses use `{items,next_cursor}`.
15. HTTP writes use `If-Match` with `semantic_hash`.
16. LLM provider returns artifacts and never owns graph-write capability.

---

## 8. API compatibility overrides

Implement every v2 P/P-R endpoint in `docs/plan/merged-implementation-plan-v2.md §5`, with only these v3 contract changes:

- `GET /api/model/version` returns:
  ```json
  {
    "version": 42,
    "hash": "psg:sha256:...",
    "revision": "rev:...",
    "semantic_hash": "psg:sha256:...",
    "evidence_hash": "ev:sha256:..."
  }
  ```
  `version` is compatibility sequence number and `hash == semantic_hash`.

- `GET /api/gates` returns only `I0,F1,F2,F3,F4` under default scope, in that order.

- List endpoints return:
  ```json
  {"items": [], "next_cursor": null}
  ```

  A non-null cursor is the lowercase hex encoding of UTF-8 RFC-8785 canonical JSON:
  ```json
  {"after_id":"<last-returned-id>","semantic_hash":"psg:sha256:..."}
  ```
  A cursor whose semantic_hash is not the current read baseline returns HTTP 409 with code `E_CURSOR_BASELINE_CHANGED`.

- `WS /api/events` payload is exactly:
  ```json
  {
    "kind": "model.changed",
    "ids": ["..."],
    "version": 42,
    "revision": "rev:42:...",
    "semantic_hash": "psg:sha256:..."
  }
  ```

- Every write requires `If-Match: <semantic_hash>`. Wrong hash returns HTTP 412 with:
  ```json
  {
    "code": "E_VERSION_MISMATCH",
    "message": "Model baseline changed",
    "details": {
      "revision": "rev:...",
      "semantic_hash": "psg:sha256:..."
    }
  }
  ```

- Stage job responses include `base_revision` and status may be `stale`.

- Question answers return proposal/intake/patch preview; acceptance is a separate proposal action.

No other endpoint change is authorized.

---

## 9. Target repository paths

The plan intentionally names source files instead of allowing the agent to invent module names. The complete set of production/generated/fixture paths controlled by this build contract is:

- `.github/workflows/ci.yml`
- `.gitignore`
- `.node-version`
- `Cargo.lock`
- `Cargo.toml`
- `Makefile`
- `api/modeller.openapi.json`
- `config/profiles/plumb-software-2026.1-rules.yaml`
- `crates/plumb-api/Cargo.toml`
- `crates/plumb-api/src/error.rs`
- `crates/plumb-api/src/lib.rs`
- `crates/plumb-api/src/openapi.rs`
- `crates/plumb-api/src/routes/gates.rs`
- `crates/plumb-api/src/routes/jobs.rs`
- `crates/plumb-api/src/routes/meta.rs`
- `crates/plumb-api/src/routes/mod.rs`
- `crates/plumb-api/src/routes/model.rs`
- `crates/plumb-api/src/routes/proposals.rs`
- `crates/plumb-api/src/routes/questions.rs`
- `crates/plumb-api/src/routes/requirements.rs`
- `crates/plumb-api/src/routes/scenarios.rs`
- `crates/plumb-api/src/routes/sessions.rs`
- `crates/plumb-api/src/routes/sources.rs`
- `crates/plumb-api/src/state.rs`
- `crates/plumb-api/tests/http_core.rs`
- `crates/plumb-api/tests/http_workflow.rs`
- `crates/plumb-api/tests/openapi.rs`
- `crates/plumb-artifacts/Cargo.toml`
- `crates/plumb-artifacts/src/lib.rs`
- `crates/plumb-artifacts/src/model.rs`
- `crates/plumb-artifacts/src/sqlite.rs`
- `crates/plumb-artifacts/src/store.rs`
- `crates/plumb-artifacts/tests/store.rs`
- `crates/plumb-cli/Cargo.toml`
- `crates/plumb-cli/src/commands.rs`
- `crates/plumb-cli/src/main.rs`
- `crates/plumb-cli/tests/cli.rs`
- `crates/plumb-compiler/Cargo.toml`
- `crates/plumb-compiler/src/context.rs`
- `crates/plumb-compiler/src/job.rs`
- `crates/plumb-compiler/src/lib.rs`
- `crates/plumb-compiler/src/run.rs`
- `crates/plumb-compiler/src/stage.rs`
- `crates/plumb-compiler/tests/job.rs`
- `crates/plumb-compiler/tests/stage.rs`
- `crates/plumb-core/Cargo.toml`
- `crates/plumb-core/src/canonical.rs`
- `crates/plumb-core/src/clock.rs`
- `crates/plumb-core/src/error.rs`
- `crates/plumb-core/src/hash.rs`
- `crates/plumb-core/src/id.rs`
- `crates/plumb-core/src/lib.rs`
- `crates/plumb-core/tests/canonical.rs`
- `crates/plumb-core/tests/hash.rs`
- `crates/plumb-expr/Cargo.toml`
- `crates/plumb-expr/src/ast.rs`
- `crates/plumb-expr/src/calendar.rs`
- `crates/plumb-expr/src/eval.rs`
- `crates/plumb-expr/src/grammar.pest`
- `crates/plumb-expr/src/lib.rs`
- `crates/plumb-expr/src/parser.rs`
- `crates/plumb-expr/src/render.rs`
- `crates/plumb-expr/src/typecheck.rs`
- `crates/plumb-expr/src/types.rs`
- `crates/plumb-expr/tests/eval.rs`
- `crates/plumb-expr/tests/parser.rs`
- `crates/plumb-functional/Cargo.toml`
- `crates/plumb-functional/src/assumption.rs`
- `crates/plumb-functional/src/authorization.rs`
- `crates/plumb-functional/src/calculation.rs`
- `crates/plumb-functional/src/data_class.rs`
- `crates/plumb-functional/src/decision_table.rs`
- `crates/plumb-functional/src/domain.rs`
- `crates/plumb-functional/src/duplicates.rs`
- `crates/plumb-functional/src/ears.rs`
- `crates/plumb-functional/src/event.rs`
- `crates/plumb-functional/src/intent.rs`
- `crates/plumb-functional/src/invariant.rs`
- `crates/plumb-functional/src/lib.rs`
- `crates/plumb-functional/src/model.rs`
- `crates/plumb-functional/src/operation.rs`
- `crates/plumb-functional/src/process.rs`
- `crates/plumb-functional/src/project.rs`
- `crates/plumb-functional/src/projections.rs`
- `crates/plumb-functional/src/question.rs`
- `crates/plumb-functional/src/question_templates.rs`
- `crates/plumb-functional/src/report.rs`
- `crates/plumb-functional/src/requirements.rs`
- `crates/plumb-functional/src/resolution.rs`
- `crates/plumb-functional/src/round.rs`
- `crates/plumb-functional/src/scenario.rs`
- `crates/plumb-functional/src/state.rs`
- `crates/plumb-functional/src/verification.rs`
- `crates/plumb-functional/src/vocabulary.rs`
- `crates/plumb-functional/src/vocabulary_exceptions.rs`
- `crates/plumb-functional/tests/assumption.rs`
- `crates/plumb-functional/tests/authorization.rs`
- `crates/plumb-functional/tests/calculation.rs`
- `crates/plumb-functional/tests/data_class.rs`
- `crates/plumb-functional/tests/decision_table.rs`
- `crates/plumb-functional/tests/domain.rs`
- `crates/plumb-functional/tests/duplicates.rs`
- `crates/plumb-functional/tests/ears.rs`
- `crates/plumb-functional/tests/golden.rs`
- `crates/plumb-functional/tests/operation.rs`
- `crates/plumb-functional/tests/process.rs`
- `crates/plumb-functional/tests/projections.rs`
- `crates/plumb-functional/tests/question.rs`
- `crates/plumb-functional/tests/requirements.rs`
- `crates/plumb-functional/tests/resolution.rs`
- `crates/plumb-functional/tests/round.rs`
- `crates/plumb-functional/tests/scenario.rs`
- `crates/plumb-functional/tests/state.rs`
- `crates/plumb-functional/tests/verification.rs`
- `crates/plumb-functional/tests/vocabulary.rs`
- `crates/plumb-import/Cargo.toml`
- `crates/plumb-import/src/docx.rs`
- `crates/plumb-import/src/fallback.rs`
- `crates/plumb-import/src/lib.rs`
- `crates/plumb-import/src/manifest.rs`
- `crates/plumb-import/src/md.rs`
- `crates/plumb-import/src/segment.rs`
- `crates/plumb-import/src/source.rs`
- `crates/plumb-import/src/text.rs`
- `crates/plumb-import/tests/docx.rs`
- `crates/plumb-import/tests/md_text.rs`
- `crates/plumb-import/tests/segment.rs`
- `crates/plumb-inference/Cargo.toml`
- `crates/plumb-inference/src/anthropic.rs`
- `crates/plumb-inference/src/lib.rs`
- `crates/plumb-inference/src/mock.rs`
- `crates/plumb-inference/src/model.rs`
- `crates/plumb-inference/src/null.rs`
- `crates/plumb-inference/src/provider.rs`
- `crates/plumb-inference/tests/artifacts.rs`
- `crates/plumb-inference/tests/provider_contract.rs`
- `crates/plumb-intake/Cargo.toml`
- `crates/plumb-intake/src/bm25.rs`
- `crates/plumb-intake/src/index.rs`
- `crates/plumb-intake/src/intake.rs`
- `crates/plumb-intake/src/lib.rs`
- `crates/plumb-intake/src/report.rs`
- `crates/plumb-intake/tests/bm25.rs`
- `crates/plumb-intake/tests/intake.rs`
- `crates/plumb-lint/Cargo.toml`
- `crates/plumb-lint/src/benchmark.rs`
- `crates/plumb-lint/src/lib.rs`
- `crates/plumb-lint/src/rules.rs`
- `crates/plumb-lint/tests/rules.rs`
- `crates/plumb-patch/Cargo.toml`
- `crates/plumb-patch/src/apply.rs`
- `crates/plumb-patch/src/diff.rs`
- `crates/plumb-patch/src/impact.rs`
- `crates/plumb-patch/src/lib.rs`
- `crates/plumb-patch/src/model.rs`
- `crates/plumb-patch/tests/impact.rs`
- `crates/plumb-patch/tests/patch.rs`
- `crates/plumb-psg/Cargo.toml`
- `crates/plumb-psg/src/audit.rs`
- `crates/plumb-psg/src/edge.rs`
- `crates/plumb-psg/src/graph.rs`
- `crates/plumb-psg/src/hash.rs`
- `crates/plumb-psg/src/lib.rs`
- `crates/plumb-psg/src/node.rs`
- `crates/plumb-psg/src/payload.rs`
- `crates/plumb-psg/src/registry.rs`
- `crates/plumb-psg/src/relations.rs`
- `crates/plumb-psg/src/standards.rs`
- `crates/plumb-psg/src/status.rs`
- `crates/plumb-psg/tests/graph.rs`
- `crates/plumb-psg/tests/payload_roundtrip.rs`
- `crates/plumb-psg/tests/relations.rs`
- `crates/plumb-psg/tests/serde.rs`
- `crates/plumb-session/Cargo.toml`
- `crates/plumb-session/src/lib.rs`
- `crates/plumb-session/src/model.rs`
- `crates/plumb-session/src/turn.rs`
- `crates/plumb-session/tests/turn.rs`
- `crates/plumb-sim/Cargo.toml`
- `crates/plumb-sim/src/compare.rs`
- `crates/plumb-sim/src/execute.rs`
- `crates/plumb-sim/src/incremental.rs`
- `crates/plumb-sim/src/lib.rs`
- `crates/plumb-sim/src/materialize.rs`
- `crates/plumb-sim/src/model.rs`
- `crates/plumb-sim/src/trace.rs`
- `crates/plumb-sim/tests/compare.rs`
- `crates/plumb-sim/tests/execute.rs`
- `crates/plumb-sim/tests/incremental.rs`
- `crates/plumb-store/Cargo.toml`
- `crates/plumb-store/src/branches.rs`
- `crates/plumb-store/src/lib.rs`
- `crates/plumb-store/src/revisions.rs`
- `crates/plumb-store/src/schema.rs`
- `crates/plumb-store/src/sqlite.rs`
- `crates/plumb-store/tests/revisions.rs`
- `crates/plumb-validation/Cargo.toml`
- `crates/plumb-validation/src/evaluator.rs`
- `crates/plumb-validation/src/finding.rs`
- `crates/plumb-validation/src/lib.rs`
- `crates/plumb-validation/src/model.rs`
- `crates/plumb-validation/src/profile.rs`
- `crates/plumb-validation/src/registry.rs`
- `crates/plumb-validation/src/rules/f1.rs`
- `crates/plumb-validation/src/rules/f2.rs`
- `crates/plumb-validation/src/rules/f3.rs`
- `crates/plumb-validation/src/rules/f4.rs`
- `crates/plumb-validation/src/rules/i0.rs`
- `crates/plumb-validation/src/rules/mod.rs`
- `crates/plumb-validation/src/waiver.rs`
- `crates/plumb-validation/tests/evaluator.rs`
- `crates/plumb-validation/tests/f1.rs`
- `crates/plumb-validation/tests/f2.rs`
- `crates/plumb-validation/tests/f3.rs`
- `crates/plumb-validation/tests/f4.rs`
- `crates/plumb-validation/tests/i0.rs`
- `crates/plumb-validation/tests/profile.rs`
- `docs/api.md`
- `docs/blockers/`
- `docs/formats.md`
- `docs/pilot/BUILD-REPORT.md`
- `docs/pilot/STANDARDS-ALIGNMENT-REPORT.md`
- `docs/plan/AUDIT-PLUMB-IMPLEMENTATION-PLAN-v3.md`
- `docs/plan/BLOCKER-TEMPLATE.md`
- `docs/plan/EXECUTION-STATE.yaml`
- `docs/plan/POST-PILOT-SCOPE.md`
- `docs/status.md`
- `fixtures/hr-leave/README.md`
- `fixtures/hr-leave/expert-answers.yaml`
- `fixtures/hr-leave/fixture-contract.yaml`
- `fixtures/hr-leave/inference-fixtures.jsonl`
- `fixtures/hr-leave/profile.yaml`
- `fixtures/hr-leave/reference-functional.yaml`
- `fixtures/hr-leave/reference-psg.yaml`
- `fixtures/hr-leave/requirements.docx`
- `fixtures/hr-leave/requirements.md`
- `fixtures/hr-leave/scenarios.yaml`
- `fixtures/hr-leave/stakeholders.yaml`
- `fixtures/lint-corpus/baseline.json`
- `fixtures/lint-corpus/corpus.jsonl`
- `prompts/s1-classify-requirement.md`
- `prompts/s1-ears.md`
- `prompts/s1-segment-requirements.md`
- `prompts/s1-vocabulary.md`
- `prompts/s2-calculation.md`
- `prompts/s2-domain.md`
- `prompts/s2-process.md`
- `prompts/s4-scenario.md`
- `rust-toolchain.toml`
- `schemas/functional-v2.schema.json`
- `schemas/inference/s1-ears.schema.json`
- `schemas/inference/s1-requirement-classification.schema.json`
- `schemas/inference/s1-segmentation.schema.json`
- `schemas/inference/s1-vocabulary.schema.json`
- `schemas/inference/s2-calculation.schema.json`
- `schemas/inference/s2-domain.schema.json`
- `schemas/inference/s2-process.schema.json`
- `schemas/inference/s4-scenario.schema.json`
- `scripts/check-placeholders.sh`
- `scripts/manual-llm-smoke.sh`
- `scripts/run-pilot-e2e.sh`
- `tests/e2e_hr.rs`
- `tests/support/hr_fixture.rs`
- `ui/e2e/pilot.spec.ts`
- `ui/eslint.config.js`
- `ui/package-lock.json`
- `ui/package.json`
- `ui/playwright.config.ts`
- `ui/src/api/client.ts`
- `ui/src/api/schema.d.ts`
- `ui/src/api/useEvents.ts`
- `ui/src/api/useVersionedMutation.ts`
- `ui/src/app/AppShell.tsx`
- `ui/src/app/auth.ts`
- `ui/src/app/nav.ts`
- `ui/src/app/router.tsx`
- `ui/src/components/ConflictDialog.tsx`
- `ui/src/components/DiagramFrame.tsx`
- `ui/src/components/DiffViewer.tsx`
- `ui/src/components/FindingsPanel.tsx`
- `ui/src/components/GateBadge.tsx`
- `ui/src/components/ProposalReviewBar.tsx`
- `ui/src/components/ProvenanceDrawer.tsx`
- `ui/src/components/TypedAnswerControl.tsx`
- `ui/src/components/shared.test.tsx`
- `ui/src/features/analyst/analyst.test.tsx`
- `ui/src/features/calculations/CalculationsPage.tsx`
- `ui/src/features/decisions/DecisionsPage.tsx`
- `ui/src/features/decisions/decisions.test.tsx`
- `ui/src/features/entities/EntitiesPage.tsx`
- `ui/src/features/findings/FindingsPage.tsx`
- `ui/src/features/gates/GatesPage.tsx`
- `ui/src/features/gates/gates.test.tsx`
- `ui/src/features/glossary/GlossaryPage.tsx`
- `ui/src/features/glossary/glossary.test.tsx`
- `ui/src/features/home/HomePage.test.tsx`
- `ui/src/features/home/HomePage.tsx`
- `ui/src/features/processes/ProcessDiagram.tsx`
- `ui/src/features/processes/ProcessesPage.tsx`
- `ui/src/features/processes/processes.test.tsx`
- `ui/src/features/questions/QuestionsPage.tsx`
- `ui/src/features/questions/questions.test.tsx`
- `ui/src/features/requirements/RequirementDetail.tsx`
- `ui/src/features/requirements/RequirementsPage.tsx`
- `ui/src/features/requirements/requirements.test.tsx`
- `ui/src/features/rules/RulesPage.tsx`
- `ui/src/features/scenarios/ScenarioTrace.tsx`
- `ui/src/features/scenarios/ScenariosPage.tsx`
- `ui/src/features/scenarios/scenarios.test.tsx`
- `ui/src/features/sentences/SentencesPage.tsx`
- `ui/src/features/sentences/sentences.test.tsx`
- `ui/src/features/sessions/ProposalCard.tsx`
- `ui/src/features/sessions/SessionPanel.tsx`
- `ui/src/features/sessions/session.test.tsx`
- `ui/src/i18n/en.json`
- `ui/src/i18n/pl.json`
- `ui/src/main.tsx`
- `ui/src/test/msw.ts`
- `ui/tsconfig.json`
- `ui/vite.config.ts`

The checksummed HR fixture inputs are read-only. Directories may contain build outputs ignored by Git, but production source files not named by a task are not authorized.

---

## 10. Task execution contract

Each task has exactly these fields in `PLUMB-IMPLEMENTATION-TASKS-v3.yaml`:

- `id`
- `title`
- `phase`
- `scope`
- `depends_on`
- `files`
- `steps`
- `tests`
- `acceptance`
- `source_refs` — IDs only; resolve via `docs/plan/SOURCE-REFERENCE-INDEX.json`
- `forbidden`
- `commands`
- `commit_message`

`files` is a write allowlist for the task. Reading other repository files is allowed.

A task is complete only when every acceptance statement is true and every test/command exits successfully. Partial completion is not completion.

---

## 11. Ordered task summary

| Order | Task | Phase | Scope | Depends on |
|---:|---|---|---|---|
| 1 | `P0.1` — Verify specification pack and execution scope | P0 | NOW | — |
| 2 | `P0.2` — Create Rust and UI workspace skeleton | P0 | NOW | P0.1 |
| 3 | `F0.1` — Implement core IDs, clock, canonical JSON and hashes | Foundation | NOW | P0.2 |
| 4 | `F0.2` — Implement content-addressed artifact model and SQLite artifact store | Foundation | NOW | F0.1 |
| 5 | `F0.3` — Implement PSG base envelope and typed status/standards structures | Foundation | NOW | F0.2 |
| 6 | `F0.4` — Implement complete NodePayload semantic enum | Foundation | NOW | F0.3 |
| 7 | `F0.5` — Implement RelationKind and typed relation registry | Foundation | NOW | F0.4 |
| 8 | `F0.6` — Implement Graph, typed indexes and semantic/evidence hashes | Foundation | NOW | F0.5 |
| 9 | `F0.7` — Implement serializable SemanticPatch AST and deterministic apply/diff | Foundation | NOW | F0.6 |
| 10 | `F0.8` — Implement immutable revision store, branch heads and CAS commit | Foundation | NOW | F0.7 |
| 11 | `F0.9` — Implement provenance DerivationRecord and inference artifact contracts | Foundation | NOW | F0.8 |
| 12 | `F0.10` — Implement compiler stage plan/evaluate contract and compile-run records | Foundation | NOW | F0.9 |
| 13 | `F0.11` — Implement impact seed and dirty-set reachability | Foundation | NOW | F0.10 |
| 14 | `F0.12` — Implement standards profile loader and validation rule metadata | Foundation | NOW | F0.11 |
| 15 | `F0.13` — Implement deterministic gate evaluator, findings and waivers | Foundation | NOW | F0.12 |
| 16 | `F0.14` — Implement functional.yaml v2 compatibility projection from PSG | Foundation | NOW | F0.13 |
| 17 | `S0.1` — Implement Markdown and plain-text source import | S0 | NOW | F0.14 |
| 18 | `S0.2` — Implement deterministic minimal DOCX importer | S0 | NOW | S0.1 |
| 19 | `S0.3` — Implement evidence manifest and I0 evaluator functions | S0 | NOW | S0.2 |
| 20 | `S0.4` — Implement segmentation request/artifact pipeline and deterministic fallback | S0 | NOW | S0.3 |
| 21 | `S1.1` — Implement Requirement, Need, Goal, Concern and Constraint compilation | S1 | NOW | S0.4 |
| 22 | `S1.2` — Implement EARS normalization as non-authoritative proposal | S1 | NOW | S1.1 |
| 23 | `S1.3` — Implement duplicate detection and supersession proposals | S1 | NOW | S1.2 |
| 24 | `S1.4` — Implement deterministic requirement lint pack and corpus benchmark | S1 | NOW | S1.3 |
| 25 | `S1.5` — Implement vocabulary normalization and typed concept proposals | S1 | NOW | S1.4 |
| 26 | `S1.6` — Implement all F1 validation rules | S1 | NOW | S1.5 |
| 27 | `S2.1` — Implement entity, attribute and domain-relationship proposals | S2 | NOW | S1.6 |
| 28 | `S2.2` — Implement states, transitions and invariants | S2 | NOW | S2.1 |
| 29 | `S2.3` — Implement data classification dictionary proposals | S2 | NOW | S2.2 |
| 30 | `S2.4` — Implement PlumbExpr grammar and typed AST | S2 | NOW | S2.3 |
| 31 | `S2.5` — Implement PlumbExpr type checker, evaluator and calendars | S2 | NOW | S2.4 |
| 32 | `S2.6` — Implement calculations and decision tables | S2 | NOW | S2.5 |
| 33 | `S2.7` — Implement operations, outcomes, events and read/write semantics | S2 | NOW | S2.6 |
| 34 | `S2.8` — Implement separate business-role and security authorization semantics | S2 | NOW | S2.7 |
| 35 | `S2.9` — Implement process model and safe process proposals | S2 | NOW | S2.8 |
| 36 | `S2.10` — Implement all F2 validation rules | S2 | NOW | S2.9 |
| 37 | `S3.1` — Implement deterministic Question generation and routing | S3 | NOW | S2.10 |
| 38 | `S3.2` — Implement question rounds | S3 | NOW | S3.1 |
| 39 | `S3.3` — Implement ResolutionDecision and answer-to-patch application | S3 | NOW | S3.2 |
| 40 | `S3.4` — Implement assumptions and governed waivers | S3 | NOW | S3.3 |
| 41 | `S3.5` — Implement all F3 validation rules | S3 | NOW | S3.4 |
| 42 | `S4.1` — Implement Scenario model and deterministic scenario derivation | S4 | NOW | S3.5 |
| 43 | `S4.2` — Implement pure functional interpreter materialization and execution | S4 | NOW | S4.1 |
| 44 | `S4.3` — Implement Then comparison and scenario traces | S4 | NOW | S4.2 |
| 45 | `S4.4` — Implement dirty-set scenario re-execution and scenario findings | S4 | NOW | S4.3 |
| 46 | `S4.5` — Implement minimal VerificationObligation model for F4 applicability | S4 | NOW | S4.4 |
| 47 | `S4.6` — Implement all F4 validation rules | S4 | NOW | S4.5 |
| 48 | `S4.7` — Implement functional pilot projections and reports | S4 | NOW | S4.6 |
| 49 | `X1.1` — Implement Anthropic InferenceProvider behind llm feature | CrossCutting | NOW | S4.7 |
| 50 | `X1.2` — Implement lexical BM25 retrieval index | CrossCutting | NOW | X1.1 |
| 51 | `X1.3` — Implement patch-centric intake gate | CrossCutting | NOW | X1.2 |
| 52 | `X1.4` — Implement transcript-as-source session pipeline | CrossCutting | NOW | X1.3 |
| 53 | `X1.5` — Implement asynchronous stage job base-revision protection | CrossCutting | NOW | X1.4 |
| 54 | `A0.1` — Generate pilot OpenAPI contract with v3 overrides | Platform | NOW | X1.5 |
| 55 | `A0.2` — Implement source, requirement, model and gate HTTP handlers | Platform | NOW | A0.1 |
| 56 | `A0.3` — Implement questions, proposals, sessions, scenarios and jobs HTTP handlers | Platform | NOW | A0.2 |
| 57 | `A0.4` — Implement CLI commands over the same application services | Platform | NOW | A0.3 |
| 58 | `U0.1` — Scaffold strict React UI and generated API client | UI | NOW | A0.1 |
| 59 | `U1.1` — Implement application shell, role gating and event/version hooks | UI | NOW | U0.1, A0.2 |
| 60 | `U1.2` — Implement shared proposal, provenance, findings, diff and typed-answer components | UI | NOW | U1.1 |
| 61 | `U2.1` — Implement Requirements and Glossary views | UI | NOW | U1.2, A0.2 |
| 62 | `U2.2` — Implement Sentences/semantic proposal review and Questions views | UI | NOW | U2.1, A0.3 |
| 63 | `U2.3` — Implement chat session view | UI | NOW | U2.2, A0.3 |
| 64 | `U2.4` — Implement process swimlane view and semantic step edits | UI | NOW | U2.3 |
| 65 | `U2.5` — Implement Scenarios view and business-semantic traces | UI | NOW | U2.4, A0.3 |
| 66 | `U2.6` — Implement Findings, Entities, Rules and Calculations analyst views | UI | NOW | U2.5, A0.2 |
| 67 | `U2.7` — Implement Gates and Decisions views | UI | NOW | U2.6 |
| 68 | `E0.1` — Generate derived HR reference artifacts from the authoritative fixture contract | E2E | NOW | A0.4, U2.7 |
| 69 | `E0.2` — Implement backend end-to-end pilot test | E2E | NOW | E0.1 |
| 70 | `E0.3` — Implement Playwright pilot workflow against real backend | E2E | NOW | E0.2 |
| 71 | `E0.4` — Enforce coverage, mutation and no-placeholder quality bar | E2E | NOW | E0.3 |
| 72 | `E0.5` — Produce pilot build and standards-alignment report | E2E | NOW | E0.4 |
| 73 | `POST.0` — Post-pilot boundary marker; do not execute under NOW scope | PostPilot | LATER | E0.5 |

---

## 12. Detailed task contracts

### `P0.1` — Verify specification pack and execution scope

**Phase:** `P0`  
**Scope:** `NOW`  
**Dependencies:** none  
**Commit:** `P0.1: Verify specification pack and execution scope`

**Write allowlist**

- _No production/specification file may be written by this task._

**Required actions**

1. Run sha256sum -c docs/plan/SUPPORTING-DOCS.sha256.
2. Run python3 scripts/verify-plan-contract.py.
3. Read CLAUDE.md, this plan, EXECUTION-SCOPE.yaml, SOURCE-REFERENCE-INDEX.yaml, and the complete task manifest before modifying source code.
4. Confirm execution_scope.release equals pilot-v3-foundation and allowed_scope contains NOW only.
5. If either preflight command fails, a required document is missing, a source reference cannot be resolved, or scope differs, create docs/blockers/P0.1.md using the blocker template and stop.

**Commands**

```bash
sha256sum -c docs/plan/SUPPORTING-DOCS.sha256
python3 scripts/verify-plan-contract.py
```

**Tests**

- `Both preflight commands must exit 0.`

**Acceptance**

- All required documents exist, every checksum passes, every task dependency/reference/write-allowlist invariant passes, fixture counts match the contract, and execution scope is exactly pilot-v3-foundation/NOW.

**Supporting references**

- `SRCREF-9C906A7B51` — `docs/plan/PLUMB-IMPLEMENTATION-PLAN-v3.md §§1-4`
- `SRCREF-481C103B3C` — `docs/plan/PLUMB-v2-to-v3-IMPLEMENTATION-DELTA.md §§7-13`

**Task-specific prohibitions**

- Do not modify supporting documents during preflight.
- Do not create source crates before preflight passes.


### `P0.2` — Create Rust and UI workspace skeleton

**Phase:** `P0`  
**Scope:** `NOW`  
**Dependencies:** `P0.1`  
**Commit:** `P0.2: Create Rust and UI workspace skeleton`

**Write allowlist**

- `Cargo.toml`
- `Cargo.lock`
- `rust-toolchain.toml`
- `Makefile`
- `.gitignore`
- `.github/workflows/ci.yml`
- `crates/plumb-core/Cargo.toml`
- `crates/plumb-core/src/lib.rs`
- `crates/plumb-artifacts/Cargo.toml`
- `crates/plumb-artifacts/src/lib.rs`
- `crates/plumb-psg/Cargo.toml`
- `crates/plumb-psg/src/lib.rs`
- `crates/plumb-patch/Cargo.toml`
- `crates/plumb-patch/src/lib.rs`
- `crates/plumb-store/Cargo.toml`
- `crates/plumb-store/src/lib.rs`
- `crates/plumb-validation/Cargo.toml`
- `crates/plumb-validation/src/lib.rs`
- `crates/plumb-inference/Cargo.toml`
- `crates/plumb-inference/src/lib.rs`
- `crates/plumb-compiler/Cargo.toml`
- `crates/plumb-compiler/src/lib.rs`
- `crates/plumb-import/Cargo.toml`
- `crates/plumb-import/src/lib.rs`
- `crates/plumb-lint/Cargo.toml`
- `crates/plumb-lint/src/lib.rs`
- `crates/plumb-expr/Cargo.toml`
- `crates/plumb-expr/src/lib.rs`
- `crates/plumb-functional/Cargo.toml`
- `crates/plumb-functional/src/lib.rs`
- `crates/plumb-sim/Cargo.toml`
- `crates/plumb-sim/src/lib.rs`
- `crates/plumb-intake/Cargo.toml`
- `crates/plumb-intake/src/lib.rs`
- `crates/plumb-session/Cargo.toml`
- `crates/plumb-session/src/lib.rs`
- `crates/plumb-api/Cargo.toml`
- `crates/plumb-api/src/lib.rs`
- `crates/plumb-cli/Cargo.toml`
- `crates/plumb-cli/src/main.rs`
- `ui/package.json`
- `ui/tsconfig.json`
- `ui/vite.config.ts`
- `ui/src/main.tsx`
- `.node-version`

**Required actions**

1. Create the workspace members exactly as listed in the target repository layout in this plan.
2. Write rust-toolchain.toml exactly as: [toolchain] channel="1.98.1", profile="minimal", components=["rustfmt","clippy","llvm-tools-preview"].
3. Write .node-version containing exactly `24.21.0` followed by one newline.
4. Set ui/package.json `engines` exactly to {"node":"24.21.0","npm":"11.19.0"} and `packageManager` exactly to "npm@11.19.0".
5. Set ui/package.json scripts at P0.2 exactly to {"typecheck":"tsc --noEmit"}. U0.1 later replaces/extends the scripts block with its fully specified UI scripts.
6. Set ui/package.json install-script policy exactly to "allowScripts": {"msw": false}. MSW's postinstall worker synchronization is not required by the P0.2 skeleton and must not execute. Any other package with an unreviewed install script must make P0.2 block.
7. Copy the exact [workspace.dependencies] and crate dependency matrix from this plan into Cargo.toml files; crate Cargo.toml files must use workspace = true and may not alter versions/features.
8. Install exactly the versioned UI runtime and development packages listed in §§5.2-5.3 with npm using --save-exact --strict-peer-deps --strict-allow-scripts, apply the JavaScript dependency installation policy in §5.3.1, and commit ui/package-lock.json. Do not add any other package. Do not use --ignore-scripts, --force, --legacy-peer-deps, --dangerously-allow-all-scripts or npm overrides, and do not regenerate the lockfile with npm install --package-lock-only.
9. Create Makefile targets with these exact P0.2 behaviors:

   - check: run cargo check --workspace, then ui-check
   - test: run cargo test --workspace
   - ui-check: run cd ui && npm run typecheck
   - cov: run cargo llvm-cov --workspace --summary-only
   - mutants: run cargo mutants --workspace
   - openapi: run test -f api/modeller.openapi.json
   - e2e: run test -f tests/e2e_hr.rs
   - all: depend on check and test

   The openapi and e2e targets are deliberate readiness checks. They are expected to return failure until the later tasks responsible for those artifacts create them. Do not put unfinished implementation markers or dummy commands into these targets.
10. Create CI on ubuntu-24.04 using actions/checkout@v4 and actions/setup-node@v4 with node-version 24.21.0; install npm@11.19.0; install Rust 1.98.1 with rustfmt, clippy, llvm-tools-preview.
11. CI verifies the native C toolchain by running cc --version; the ubuntu-24.04 runner remains the pinned CI operating-system baseline.
12. CI installs cargo-llvm-cov exactly 0.9.1 and cargo-mutants exactly 27.1.0 using cargo install --version ... --locked.
13. If local node --version is not v24.21.0 or npm --version is not 11.19.0, stop with BLOCKED-ENVIRONMENT rather than using a different version.

**Commands**

```bash
rustup toolchain install 1.98.1 --profile minimal --component rustfmt --component clippy --component llvm-tools-preview
rustc --version
node --version
npm --version
cc --version
cargo check --workspace
cargo test --workspace
cd ui && npm install --save-exact --strict-peer-deps --strict-allow-scripts react@19.3.0 react-dom@19.3.0 @tanstack/react-router@1.170.40 @tanstack/react-query@5.104.0 zustand@5.0.15 @radix-ui/react-dialog@1.1.23 @radix-ui/react-dropdown-menu@2.1.24 @radix-ui/react-tabs@1.1.21 @radix-ui/react-tooltip@1.2.16 @radix-ui/react-popover@1.1.23 @radix-ui/react-select@2.3.7 @radix-ui/react-checkbox@1.3.11 @radix-ui/react-radio-group@1.4.7 @radix-ui/react-toast@1.2.23 tailwindcss@4.3.3 @tailwindcss/vite@4.3.3 elkjs@0.12.0 openapi-fetch@0.17.0 date-fns@4.4.0 zod@4.6.5
cd ui && npm install --save-dev --save-exact --strict-peer-deps --strict-allow-scripts typescript@5.9.3 vite@8.3.1 vitest@5.0.2 jsdom@30.1.1 @types/react@19.3.0 @types/react-dom@19.3.0 @types/node@26.6.3 @testing-library/react@16.3.3 @testing-library/dom@10.4.2 @testing-library/user-event@14.6.7 msw@2.15.0 openapi-typescript@7.13.0 playwright@1.63.0 @axe-core/playwright@4.13.0 eslint@9.39.5 typescript-eslint@8.71.0 eslint-plugin-jsx-a11y@6.10.2 eslint-plugin-react-hooks@7.1.1 prettier@3.9.9
cd ui && npm ci --strict-peer-deps --strict-allow-scripts
cd ui && npm ls --depth=0
cd ui && npm audit --audit-level=high
cd ui && npm run typecheck
```

**Tests**

- `cargo check --workspace`
- `cargo test --workspace`
- `cd ui && npm ci --strict-peer-deps --strict-allow-scripts`
- `cd ui && npm ls --depth=0`
- `cd ui && npm audit --audit-level=high`
- `cd ui && npm run typecheck`

**Acceptance**

- rustc --version starts with `rustc 1.98.1`; node --version equals `v24.21.0`; npm --version equals `11.19.0`.
- Workspace compiles with empty crates; Cargo.lock and ui/package-lock.json are committed; CI workflow uses the exact pinned toolchain and runner.
- cc --version exits 0 and identifies an installed native C compiler.
- cd ui && npm ci --strict-peer-deps --strict-allow-scripts, npm ls --depth=0, npm audit --audit-level=high and npm run typecheck exit 0; npm audit reports zero HIGH and zero CRITICAL findings; no unreviewed install script exists; every direct UI dependency in ui/package.json equals its exact §§5.2-5.3 version.

**Supporting references**

- `SRCREF-E772EBFE16` — `docs/plan/merged-implementation-plan-v2.md §§1-3,7:M0.1,8:U0.1`
- `SRCREF-340600AFE6` — `docs/plan/PLUMB-v2-to-v3-IMPLEMENTATION-DELTA.md §6`

**Task-specific prohibitions**

- Do not add a crate not listed in this task.
- Do not add a dependency outside the Allowed Dependency Set.


### `F0.1` — Implement core IDs, clock, canonical JSON and hashes

**Phase:** `Foundation`  
**Scope:** `NOW`  
**Dependencies:** `P0.2`  
**Commit:** `F0.1: Implement core IDs, clock, canonical JSON and hashes`

**Write allowlist**

- `crates/plumb-core/src/lib.rs`
- `crates/plumb-core/src/id.rs`
- `crates/plumb-core/src/clock.rs`
- `crates/plumb-core/src/canonical.rs`
- `crates/plumb-core/src/hash.rs`
- `crates/plumb-core/src/error.rs`
- `crates/plumb-core/tests/canonical.rs`
- `crates/plumb-core/tests/hash.rs`

**Required actions**

1. Implement Id as a validated String newtype exactly per plan §6.3: grammar ^[a-z][a-z0-9_-]*:[A-Za-z0-9._-]+(?::[A-Za-z0-9._-]+)*$, no trimming or normalization, exact string preserved, validated FromStr and TryFrom<String>, Display, AsRef<str>, as_str(), and serde deserialization that rejects invalid strings.
2. Implement Hash as one validated String newtype exactly per plan §6.3 accepting only sha256:<64 lowercase hex>, psg:sha256:<64 lowercase hex> and ev:sha256:<64 lowercase hex>; provide HashKind {Generic, Semantic, Evidence}, Hash::kind(), Hash::content_sha256(&[u8]), Hash::semantic_sha256(&[u8]), Hash::evidence_sha256(&[u8]), validated FromStr and TryFrom<String>, Display, AsRef<str>, as_str(), and validating serde deserialization. Do not create separate SemanticHash or EvidenceHash types.
3. Implement Timestamp(time::OffsetDateTime) exactly per plan §6.3 (UTC-normalized; RFC 3339 input with T or single-space separator and offset conversion; surrounding whitespace rejected; UTC year restricted to 0000..=9999 with rejection, never panic, outside it; nanosecond precision; canonical Display/serialization with uppercase T, uppercase Z and exactly nine fractional digits) and the Clock trait with fn now(&self) -> Timestamp, SystemClock and FixedClock.
4. Implement the canonical JSON primitive by applying RFC 8785 JCS through the explicitly allowed serde_json_canonicalizer dependency (serde_json_canonicalizer::to_vec) to the supplied value. Do not implement Plumb domain-specific ordering of semantically unordered arrays in F0.1; later semantic-envelope construction applies it before calling this primitive.
5. Implement sha256 hashing over canonical bytes.
6. Do not include timestamps, layout metadata, or audit metadata in semantic hash input; the exact semantic envelope is defined by the metamodel.
7. Add deterministic property tests that map insertion order does not affect canonical output or hash.
8. Tests cover at minimum:

   - Id: every plan §6.3 valid and invalid example; serialization round-trip; invalid serde deserialization rejection.
   - Hash: all three accepted prefixes; exactly 64 lowercase hex; uppercase rejection; length rejection; unknown-prefix rejection; HashKind; deterministic SHA-256 constructors.
   - Timestamp: UTC serialization; non-UTC offset normalization; exactly nine fractional digits; nanosecond retention; serde round-trip; FixedClock repeatability; space-separator input serializing back with T; surrounding-whitespace rejection; valid boundary years 0000 and 9999; rejection below and above the representable range where constructible (including offset normalization past either bound); offset normalization that stays inside the range.
   - Canonical JSON/hash: every plan §6.3 RFC 8785 conformance fixture produces its fixed expected bytes (ASCII, quote/backslash, control-character, BMP and non-BMP keys, string escaping, numbers); each expected output is a fixed point; map insertion order produces byte-identical output and equal resulting hashes, including property tests; repeated canonicalization is byte-identical.

**Commands**

```bash
cargo test -p plumb-core
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-core`

**Acceptance**

- Canonical output is byte-identical for semantically identical map insertion orders and all tests pass.
- Id, Hash and Timestamp satisfy every plan §6.3 grammar, example and canonicalization rule, and serde deserialization of invalid Id/Hash/Timestamp values fails.
- Every plan §6.3 RFC 8785 conformance fixture produces exactly its fixed expected bytes, and each expected output canonicalizes to itself.

**Supporting references**

- `SRCREF-1B777C546D` — `docs/architecture/PLUMB-METAMODEL-v3.md §§3.1,4`
- `SRCREF-C85B1E19E9` — `docs/architecture/PLUMB-COMPILER-ARCHITECTURE-v3.md §§4.1,30`
- `SRCREF-ACB4876C07` — `docs/plan/merged-implementation-plan-v2.md §1 Determinism`

**Task-specific prohibitions**

- Do not write a custom approximate JCS serializer.
- Do not use wall-clock time directly outside SystemClock.


### `F0.2` — Implement content-addressed artifact model and SQLite artifact store

**Phase:** `Foundation`  
**Scope:** `NOW`  
**Dependencies:** `F0.1`  
**Commit:** `F0.2: Implement content-addressed artifact model and SQLite artifact store`

**Write allowlist**

- `crates/plumb-artifacts/src/lib.rs`
- `crates/plumb-artifacts/src/model.rs`
- `crates/plumb-artifacts/src/store.rs`
- `crates/plumb-artifacts/src/sqlite.rs`
- `crates/plumb-artifacts/tests/store.rs`

**Required actions**

1. Implement ArtifactKind exactly for source-original, source-extracted, evidence-manifest, inference-request, inference-response, validated-inference, external-validation, projection, proposal, patch, diff, impact-report, gate-report, conformance-report, scenario-trace, test-receipt, architecture-check. Serialize and persist each kind as exactly its listed kebab-case string. An unknown persisted kind string returns an explicit error; do not map it to an Other variant.
2. Artifact identity is the generic sha256: Hash (plan §6.3) of the exact stored bytes: artifact_hash = sha256:<lowercase SHA-256 of exact stored bytes>. ArtifactKind, media_type and created_at do not participate in the hash. The persisted artifact semantics are Artifact { hash: Hash, kind: ArtifactKind, media_type: String, bytes: Vec<u8>, created_at: Timestamp }; equivalent owned/borrowed API shapes are acceptable. Use the plumb-core Hash and Timestamp primitives; do not duplicate them.
3. Create SQLite table artifacts(hash TEXT PRIMARY KEY, kind TEXT NOT NULL, media_type TEXT NOT NULL, bytes BLOB NOT NULL, created_at TEXT NOT NULL). Persist created_at in the canonical Timestamp form of plan §6.3.
4. put receives ArtifactKind, media type, the exact artifact bytes and a caller-supplied Timestamp; the store must not obtain wall-clock time itself. put semantics:

   1. Compute the generic sha256: Hash over the exact byte slice.
   2. If the hash does not exist: insert exactly one row persisting the supplied kind, media type, bytes and created_at; return the artifact hash.
   3. If the hash already exists: load the existing row and verify that the existing bytes, ArtifactKind and media_type equal the supplied values.
   4. If bytes, kind and media_type all match: the operation is idempotent; do not update created_at, do not add another row, and return the existing hash. created_at from a duplicate put is intentionally ignored after the first successful insert.
   5. If the same content hash exists but kind or media_type differs: return an explicit ArtifactMetadataConflict and do not mutate the existing row.
   6. If the stored bytes differ from the supplied bytes for the same hash (a hash collision): return an explicit integrity error and never overwrite the existing row.
5. Artifact bytes and their persisted metadata are immutable after the first successful insertion. No artifact update, replace or delete-by-put behavior is permitted.
6. Provide get (lookup by artifact Hash) and exists (existence check by Hash). get on a missing artifact returns an explicit not-found result/error and never fabricates an empty artifact; exists returns false for a missing hash. Reject non-Generic hash kinds (psg:sha256: and ev:sha256:) as artifact-store keys; the artifact table is keyed only by generic sha256: hashes.
7. Duplicate insertion is transaction-safe and a race/retry converges to the same result: same bytes, kind and media_type give the same hash and one row; a metadata mismatch gives a conflict with the existing row unchanged. Do not solve concurrency by weakening uniqueness or adding duplicate rows.
8. Tests cover at minimum:

   - first put inserts one row;
   - identical bytes with identical kind and media_type put twice return the same hash;
   - a duplicate put leaves the original created_at unchanged;
   - different bytes produce different hashes;
   - same bytes with a different ArtifactKind return ArtifactMetadataConflict;
   - same bytes with a different media_type return ArtifactMetadataConflict;
   - a metadata conflict leaves the original row byte-for-byte unchanged;
   - get returns all five persisted fields correctly;
   - exists true/false behavior;
   - the artifact hash is the generic sha256: kind;
   - the store rejects psg:sha256: key lookup where applicable;
   - the store rejects ev:sha256: key lookup where applicable;
   - an unknown persisted ArtifactKind string fails explicitly;
   - duplicate/repeated insertion never creates more than one row;
   - transaction rollback leaves no partial artifact on failure.

   If an actual SHA-256 collision cannot reasonably be generated in a unit test, implement the integrity check in code but do not fabricate a cryptographic collision test.

**Commands**

```bash
cargo test -p plumb-artifacts
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-artifacts`

**Acceptance**

- Putting identical bytes twice returns the same hash and stores one row; differing bytes return differing hashes.
- A duplicate put with identical bytes, kind and media_type is idempotent and keeps the original created_at; the same bytes with a different kind or media_type return ArtifactMetadataConflict and leave the existing row unchanged; get and exists reject non-generic hash keys; an unknown persisted kind fails explicitly; a failed put leaves no partial row.

**Supporting references**

- `SRCREF-440BF23BDA` — `docs/architecture/PLUMB-COMPILER-ARCHITECTURE-v3.md §24`
- `SRCREF-AED4772768` — `docs/plan/PLUMB-v2-to-v3-IMPLEMENTATION-DELTA.md §4 Artifact store`

**Task-specific prohibitions**

- Do not store mutable semantic state in the artifact table.


### `F0.3` — Implement PSG base envelope and typed status/standards structures

**Phase:** `Foundation`  
**Scope:** `NOW`  
**Dependencies:** `F0.2`  
**Commit:** `F0.3: Implement PSG base envelope and typed status/standards structures`

**Write allowlist**

- `crates/plumb-psg/src/lib.rs`
- `crates/plumb-psg/src/status.rs`
- `crates/plumb-psg/src/standards.rs`
- `crates/plumb-psg/src/node.rs`
- `crates/plumb-psg/src/edge.rs`
- `crates/plumb-psg/src/audit.rs`
- `crates/plumb-psg/tests/serde.rs`

**Required actions**

1. Implement ElementStatus exactly: Proposed, Accepted, Rejected, Superseded, Deprecated, Suspect.
2. Implement StandardMapping, MappingRole and MappingStrength exactly from the metamodel.
3. Implement Node and Edge envelopes with immutable id, revision, status, evidence refs, derivation refs, standards, tags, extensions and audit metadata.
4. Extensions require a namespace-qualified key containing one colon.
5. Do not add a universal confidence field to Node or Edge.

**Commands**

```bash
cargo test -p plumb-psg serde
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-psg serde`

**Acceptance**

- All base structures round-trip through JSON and reject an extension key without a namespace.

**Supporting references**

- `SRCREF-2A709879EB` — `docs/architecture/PLUMB-METAMODEL-v3.md §§2-4,16`

**Task-specific prohibitions**

- Do not implement core semantics as kind:String plus arbitrary props.
- Do not add fields absent from the metamodel without a blocking spec issue.


### `F0.4` — Implement complete NodePayload semantic enum

**Phase:** `Foundation`  
**Scope:** `NOW`  
**Dependencies:** `F0.3`  
**Commit:** `F0.4: Implement complete NodePayload semantic enum`

**Write allowlist**

- `crates/plumb-psg/src/payload.rs`
- `crates/plumb-psg/tests/payload_roundtrip.rs`

**Required actions**

1. Implement every core NodePayload variant defined by the metamodel namespaces: evidence/provenance, governance, intent/requirements, vocabulary/domain, functional behavior, authorization, quality, architecture, technical contracts, delivery, verification, standards profile and view model.
2. Field names, required/optional status, and enums must follow the metamodel sections defining each type.
3. Use serde tagged representation type/data as specified by metamodel §24.
4. When the prose definition and skeleton disagree, do not choose; create docs/blockers/F0.4.md identifying both conflicting passages and stop.

**Commands**

```bash
cargo test -p plumb-psg payload_roundtrip
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-psg payload_roundtrip`

**Acceptance**

- One golden round-trip fixture exists for every NodePayload variant and all required-field omission tests fail deserialization or validation as intended.

**Supporting references**

- `SRCREF-7EC2A38F66` — `docs/architecture/PLUMB-METAMODEL-v3.md §§5-16,19,24`

**Task-specific prohibitions**

- Do not omit a metamodel variant because it is post-pilot; post-pilot types must exist even when no service uses them yet.


### `F0.5` — Implement RelationKind and typed relation registry

**Phase:** `Foundation`  
**Scope:** `NOW`  
**Dependencies:** `F0.4`  
**Commit:** `F0.5: Implement RelationKind and typed relation registry`

**Write allowlist**

- `crates/plumb-psg/src/relations.rs`
- `crates/plumb-psg/src/registry.rs`
- `crates/plumb-psg/tests/relations.rs`

**Required actions**

1. Implement the closed core relation vocabulary exactly from metamodel §17.
2. For each relation register allowed source NodePayload types, allowed target types, cardinality constraints and graph constraints.
3. Support namespaced extension relations separately; core code must reject an unknown unqualified relation.
4. Validate acyclicity for supersedes, refines/decomposes_to where mandated, inherits_role, and depends_on_slice.

**Commands**

```bash
cargo test -p plumb-psg relations
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-psg relations`

**Acceptance**

- Every relation listed in metamodel §17 has a registry entry; invalid source/target pairs are rejected; required acyclic relations reject cycles.

**Supporting references**

- `SRCREF-23F6E29DAE` — `docs/architecture/PLUMB-METAMODEL-v3.md §§17-18`

**Task-specific prohibitions**

- Do not infer relation compatibility from type names.


### `F0.6` — Implement Graph, typed indexes and semantic/evidence hashes

**Phase:** `Foundation`  
**Scope:** `NOW`  
**Dependencies:** `F0.5`  
**Commit:** `F0.6: Implement Graph, typed indexes and semantic/evidence hashes`

**Write allowlist**

- `crates/plumb-psg/src/graph.rs`
- `crates/plumb-psg/src/hash.rs`
- `crates/plumb-psg/tests/graph.rs`

**Required actions**

1. Implement BTreeMap-backed node and edge storage.
2. Maintain indexes by payload type, relation kind, outgoing node and incoming node.
3. Graph validation calls node schema validation, relation registry validation and mandatory core invariants applicable without gate context.
4. Implement semantic_hash over accepted semantic nodes/edges plus accepted semantic decisions/constraints using exclusions defined by metamodel §3.1.
5. Implement evidence_hash over source artifacts/evidence identity.
6. View/layout data must not influence semantic_hash.

**Commands**

```bash
cargo test -p plumb-psg graph
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-psg graph`

**Acceptance**

- Graph hash is stable across insertion order; changing audit timestamp or view position does not change semantic_hash; changing an accepted requirement statement does.

**Supporting references**

- `SRCREF-02DFDD2B29` — `docs/architecture/PLUMB-METAMODEL-v3.md §§3,18,19`

**Task-specific prohibitions**

- Do not hash transient compile jobs or cached inference text into semantic_hash.


### `F0.7` — Implement serializable SemanticPatch AST and deterministic apply/diff

**Phase:** `Foundation`  
**Scope:** `NOW`  
**Dependencies:** `F0.6`  
**Commit:** `F0.7: Implement serializable SemanticPatch AST and deterministic apply/diff`

**Write allowlist**

- `crates/plumb-patch/src/lib.rs`
- `crates/plumb-patch/src/model.rs`
- `crates/plumb-patch/src/apply.rs`
- `crates/plumb-patch/src/diff.rs`
- `crates/plumb-patch/tests/patch.rs`

**Required actions**

1. Implement AddNode, RemoveNode, ReplacePayload, SetStatus, AddEdge, RemoveEdge, MergeNodes, Supersede, AttachEvidence, AttachStandardMapping and Compound exactly as metamodel §20.
2. Every destructive operation carries the expected current semantic hash or expected element hash required by the metamodel.
3. Apply patches to an immutable Graph input and return GraphDelta plus new Graph.
4. Provide deterministic structured diff sorted by element ID and operation order.
5. Implement inverse only for patch operations where an exact inverse is derivable from the patch input; do not fabricate an inverse.

**Commands**

```bash
cargo test -p plumb-patch
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-patch`

**Acceptance**

- All patch variants serialize/deserialize; apply is deterministic; property tests cover applicable apply/invert round trips.

**Supporting references**

- `SRCREF-4C3A2D5489` — `docs/architecture/PLUMB-METAMODEL-v3.md §20`
- `SRCREF-76F0A59202` — `docs/plan/PLUMB-v2-to-v3-IMPLEMENTATION-DELTA.md §§3.3,4`

**Task-specific prohibitions**

- Do not use Box<dyn Patch>.
- Do not mutate Graph in place.


### `F0.8` — Implement immutable revision store, branch heads and CAS commit

**Phase:** `Foundation`  
**Scope:** `NOW`  
**Dependencies:** `F0.7`  
**Commit:** `F0.8: Implement immutable revision store, branch heads and CAS commit`

**Write allowlist**

- `crates/plumb-store/src/lib.rs`
- `crates/plumb-store/src/schema.rs`
- `crates/plumb-store/src/sqlite.rs`
- `crates/plumb-store/src/revisions.rs`
- `crates/plumb-store/src/branches.rs`
- `crates/plumb-store/tests/revisions.rs`

**Required actions**

1. Create SQLite tables graph_revisions, revision_nodes, revision_edges, branch_heads and ledger_events using the exact schema block in this plan.
2. A GraphRevision is immutable and stores global numeric version, parent revision, semantic_hash, evidence_hash, profile_ref, rule_pack_hash, patch artifact ref, decision refs, creator and timestamp.
3. Assign version 1 to the initial revision and max(version)+1 to each successful new revision inside the same SQLite transaction.
4. Set revision ID to `rev:<version>:<first16-semantic-hash-hex>`.
5. Implement create_initial_revision, load_revision, head, create_branch, commit(branch, expected_head, patch), and move_head for explicit restore.
6. commit must be one SQLite transaction and fail with STALE_BASE when expected_head differs.
7. Persist full graph snapshot per revision for the pilot; do not introduce delta storage.

**Commands**

```bash
cargo test -p plumb-store revisions
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-store revisions`

**Acceptance**

- Concurrent commits against the same expected head allow exactly one success; restore is implemented by branch-head movement and does not delete history.

**Supporting references**

- `SRCREF-D32DB0990A` — `docs/architecture/PLUMB-COMPILER-ARCHITECTURE-v3.md §§4.1,5.8,21`
- `SRCREF-BBA84CFE16` — `docs/plan/PLUMB-v2-to-v3-IMPLEMENTATION-DELTA.md §§3.6-3.7`

**Task-specific prohibitions**

- Do not overwrite an existing revision.
- Do not silently rebase a stale patch.


### `F0.9` — Implement provenance DerivationRecord and inference artifact contracts

**Phase:** `Foundation`  
**Scope:** `NOW`  
**Dependencies:** `F0.8`  
**Commit:** `F0.9: Implement provenance DerivationRecord and inference artifact contracts`

**Write allowlist**

- `crates/plumb-inference/src/lib.rs`
- `crates/plumb-inference/src/model.rs`
- `crates/plumb-inference/src/provider.rs`
- `crates/plumb-inference/src/null.rs`
- `crates/plumb-inference/src/mock.rs`
- `crates/plumb-inference/tests/artifacts.rs`

**Required actions**

1. Implement Agent, DerivationRecord, InferenceRequest and InferenceArtifact fields exactly from metamodel §5 and compiler architecture §4.3-4.4.
2. InferenceRequest ID is deterministic from canonical request content.
3. NullProvider always returns a typed provider-disabled error.
4. MockProvider returns only artifacts supplied explicitly by a test fixture keyed by request hash.
5. A live provider interface returns an InferenceArtifact and has no access to GraphStore or branch commits.

**Commands**

```bash
cargo test -p plumb-inference
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-inference`

**Acceptance**

- Request hashes are deterministic; replaying a MockProvider fixture produces the same validated_output_hash.

**Supporting references**

- `SRCREF-96AA517C73` — `docs/architecture/PLUMB-METAMODEL-v3.md §5.3`
- `SRCREF-25544DD485` — `docs/architecture/PLUMB-COMPILER-ARCHITECTURE-v3.md §§3,4.3-4.4,19-20`

**Task-specific prohibitions**

- Do not permit provider code to mutate PSG.
- Do not call a live network provider in tests.


### `F0.10` — Implement compiler stage plan/evaluate contract and compile-run records

**Phase:** `Foundation`  
**Scope:** `NOW`  
**Dependencies:** `F0.9`  
**Commit:** `F0.10: Implement compiler stage plan/evaluate contract and compile-run records`

**Write allowlist**

- `crates/plumb-compiler/src/lib.rs`
- `crates/plumb-compiler/src/stage.rs`
- `crates/plumb-compiler/src/context.rs`
- `crates/plumb-compiler/src/run.rs`
- `crates/plumb-compiler/tests/stage.rs`

**Required actions**

1. Implement CompilerStage::plan and CompilerStage::evaluate exactly as compiler architecture §28.
2. Implement StagePlan, StageEvaluation and CompileRun.
3. plan and evaluate receive no network/client dependency and must be deterministic for equal inputs/artifacts.
4. Artifact acquisition is represented by requests returned in StagePlan and executed by orchestration code outside the trait.
5. Persist compile-run metadata as an artifact; do not make operational timestamps part of semantic output hash.

**Commands**

```bash
cargo test -p plumb-compiler
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-compiler`

**Acceptance**

- A fixture stage returns identical plan/evaluation bytes for repeated equal inputs and cannot access an InferenceProvider through the stage trait.

**Supporting references**

- `SRCREF-BC01A56530` — `docs/architecture/PLUMB-COMPILER-ARCHITECTURE-v3.md §§3,4.2,28`

**Task-specific prohibitions**

- Do not add complete_json or HTTP calls to CompilerStage.


### `F0.11` — Implement impact seed and dirty-set reachability

**Phase:** `Foundation`  
**Scope:** `NOW`  
**Dependencies:** `F0.10`  
**Commit:** `F0.11: Implement impact seed and dirty-set reachability`

**Write allowlist**

- `crates/plumb-patch/src/impact.rs`
- `crates/plumb-patch/tests/impact.rs`

**Required actions**

1. For each GraphDelta compute ChangedSet containing touched node and edge IDs.
2. Compute DirtySet through typed graph relations that participate in semantic dependency: derived_from, specified_by, constrained_by, satisfied_by, reads, writes, governed_by, uses_calculation, allocated_to, exposed_by, implemented_by, verified_by and implemented_as.
3. Return affected projections and affected gate namespaces as symbolic sets; full C1 evidence staleness remains post-pilot.
4. Traversal order must be deterministic.

**Commands**

```bash
cargo test -p plumb-patch impact
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-patch impact`

**Acceptance**

- Changing a calculation marks dependent operations and scenarios dirty; changing only view metadata marks no semantic element dirty.

**Supporting references**

- `SRCREF-D078477292` — `docs/architecture/PLUMB-COMPILER-ARCHITECTURE-v3.md §§5.6,22`
- `SRCREF-856494881E` — `docs/plan/PLUMB-v2-to-v3-IMPLEMENTATION-DELTA.md §4 Impact seed graph`

**Task-specific prohibitions**

- Do not use name similarity for impact propagation.


### `F0.12` — Implement standards profile loader and validation rule metadata

**Phase:** `Foundation`  
**Scope:** `NOW`  
**Dependencies:** `F0.11`  
**Commit:** `F0.12: Implement standards profile loader and validation rule metadata`

**Write allowlist**

- `crates/plumb-validation/src/lib.rs`
- `crates/plumb-validation/src/model.rs`
- `crates/plumb-validation/src/profile.rs`
- `crates/plumb-validation/src/registry.rs`
- `crates/plumb-validation/tests/profile.rs`

**Required actions**

1. Load profile metadata from the supplied YAML without modifying that YAML.
2. Implement RuleClass, RuleResultState, Severity, WaiverPolicy, GateId and StandardRef.
3. At startup validate that the YAML contains exactly the 133 rule IDs from the supplied file and unique IDs.
4. For NOW scope register evaluator functions only for gates I0, F1, F2, F3 and F4; metadata for later gates must still load.
5. A missing evaluator for a NOW rule is startup error; a missing evaluator for a later gate returns NOT_IMPLEMENTED if that gate is explicitly requested.

**Commands**

```bash
cargo test -p plumb-validation profile
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-validation profile`

**Acceptance**

- All 133 metadata rules load; exactly 54 NOW gate rules require evaluators; duplicate rule IDs fail profile loading.

**Supporting references**

- `SRCREF-0B1179FD2E` — `docs/standards/PLUMB-VALIDATION-RULEBOOK-2026.1.md §§1-4,7-8`
- `SRCREF-5608A0663D` — `config/profiles/plumb-software-2026.1-rules.yaml entire file`

**Task-specific prohibitions**

- Do not rewrite rule IDs or severities in code.
- Do not duplicate rule metadata in Rust constants when it can be loaded from YAML.


### `F0.13` — Implement deterministic gate evaluator, findings and waivers

**Phase:** `Foundation`  
**Scope:** `NOW`  
**Dependencies:** `F0.12`  
**Commit:** `F0.13: Implement deterministic gate evaluator, findings and waivers`

**Write allowlist**

- `crates/plumb-validation/src/evaluator.rs`
- `crates/plumb-validation/src/finding.rs`
- `crates/plumb-validation/src/waiver.rs`
- `crates/plumb-validation/tests/evaluator.rs`

**Required actions**

1. Implement rule result states PASS, FAIL, WARN, NOT_APPLICABLE, WAIVED, ERROR.
2. Finding key is sha256(rule_id || sorted(target_ids) || semantic_condition_key).
3. Gate passes only under rulebook §2 semantics.
4. WAIVED requires rule waiver policy plus an accepted ResolutionDecision reference; forbidden waivers fail.
5. GateReport includes baseline semantic_hash, profile ID/hash, rule-pack hash, individual rule results and waiver list.
6. Evaluation is pure; external validation artifacts are inputs, never executed inside evaluator.

**Commands**

```bash
cargo test -p plumb-validation evaluator
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-validation evaluator`

**Acceptance**

- Repeated gate evaluation over equal graph/profile/artifacts is byte-identical except explicitly excluded operational timestamp fields.

**Supporting references**

- `SRCREF-FF7208E4F2` — `docs/standards/PLUMB-VALIDATION-RULEBOOK-2026.1.md §§2,5,7`

**Task-specific prohibitions**

- Do not let readiness scores influence gate result.


### `F0.14` — Implement functional.yaml v2 compatibility projection from PSG

**Phase:** `Foundation`  
**Scope:** `NOW`  
**Dependencies:** `F0.13`  
**Commit:** `F0.14: Implement functional.yaml v2 compatibility projection from PSG`

**Write allowlist**

- `crates/plumb-functional/src/lib.rs`
- `crates/plumb-functional/src/model.rs`
- `crates/plumb-functional/src/project.rs`
- `crates/plumb-functional/tests/golden.rs`
- `schemas/functional-v2.schema.json`
- `docs/formats.md`

**Required actions**

1. Implement the v2 functional.yaml shape exactly as v2 plan §4.1 as an output projection only.
2. Map PSG Actor/BusinessRole/SecurityRole/Permission to the legacy actor/role representation without losing PSG truth; any lossy mapping must be documented in projection metadata and never written back automatically.
3. Map QualityScenario to legacy NFR fields only where an exact legacy field exists; omit unsupported quality scenarios from the legacy NFR bag and list them in projection warnings.
4. Validate the projection against schemas/functional-v2.schema.json.
5. Projection metadata records source semantic_hash, profile_hash, projection version and content hash.

**Commands**

```bash
cargo test -p plumb-functional golden
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-functional golden`

**Acceptance**

- Golden HR projection is deterministic and validates; modifying UI layout does not change projection content.

**Supporting references**

- `SRCREF-94287B9738` — `docs/plan/merged-implementation-plan-v2.md §4.1`
- `SRCREF-A384F23134` — `docs/architecture/PLUMB-METAMODEL-v3.md §§26-27`
- `SRCREF-ED9C3A0EA2` — `docs/plan/PLUMB-v2-to-v3-IMPLEMENTATION-DELTA.md §§3.4,11`

**Task-specific prohibitions**

- Do not treat functional.yaml as canonical input state.


### `S0.1` — Implement Markdown and plain-text source import

**Phase:** `S0`  
**Scope:** `NOW`  
**Dependencies:** `F0.14`  
**Commit:** `S0.1: Implement Markdown and plain-text source import`

**Write allowlist**

- `crates/plumb-import/src/lib.rs`
- `crates/plumb-import/src/source.rs`
- `crates/plumb-import/src/md.rs`
- `crates/plumb-import/src/text.rs`
- `crates/plumb-import/tests/md_text.rs`

**Required actions**

1. Import bytes into SourceArtifact with source_kind, display_name, content_hash and media_type.
2. Decode UTF-8; reject invalid UTF-8 for .md/.txt with E_SOURCE_ENCODING. Normalize CRLF and CR to LF; do not trim internal whitespace.
3. Create a content-addressed source-extracted artifact from the normalized UTF-8 text.
4. Markdown heading fragment: one ATX heading matching `^#{1,6}[ \t]+(.+?)\s*$`; fragment includes the full normalized source line.
5. Markdown unordered-list fragment: one line matching `^[ \t]*[-+*][ \t]+(.+)$`; ordered-list fragment: one line matching `^[ \t]*[0-9]+[.)][ \t]+(.+)$`.
6. Markdown table fragment: a contiguous pipe-delimited block is a table only when its second nonblank line matches a GFM delimiter row whose cells match `^:?-{3,}:?$`; emit one EvidenceFragment per subsequent data row and include header cells in fragment metadata.
7. Paragraph fragment: maximal contiguous nonblank lines not already classified as heading/list/table; preserve embedded newlines.
8. Plain-text import emits paragraph fragments using the same paragraph rule and ordered/unordered list recognition; it has no heading or table inference.
9. TextRange start/end are zero-based UTF-8 byte offsets into normalized extracted text with end exclusive; slicing those bytes must equal the exact fragment text.

**Commands**

```bash
cargo test -p plumb-import md_text
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-import md_text`

**Acceptance**

- All classification regexes above are covered by tests; fragment ranges round-trip exactly; no other Markdown construct receives a special fragment kind in NOW scope.

**Supporting references**

- `SRCREF-52BF75EEBC` — `docs/plan/merged-implementation-plan-v2.md §7:M1.1`
- `SRCREF-F3C31C7BB3` — `docs/architecture/PLUMB-METAMODEL-v3.md §§5.1-5.2`
- `SRCREF-BC2B9CB618` — `docs/architecture/PLUMB-COMPILER-ARCHITECTURE-v3.md §6`


### `S0.2` — Implement deterministic minimal DOCX importer

**Phase:** `S0`  
**Scope:** `NOW`  
**Dependencies:** `S0.1`  
**Commit:** `S0.2: Implement deterministic minimal DOCX importer`

**Write allowlist**

- `crates/plumb-import/src/docx.rs`
- `crates/plumb-import/tests/docx.rs`

**Required actions**

1. Implement DOCX parsing over zip/XML for paragraphs, headings, numbered lists, bulleted lists and tables with row/header extraction.
2. Use only zip and quick-xml from the allowed dependency set.
3. Preserve extracted-text order; merged cells must resolve deterministically.
4. Unsupported DOCX content becomes a parse warning with source locator; it must not be silently dropped when it contains visible text.
5. Use the bundled read-only fixtures/hr-leave/requirements.docx as one acceptance input; do not edit it.

**Commands**

```bash
cargo test -p plumb-import docx
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-import docx`

**Acceptance**

- The HR DOCX extracts the expected ordered text and tables; unsupported visible text creates a warning.

**Supporting references**

- `SRCREF-4C965A435B` — `docs/plan/merged-implementation-plan-v2.md §§1 Import formats,7:M1.1`
- `SRCREF-BC2B9CB618` — `docs/architecture/PLUMB-COMPILER-ARCHITECTURE-v3.md §6`


### `S0.3` — Implement evidence manifest and I0 evaluator functions

**Phase:** `S0`  
**Scope:** `NOW`  
**Dependencies:** `S0.2`  
**Commit:** `S0.3: Implement evidence manifest and I0 evaluator functions`

**Write allowlist**

- `crates/plumb-import/src/manifest.rs`
- `crates/plumb-validation/src/rules/i0.rs`
- `crates/plumb-validation/src/rules/mod.rs`
- `crates/plumb-validation/tests/i0.rs`

**Required actions**

1. Implement all six I0 rules from the supplied rule YAML and rulebook without changing IDs, severity or waiver policy.
2. Evidence manifest lists source artifact refs, extracted artifact refs, fragment refs and hashes in sorted order.
3. PPMN.I0.PROVENANCE.AGENT_IDENTIFIED passes only when every derivation-producing action identifies an Agent.
4. PLUMB.I0.BASELINE.HASHABLE validates reproducible evidence_hash.

**Commands**

```bash
cargo test -p plumb-validation i0
cargo test -p plumb-import manifest
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-validation i0`
- `cargo test -p plumb-import manifest`

**Acceptance**

- All six I0 rule fixtures cover PASS and FAIL; HR source corpus passes I0.

**Supporting references**

- `SRCREF-EAF71A2D01` — `docs/standards/PLUMB-VALIDATION-RULEBOOK-2026.1.md §I0`
- `SRCREF-D0FDFF3BDE` — `config/profiles/plumb-software-2026.1-rules.yaml rules where gate=I0`


### `S0.4` — Implement segmentation request/artifact pipeline and deterministic fallback

**Phase:** `S0`  
**Scope:** `NOW`  
**Dependencies:** `S0.3`  
**Commit:** `S0.4: Implement segmentation request/artifact pipeline and deterministic fallback`

**Write allowlist**

- `crates/plumb-import/src/segment.rs`
- `crates/plumb-import/src/fallback.rs`
- `crates/plumb-import/tests/segment.rs`
- `prompts/s1-segment-requirements.md`
- `schemas/inference/s1-segmentation.schema.json`

**Required actions**

1. Create an InferenceRequest for semantic segmentation anchored only to EvidenceFragment IDs and extracted text.
2. Validate inference output against the JSON schema before evaluate uses it.
3. Implement deterministic fallback: one candidate per list item or sentence containing shall, must, should or can.
4. Post-check fragment coverage, overlap and locator validity; unassigned text is explicitly classified non-requirement and flagged.
5. A failed or absent inference artifact uses fallback; this operational fallback must be recorded in DerivationRecord.

**Commands**

```bash
cargo test -p plumb-import segment
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-import segment`

**Acceptance**

- Mock artifact path and fallback path produce valid, non-overlapping candidates; a gap produces the declared intake uncovered error.

**Supporting references**

- `SRCREF-40D69B8FBA` — `docs/plan/merged-implementation-plan-v2.md §7:M1.2`
- `SRCREF-563DAF2FEF` — `docs/architecture/PLUMB-COMPILER-ARCHITECTURE-v3.md §§3,7`

**Task-specific prohibitions**

- Do not call an LLM from segment evaluation.
- Do not create a requirement without an EvidenceFragment reference.


### `S1.1` — Implement Requirement, Need, Goal, Concern and Constraint compilation

**Phase:** `S1`  
**Scope:** `NOW`  
**Dependencies:** `S0.4`  
**Commit:** `S1.1: Implement Requirement, Need, Goal, Concern and Constraint compilation`

**Write allowlist**

- `crates/plumb-functional/src/requirements.rs`
- `crates/plumb-functional/src/intent.rs`
- `crates/plumb-functional/tests/requirements.rs`
- `prompts/s1-classify-requirement.md`
- `schemas/inference/s1-requirement-classification.schema.json`

**Required actions**

1. Compile candidate requirement statements into Proposed Requirement nodes with evidence.
2. Use deterministic modality extraction for shall, should, may and shall not; any other normative interpretation is a proposal requiring human confirmation.
3. Classification inference may propose requirement_kind and level but evaluate rejects values outside metamodel enums.
4. Create optional Need/Goal/Concern/Constraint proposals only when they contain evidence refs.
5. Requirement IDs are deterministic from project namespace plus stable evidence fragment key; re-import of unchanged evidence preserves IDs.

**Commands**

```bash
cargo test -p plumb-functional requirements
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-functional requirements`

**Acceptance**

- Re-import preserves IDs; classification outside enum is rejected; no accepted requirement lacks evidence or explicit human-origin derivation.

**Supporting references**

- `SRCREF-F6F023A36E` — `docs/architecture/PLUMB-METAMODEL-v3.md §7`
- `SRCREF-775B925F0C` — `docs/plan/merged-implementation-plan-v2.md §7:M1.3`
- `SRCREF-B0F74B9142` — `docs/architecture/PLUMB-COMPILER-ARCHITECTURE-v3.md §7`


### `S1.2` — Implement EARS normalization as non-authoritative proposal

**Phase:** `S1`  
**Scope:** `NOW`  
**Dependencies:** `S1.1`  
**Commit:** `S1.2: Implement EARS normalization as non-authoritative proposal`

**Write allowlist**

- `crates/plumb-functional/src/ears.rs`
- `crates/plumb-functional/tests/ears.rs`
- `prompts/s1-ears.md`
- `schemas/inference/s1-ears.schema.json`

**Required actions**

1. Store original requirement statement unchanged.
2. EARS-form normalized text is a Proposal that references the original requirement and evidence.
3. Accepting an EARS proposal uses SemanticPatch ReplacePayload and records ResolutionDecision when meaning changes materially.
4. Reject an EARS output that introduces an actor, trigger, condition, response or numeric constraint not grounded in evidence or already accepted PSG semantics.

**Commands**

```bash
cargo test -p plumb-functional ears
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-functional ears`

**Acceptance**

- A crafted inference that invents a numeric threshold is rejected; original text remains retrievable after accepted normalization.

**Supporting references**

- `SRCREF-397E123D2C` — `docs/plan/merged-implementation-plan-v2.md §§7:M1.3,7:M2.1`
- `SRCREF-91B589D90C` — `docs/architecture/PLUMB-METAMODEL-v3.md §7.5`


### `S1.3` — Implement duplicate detection and supersession proposals

**Phase:** `S1`  
**Scope:** `NOW`  
**Dependencies:** `S1.2`  
**Commit:** `S1.3: Implement duplicate detection and supersession proposals`

**Write allowlist**

- `crates/plumb-functional/src/duplicates.rs`
- `crates/plumb-functional/tests/duplicates.rs`

**Required actions**

1. Implement normalized Jaccard duplicate score using the threshold from profile configuration.
2. Return duplicate/near-duplicate findings and merge/supersede proposals; never auto-merge accepted requirements.
3. Use MergeNodes only when the two requirements assert the same active obligation and neither is identified as a later replacement; preserve evidence from both.
4. Use Supersede only when evidence or an explicit human decision identifies one requirement as replacing the other; preserve both nodes and the supersedes relation.

**Commands**

```bash
cargo test -p plumb-functional duplicates
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-functional duplicates`

**Acceptance**

- Exact duplicate fixture is detected deterministically; accepting merge preserves all evidence references.

**Supporting references**

- `SRCREF-BB7E8B02A9` — `docs/plan/merged-implementation-plan-v2.md §7:M1.4`
- `SRCREF-97536C2180` — `docs/standards/PLUMB-VALIDATION-RULEBOOK-2026.1.md F1 duplicate rule`


### `S1.4` — Implement deterministic requirement lint pack and corpus benchmark

**Phase:** `S1`  
**Scope:** `NOW`  
**Dependencies:** `S1.3`  
**Commit:** `S1.4: Implement deterministic requirement lint pack and corpus benchmark`

**Write allowlist**

- `crates/plumb-lint/src/lib.rs`
- `crates/plumb-lint/src/rules.rs`
- `crates/plumb-lint/src/benchmark.rs`
- `crates/plumb-lint/tests/rules.rs`
- `fixtures/lint-corpus/corpus.jsonl`
- `fixtures/lint-corpus/baseline.json`

**Required actions**

1. Implement the 15 pilot lint rules listed in v2 M2.1 with stable IDs.
2. Each lint result references exact EvidenceFragment range.
3. Benchmark precision/recall per rule; if measured precision is below 0.85, effective severity is warn through configuration, not code mutation.
4. Regression test fails if precision drops by more than 0.03 from baseline.

**Commands**

```bash
cargo test -p plumb-lint
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-lint`

**Acceptance**

- All 15 rule fixtures pass and the 200-sentence corpus baseline is committed with annotator agreement metadata.

**Supporting references**

- `SRCREF-EE31682F6E` — `docs/plan/merged-implementation-plan-v2.md §7:M2.1-M2.3`


### `S1.5` — Implement vocabulary normalization and typed concept proposals

**Phase:** `S1`  
**Scope:** `NOW`  
**Dependencies:** `S1.4`  
**Commit:** `S1.5: Implement vocabulary normalization and typed concept proposals`

**Write allowlist**

- `crates/plumb-functional/src/vocabulary.rs`
- `crates/plumb-functional/src/vocabulary_exceptions.rs`
- `crates/plumb-functional/tests/vocabulary.rs`
- `prompts/s1-vocabulary.md`
- `schemas/inference/s1-vocabulary.schema.json`

**Required actions**

1. Implement NFKC, case normalization, determiner stripping and the explicit singularization exception table stored in crates/plumb-functional/src/vocabulary_exceptions.rs.
2. Generate Term candidates with frequency and co-occurrence.
3. Inference may propose Concept kind only from object_type, fact_type, value_type, role, other.
4. Conflicting type proposals create findings; they do not overwrite accepted concepts.

**Commands**

```bash
cargo test -p plumb-functional vocabulary
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-functional vocabulary`

**Acceptance**

- Vocabulary output is deterministic and HR reference typing reaches the v2 target where fixture labels exist.

**Supporting references**

- `SRCREF-7B43AD5876` — `docs/plan/merged-implementation-plan-v2.md §7:M3.1-M3.3`
- `SRCREF-C0AC034159` — `docs/architecture/PLUMB-METAMODEL-v3.md §8`


### `S1.6` — Implement all F1 validation rules

**Phase:** `S1`  
**Scope:** `NOW`  
**Dependencies:** `S1.5`  
**Commit:** `S1.6: Implement all F1 validation rules`

**Write allowlist**

- `crates/plumb-validation/src/rules/f1.rs`
- `crates/plumb-validation/tests/f1.rs`

**Required actions**

1. Implement all eleven F1 evaluators from the supplied rule YAML and rulebook.
2. Each evaluator returns target IDs and deterministic semantic_condition_key.
3. Rules classified external_standard use the supplied metadata only; code must not claim clause-level compliance.
4. F1 gate result must include unresolved lint blockers, vocabulary blockers, duplicates and contradictions.

**Commands**

```bash
cargo test -p plumb-validation f1
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-validation f1`

**Acceptance**

- All eleven F1 rules have PASS and FAIL fixtures; HR requirements baseline reaches F1 only after required fixture resolutions.

**Supporting references**

- `SRCREF-273CB17B27` — `docs/standards/PLUMB-VALIDATION-RULEBOOK-2026.1.md §F1`
- `SRCREF-0DA654DB72` — `config/profiles/plumb-software-2026.1-rules.yaml rules where gate=F1`


### `S2.1` — Implement entity, attribute and domain-relationship proposals

**Phase:** `S2`  
**Scope:** `NOW`  
**Dependencies:** `S1.6`  
**Commit:** `S2.1: Implement entity, attribute and domain-relationship proposals`

**Write allowlist**

- `crates/plumb-functional/src/domain.rs`
- `crates/plumb-functional/tests/domain.rs`
- `prompts/s2-domain.md`
- `schemas/inference/s2-domain.schema.json`

**Required actions**

1. Inference proposals are closed to accepted vocabulary Concept IDs; unknown terms are rejected and returned as findings.
2. Attributes require owning Entity, value_type and nullable.
3. DomainRelationship cardinality is populated only when stated or later human-resolved; inference cannot silently choose missing cardinality.
4. Consolidation produces MergeNodes proposals, never direct mutation.

**Commands**

```bash
cargo test -p plumb-functional domain
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-functional domain`

**Acceptance**

- Unknown vocabulary is rejected; accepted attribute has exactly one owner; HR entity survival meets the v2 fixture target.

**Supporting references**

- `SRCREF-15D96DDAA9` — `docs/plan/merged-implementation-plan-v2.md §7:M4.1-M4.2`
- `SRCREF-C0AC034159` — `docs/architecture/PLUMB-METAMODEL-v3.md §8`


### `S2.2` — Implement states, transitions and invariants

**Phase:** `S2`  
**Scope:** `NOW`  
**Dependencies:** `S2.1`  
**Commit:** `S2.2: Implement states, transitions and invariants`

**Write allowlist**

- `crates/plumb-functional/src/state.rs`
- `crates/plumb-functional/src/invariant.rs`
- `crates/plumb-functional/tests/state.rs`

**Required actions**

1. Create State and Transition proposals from lifecycle evidence.
2. Transition requires stateful_ref, from_state, to_state and trigger_ref; missing trigger is a finding/question.
3. Invariant expressions are PlumbExpr proposals and cannot become Accepted until parse/typecheck succeeds.
4. No lifecycle state is inferred solely from capitalization or field names.

**Commands**

```bash
cargo test -p plumb-functional state
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-functional state`

**Acceptance**

- State graph rejects missing endpoints/trigger; HR state fixture validates.

**Supporting references**

- `SRCREF-B04CD24E97` — `docs/plan/merged-implementation-plan-v2.md §7:M4.2`
- `SRCREF-48F010D7D1` — `docs/architecture/PLUMB-METAMODEL-v3.md §§8.8-8.10`


### `S2.3` — Implement data classification dictionary proposals

**Phase:** `S2`  
**Scope:** `NOW`  
**Dependencies:** `S2.2`  
**Commit:** `S2.3: Implement data classification dictionary proposals`

**Write allowlist**

- `crates/plumb-functional/src/data_class.rs`
- `crates/plumb-functional/tests/data_class.rs`

**Required actions**

1. Implement explicit deterministic dictionary entries for the v2 pilot examples including email, dob, salary and iban.
2. Dictionary hit creates proposed classification with deterministic derivation.
3. Unknown classification may be proposed by inference only through a separate proposal; absence remains unknown rather than defaulting to none.

**Commands**

```bash
cargo test -p plumb-functional data_class
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-functional data_class`

**Acceptance**

- Known dictionary values classify deterministically; unknown values remain unresolved until accepted proposal/decision.

**Supporting references**

- `SRCREF-E208E34574` — `docs/plan/merged-implementation-plan-v2.md §7:M4.3`


### `S2.4` — Implement PlumbExpr grammar and typed AST

**Phase:** `S2`  
**Scope:** `NOW`  
**Dependencies:** `S2.3`  
**Commit:** `S2.4: Implement PlumbExpr grammar and typed AST`

**Write allowlist**

- `crates/plumb-expr/src/lib.rs`
- `crates/plumb-expr/src/grammar.pest`
- `crates/plumb-expr/src/ast.rs`
- `crates/plumb-expr/src/types.rs`
- `crates/plumb-expr/src/parser.rs`
- `crates/plumb-expr/tests/parser.rs`

**Required actions**

1. Implement literals, dotted identifiers, arithmetic, comparisons, boolean operators, calls, in and lists exactly as v2 M5.1.
2. Implement Ty variants exactly as inherited v2 type contract unless contradicted by metamodel; preserve Decimal precision, Date, DateTime, Duration, Quantity, Enum, Ref and List.
3. Parser never executes code and exposes no scripting escape.

**Commands**

```bash
cargo test -p plumb-expr parser
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-expr parser`

**Acceptance**

- Parse/unparse property tests pass and precedence table is covered.

**Supporting references**

- `SRCREF-D23EBEA264` — `docs/plan/merged-implementation-plan-v2.md §§6 plumb-expr,7:M5.1`
- `SRCREF-43093678EA` — `docs/plan/PLUMB-v2-to-v3-IMPLEMENTATION-DELTA.md §2 PlumbExpr`


### `S2.5` — Implement PlumbExpr type checker, evaluator and calendars

**Phase:** `S2`  
**Scope:** `NOW`  
**Dependencies:** `S2.4`  
**Commit:** `S2.5: Implement PlumbExpr type checker, evaluator and calendars`

**Write allowlist**

- `crates/plumb-expr/src/typecheck.rs`
- `crates/plumb-expr/src/eval.rs`
- `crates/plumb-expr/src/calendar.rs`
- `crates/plumb-expr/src/render.rs`
- `crates/plumb-expr/tests/eval.rs`

**Required actions**

1. Implement same-unit add/subtract, scalar multiply/divide, date/duration, enum membership and Ref navigation.
2. Implement decimal evaluation with rust_decimal.
3. Implement half-up, half-even, floor, ceil and to-step rounding.
4. Implement working_days(period, calendar, inclusive), days_between and aggregates using injected CalendarProvider.
5. Do not implement as_of(version) in NOW scope.
6. Implement deterministic plain-English example renderer.

**Commands**

```bash
cargo test -p plumb-expr eval
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-expr eval`

**Acceptance**

- All v2 rounding, holiday, weekend and inclusive/exclusive fixtures pass; well-typed property-generated expressions do not produce type errors.

**Supporting references**

- `SRCREF-C900B2AE78` — `docs/plan/merged-implementation-plan-v2.md §7:M5.2-M5.4`


### `S2.6` — Implement calculations and decision tables

**Phase:** `S2`  
**Scope:** `NOW`  
**Dependencies:** `S2.5`  
**Commit:** `S2.6: Implement calculations and decision tables`

**Write allowlist**

- `crates/plumb-functional/src/calculation.rs`
- `crates/plumb-functional/src/decision_table.rs`
- `crates/plumb-functional/tests/calculation.rs`
- `crates/plumb-functional/tests/decision_table.rs`
- `prompts/s2-calculation.md`
- `schemas/inference/s2-calculation.schema.json`

**Required actions**

1. Calculation inference returns PlumbExpr proposal plus target/result metadata; evaluate typechecks before proposal can be accepted.
2. Detect undefined input, unit mismatch and calculation cycles deterministically.
3. DecisionTable implements typed input/output columns, rows, default and hit policy.
4. Implement overlap and completeness checks for boolean/enumeration domains and analyzable numeric intervals.

**Commands**

```bash
cargo test -p plumb-functional calculation
cargo test -p plumb-functional decision_table
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-functional calculation`
- `cargo test -p plumb-functional decision_table`

**Acceptance**

- HR half-day/holiday/inclusive-end fixtures raise the expected gap family; complete/incomplete/overlap table fixtures pass.

**Supporting references**

- `SRCREF-E6CF46B968` — `docs/plan/merged-implementation-plan-v2.md §7:M6.1-M6.2`
- `SRCREF-9175A495B9` — `docs/architecture/PLUMB-METAMODEL-v3.md §§9.7-9.9`


### `S2.7` — Implement operations, outcomes, events and read/write semantics

**Phase:** `S2`  
**Scope:** `NOW`  
**Dependencies:** `S2.6`  
**Commit:** `S2.7: Implement operations, outcomes, events and read/write semantics`

**Write allowlist**

- `crates/plumb-functional/src/operation.rs`
- `crates/plumb-functional/src/event.rs`
- `crates/plumb-functional/tests/operation.rs`

**Required actions**

1. Compile Operation kind command/query, inputs/outputs, preconditions and postconditions.
2. Represent reads, writes, produces, consumes, governed_by and uses_calculation only as typed PSG relations.
3. Failure outcomes are Outcome nodes; event payload references typed DataSchema or domain attributes as permitted by metamodel.
4. Derive read/write candidate sets from accepted criteria, invariants, calculations and rules; human acceptance is required when derivation is not purely structural.

**Commands**

```bash
cargo test -p plumb-functional operation
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-functional operation`

**Acceptance**

- HR ApproveLeaveRequest read-set includes the v2 required attributes after fixture decisions; invalid untyped references are rejected.

**Supporting references**

- `SRCREF-2DA111CE63` — `docs/plan/merged-implementation-plan-v2.md §7:M6.4`
- `SRCREF-64C0217743` — `docs/architecture/PLUMB-METAMODEL-v3.md §§9.1-9.3,17.3`


### `S2.8` — Implement separate business-role and security authorization semantics

**Phase:** `S2`  
**Scope:** `NOW`  
**Dependencies:** `S2.7`  
**Commit:** `S2.8: Implement separate business-role and security authorization semantics`

**Write allowlist**

- `crates/plumb-functional/src/authorization.rs`
- `crates/plumb-functional/tests/authorization.rs`

**Required actions**

1. Implement Principal, SecurityRole, Permission, ResourceScope, PolicyCondition and SeparationConstraint using metamodel fields.
2. Keep Actor and BusinessRole in domain semantics; no type alias or shared enum may collapse BusinessRole and SecurityRole.
3. Permission must resolve through typed relations to operation and resource scope.
4. Implement role-hierarchy cycle detection and static separation-of-duty validation.

**Commands**

```bash
cargo test -p plumb-functional authorization
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-functional authorization`

**Acceptance**

- BusinessRole and SecurityRole serialize to distinct NodePayload variants; hierarchy cycles and SOD violations are detected.

**Supporting references**

- `SRCREF-FFC4E55C45` — `docs/architecture/PLUMB-METAMODEL-v3.md §10`
- `SRCREF-3CF65C8502` — `docs/plan/PLUMB-v2-to-v3-IMPLEMENTATION-DELTA.md §3.8`
- `SRCREF-482A7F2096` — `docs/standards/PLUMB-VALIDATION-RULEBOOK-2026.1.md RBAC F2 rules`


### `S2.9` — Implement process model and safe process proposals

**Phase:** `S2`  
**Scope:** `NOW`  
**Dependencies:** `S2.8`  
**Commit:** `S2.9: Implement process model and safe process proposals`

**Write allowlist**

- `crates/plumb-functional/src/process.rs`
- `crates/plumb-functional/tests/process.rs`
- `prompts/s2-process.md`
- `schemas/inference/s2-process.schema.json`

**Required actions**

1. Implement Process and ProcessNode controlled kinds from metamodel §9.5.
2. Validate one or more start semantics as allowed by profile, reachable end semantics, executable task resolution, exclusive conditions and balanced parallel split/join.
3. Build sequence proposals from explicit pre/postconditions, events, accepted state transitions and explicit source evidence.
4. Requirement document order may be attached only as weak evidence metadata and must never create an accepted next relation without human confirmation.
5. Mermaid import/export is deferred until the UI/process projection task.

**Commands**

```bash
cargo test -p plumb-functional process
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-functional process`

**Acceptance**

- A fixture that differs only in requirement paragraph order produces no accepted process-order change; unreachable node and unbalanced parallel fixtures fail validation.

**Supporting references**

- `SRCREF-21ED0F2039` — `docs/architecture/PLUMB-METAMODEL-v3.md §§9.4-9.5`
- `SRCREF-B87594D03C` — `docs/plan/PLUMB-v2-to-v3-IMPLEMENTATION-DELTA.md §3.11`
- `SRCREF-8FF7BA8981` — `docs/plan/merged-implementation-plan-v2.md §7:M8.1`


### `S2.10` — Implement all F2 validation rules

**Phase:** `S2`  
**Scope:** `NOW`  
**Dependencies:** `S2.9`  
**Commit:** `S2.10: Implement all F2 validation rules`

**Write allowlist**

- `crates/plumb-validation/src/rules/f2.rs`
- `crates/plumb-validation/tests/f2.rs`

**Required actions**

1. Implement all twenty-four F2 evaluators exactly from the supplied rule YAML and rulebook.
2. Each evaluator uses typed PSG relations; no evaluator infers coverage from matching names.
3. BPMN/DMN/RBAC-labeled rules are semantic subset checks, not claims of full external-format conformance.
4. PLUMB.F2.NO_UNRESOLVED_SEMANTIC_BLOCKER fails when another open blocker in the functional/domain/security family exists.

**Commands**

```bash
cargo test -p plumb-validation f2
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-validation f2`

**Acceptance**

- All twenty-four rules have PASS/FAIL fixtures; HR functional reference reaches F2 after expert-answer patches.

**Supporting references**

- `SRCREF-0E7CBD4996` — `docs/standards/PLUMB-VALIDATION-RULEBOOK-2026.1.md §F2`
- `SRCREF-E51F20A226` — `config/profiles/plumb-software-2026.1-rules.yaml rules where gate=F2`


### `S3.1` — Implement deterministic Question generation and routing

**Phase:** `S3`  
**Scope:** `NOW`  
**Dependencies:** `S2.10`  
**Commit:** `S3.1: Implement deterministic Question generation and routing`

**Write allowlist**

- `crates/plumb-functional/src/question.rs`
- `crates/plumb-functional/src/question_templates.rs`
- `crates/plumb-functional/tests/question.rs`

**Required actions**

1. Implement QuestionKind set from metamodel §6.2.
2. Map each NOW finding code that can be resolved by a question to an explicit template and answer schema.
3. Suppress duplicates by template ID plus canonical slot values.
4. Priority equals severity weight multiplied by typed dependency blast-radius count using the impact engine.
5. Route using stakeholders.yaml; unresolved routing goes to analyst role, not an invented stakeholder.

**Commands**

```bash
cargo test -p plumb-functional question
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-functional question`

**Acceptance**

- HR round-1 question set and order are deterministic; an unmapped stakeholder never becomes a fabricated person.

**Supporting references**

- `SRCREF-02E317763F` — `docs/plan/merged-implementation-plan-v2.md §7:M7.1`
- `SRCREF-7E54F3404D` — `docs/architecture/PLUMB-METAMODEL-v3.md §6.2`


### `S3.2` — Implement question rounds

**Phase:** `S3`  
**Scope:** `NOW`  
**Dependencies:** `S3.1`  
**Commit:** `S3.2: Implement question rounds`

**Write allowlist**

- `crates/plumb-functional/src/round.rs`
- `crates/plumb-functional/tests/round.rs`

**Required actions**

1. Compose at most profile.question_round_size questions per stakeholder.
2. Sort blocking before non-blocking, then descending priority, then stable question ID.
3. Group by affected entity when grouping does not violate ordering.
4. A question already answered, superseded or stale is excluded.

**Commands**

```bash
cargo test -p plumb-functional round
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-functional round`

**Acceptance**

- Round size never exceeds 15 under the default profile and output is deterministic.

**Supporting references**

- `SRCREF-674B81DA6A` — `docs/plan/merged-implementation-plan-v2.md §7:M7.2`


### `S3.3` — Implement ResolutionDecision and answer-to-patch application

**Phase:** `S3`  
**Scope:** `NOW`  
**Dependencies:** `S3.2`  
**Commit:** `S3.3: Implement ResolutionDecision and answer-to-patch application`

**Write allowlist**

- `crates/plumb-functional/src/resolution.rs`
- `crates/plumb-functional/tests/resolution.rs`

**Required actions**

1. Every accepted answer creates ResolutionDecision with question/proposal reference, answer, actor, timestamp, rationale when required and patch artifact ref.
2. Implement explicit answer mappers for cardinality, state/transition, outcome, actor allowance, permission, unit, precision, rounding, formula, rule cell, calendar and attribute type.
3. Patch preview is generated before commit.
4. Superseding a prior decision applies a new explicit patch based on current graph; it does not rely on opaque object inversion.

**Commands**

```bash
cargo test -p plumb-functional resolution
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-functional resolution`

**Acceptance**

- Applying expert-answers.yaml only through answer mappers resolves expected blockers; every graph change has a serialized patch artifact and decision reference.

**Supporting references**

- `SRCREF-AE75140DD6` — `docs/architecture/PLUMB-METAMODEL-v3.md §6.3`
- `SRCREF-A95FED2B41` — `docs/plan/merged-implementation-plan-v2.md §7:M7.3`
- `SRCREF-8A575EA9B2` — `docs/plan/PLUMB-v2-to-v3-IMPLEMENTATION-DELTA.md §3.3`


### `S3.4` — Implement assumptions and governed waivers

**Phase:** `S3`  
**Scope:** `NOW`  
**Dependencies:** `S3.3`  
**Commit:** `S3.4: Implement assumptions and governed waivers`

**Write allowlist**

- `crates/plumb-functional/src/assumption.rs`
- `crates/plumb-validation/src/waiver.rs`
- `crates/plumb-functional/tests/assumption.rs`

**Required actions**

1. Assumption requires statement, owner_ref and status; expiry/review is required when it is used to satisfy a blocking gate condition.
2. Expired assumptions become findings using injected Clock.
3. A waiver is represented by ResolutionDecision and may only satisfy a rule if the YAML waiver_policy permits it.
4. Forbidden waiver attempts return validation error and do not change gate result.

**Commands**

```bash
cargo test -p plumb-functional assumption
cargo test -p plumb-validation waiver
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-functional assumption`
- `cargo test -p plumb-validation waiver`

**Acceptance**

- Expired assumption and forbidden waiver fixtures fail as specified.

**Supporting references**

- `SRCREF-860566BE13` — `docs/architecture/PLUMB-METAMODEL-v3.md §6.4`
- `SRCREF-003999438A` — `docs/standards/PLUMB-VALIDATION-RULEBOOK-2026.1.md §§2.3,F3`


### `S3.5` — Implement all F3 validation rules

**Phase:** `S3`  
**Scope:** `NOW`  
**Dependencies:** `S3.4`  
**Commit:** `S3.5: Implement all F3 validation rules`

**Write allowlist**

- `crates/plumb-validation/src/rules/f3.rs`
- `crates/plumb-validation/tests/f3.rs`

**Required actions**

1. Implement all six F3 evaluators exactly from the supplied rule YAML and rulebook.

**Commands**

```bash
cargo test -p plumb-validation f3
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-validation f3`

**Acceptance**

- All six F3 rules have PASS/FAIL fixtures and the HR fixture passes after expert answers.

**Supporting references**

- `SRCREF-040CACC2B1` — `docs/standards/PLUMB-VALIDATION-RULEBOOK-2026.1.md §F3`
- `SRCREF-F9131DF9AF` — `config/profiles/plumb-software-2026.1-rules.yaml rules where gate=F3`


### `S4.1` — Implement Scenario model and deterministic scenario derivation

**Phase:** `S4`  
**Scope:** `NOW`  
**Dependencies:** `S3.5`  
**Commit:** `S4.1: Implement Scenario model and deterministic scenario derivation`

**Write allowlist**

- `crates/plumb-functional/src/scenario.rs`
- `crates/plumb-functional/tests/scenario.rs`
- `prompts/s4-scenario.md`
- `schemas/inference/s4-scenario.schema.json`

**Required actions**

1. Implement Scenario fields/kinds exactly from metamodel §9.10.
2. Derive rule-row, state-transition and boundary scenario skeletons deterministically from accepted semantics.
3. Inference may fill realistic values only within accepted enums, units, ranges and types.
4. Every scenario carries requirement_refs or derived_from_refs sufficient to trace its obligation.
5. Inference-created expected outcomes require HUMAN_CONFIRM.

**Commands**

```bash
cargo test -p plumb-functional scenario
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-functional scenario`

**Acceptance**

- 0.5-day and holiday boundary skeletons are derived deterministically; invalid generated values are rejected.

**Supporting references**

- `SRCREF-B79FB10635` — `docs/architecture/PLUMB-METAMODEL-v3.md §9.10`
- `SRCREF-838C22696F` — `docs/plan/merged-implementation-plan-v2.md §7:M8.3`
- `SRCREF-0765F7CC1C` — `docs/architecture/PLUMB-COMPILER-ARCHITECTURE-v3.md §10`


### `S4.2` — Implement pure functional interpreter materialization and execution

**Phase:** `S4`  
**Scope:** `NOW`  
**Dependencies:** `S4.1`  
**Commit:** `S4.2: Implement pure functional interpreter materialization and execution`

**Write allowlist**

- `crates/plumb-sim/src/lib.rs`
- `crates/plumb-sim/src/model.rs`
- `crates/plumb-sim/src/materialize.rs`
- `crates/plumb-sim/src/execute.rs`
- `crates/plumb-sim/tests/execute.rs`

**Required actions**

1. Implement pure run(model, scenario) returning Pass, Fail{diff} or Undecidable{missing} with semantic trace.
2. Materialize Given state from typed scenario values.
3. Execute actor authorization, preconditions, decision tables, calculations, invariants, writes, state transitions and events.
4. Missing semantic values return Undecidable and name the missing element IDs; never substitute defaults unless they are accepted assumptions in PSG.

**Commands**

```bash
cargo test -p plumb-sim execute
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-sim execute`

**Acceptance**

- Pre-answer HR half-day, holiday and inclusive-end cases are Undecidable for the expected missing elements; no network/I/O occurs in run().

**Supporting references**

- `SRCREF-E40232A0FE` — `docs/plan/merged-implementation-plan-v2.md §§1 Interpreter,7:M9.1-M9.2`
- `SRCREF-0765F7CC1C` — `docs/architecture/PLUMB-COMPILER-ARCHITECTURE-v3.md §10`


### `S4.3` — Implement Then comparison and scenario traces

**Phase:** `S4`  
**Scope:** `NOW`  
**Dependencies:** `S4.2`  
**Commit:** `S4.3: Implement Then comparison and scenario traces`

**Write allowlist**

- `crates/plumb-sim/src/compare.rs`
- `crates/plumb-sim/src/trace.rs`
- `crates/plumb-sim/tests/compare.rs`

**Required actions**

1. Compare expected states, values, outcomes and events using typed equality/unit semantics.
2. Structured diff includes semantic element ID, expected value and actual value.
3. Trace contains ordered business-semantic steps, rule/calculation references and event emissions.
4. Persist trace as scenario-trace artifact outside the pure run() boundary.

**Commands**

```bash
cargo test -p plumb-sim compare
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-sim compare`

**Acceptance**

- Fail fixtures produce stable structured diffs; trace ordering is deterministic.

**Supporting references**

- `SRCREF-08F3AED210` — `docs/plan/merged-implementation-plan-v2.md §7:M9.3`
- `SRCREF-A37A8FF581` — `docs/architecture/PLUMB-METAMODEL-v3.md §15.3`


### `S4.4` — Implement dirty-set scenario re-execution and scenario findings

**Phase:** `S4`  
**Scope:** `NOW`  
**Dependencies:** `S4.3`  
**Commit:** `S4.4: Implement dirty-set scenario re-execution and scenario findings`

**Write allowlist**

- `crates/plumb-sim/src/incremental.rs`
- `crates/plumb-sim/tests/incremental.rs`

**Required actions**

1. Select scenarios for re-execution only when DirtySet reaches a referenced requirement, operation, rule, calculation, state/transition, event or permission.
2. Emit stable findings for scenario fail and blocking undecidable using deterministic finding keys.
3. A view-only change reruns zero scenarios.

**Commands**

```bash
cargo test -p plumb-sim incremental
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-sim incremental`

**Acceptance**

- Changing LeaveBalance calculation reruns dependent leave scenarios; unrelated glossary/view change does not.

**Supporting references**

- `SRCREF-B1A87347A8` — `docs/plan/merged-implementation-plan-v2.md §7:M9.4`
- `SRCREF-7DDD53DC7B` — `docs/architecture/PLUMB-COMPILER-ARCHITECTURE-v3.md §22`


### `S4.5` — Implement minimal VerificationObligation model for F4 applicability

**Phase:** `S4`  
**Scope:** `NOW`  
**Dependencies:** `S4.4`  
**Commit:** `S4.5: Implement minimal VerificationObligation model for F4 applicability`

**Write allowlist**

- `crates/plumb-functional/src/verification.rs`
- `crates/plumb-functional/tests/verification.rs`

**Required actions**

1. Expose VerificationObligation and verification_kind types already present in NodePayload.
2. For NOW scope generate obligations only as deterministic/proposed links required to express F4 applicability; do not implement D2 planning.
3. Functional executable behavior may map to scenario obligation; non-executable requirements may map to analysis, inspection, review, demonstration, formal_check, architecture_check or security_check only through explicit human/profile decision.
4. Do not generate TestCase automatically in NOW scope.

**Commands**

```bash
cargo test -p plumb-functional verification
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-functional verification`

**Acceptance**

- A quality/documentation requirement can be F4-verifiable without a fake executable scenario; executable operation requirement receives scenario linkage.

**Supporting references**

- `SRCREF-FFF9705B50` — `docs/architecture/PLUMB-METAMODEL-v3.md §15.1`
- `SRCREF-441E1C9F5A` — `docs/standards/PLUMB-VALIDATION-RULEBOOK-2026.1.md §F4`
- `SRCREF-AD0A0F5D57` — `docs/plan/PLUMB-v2-to-v3-IMPLEMENTATION-DELTA.md §3.10`


### `S4.6` — Implement all F4 validation rules

**Phase:** `S4`  
**Scope:** `NOW`  
**Dependencies:** `S4.5`  
**Commit:** `S4.6: Implement all F4 validation rules`

**Write allowlist**

- `crates/plumb-validation/src/rules/f4.rs`
- `crates/plumb-validation/tests/f4.rs`

**Required actions**

1. Implement all seven F4 evaluators exactly from the supplied rule YAML and rulebook.
2. Do not use the v2 universal every-requirement scenario rule.
3. RUN.NO_BLOCKING_UNDECIDABLE considers only blocking executable scenarios under current profile.

**Commands**

```bash
cargo test -p plumb-validation f4
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-validation f4`

**Acceptance**

- All seven F4 rules have PASS/FAIL fixtures; HR fixture passes F4 after expert answers and required scenario confirmation.

**Supporting references**

- `SRCREF-441E1C9F5A` — `docs/standards/PLUMB-VALIDATION-RULEBOOK-2026.1.md §F4`
- `SRCREF-2AC9336C3C` — `config/profiles/plumb-software-2026.1-rules.yaml rules where gate=F4`
- `SRCREF-AD0A0F5D57` — `docs/plan/PLUMB-v2-to-v3-IMPLEMENTATION-DELTA.md §3.10`


### `S4.7` — Implement functional pilot projections and reports

**Phase:** `S4`  
**Scope:** `NOW`  
**Dependencies:** `S4.6`  
**Commit:** `S4.7: Implement functional pilot projections and reports`

**Write allowlist**

- `crates/plumb-functional/src/report.rs`
- `crates/plumb-functional/src/projections.rs`
- `crates/plumb-functional/tests/projections.rs`

**Required actions**

1. Generate functional.yaml, glossary.md, scenarios.yaml, decisions.jsonl, model-report.md and gate-report JSON from PSG.
2. Each machine projection records or accompanies source semantic_hash and projection content hash.
3. Sort all projected collections deterministically.
4. Readiness may be reported as advisory only and cannot alter gate result.

**Commands**

```bash
cargo test -p plumb-functional projections
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-functional projections`

**Acceptance**

- Golden outputs are byte-identical on repeated runs and every gate report remains independent from readiness.

**Supporting references**

- `SRCREF-027DE8227D` — `docs/plan/merged-implementation-plan-v2.md §§7:M10.3,12`
- `SRCREF-6581F3212C` — `docs/architecture/PLUMB-METAMODEL-v3.md §26`
- `SRCREF-A18636D6DA` — `docs/plan/PLUMB-v2-to-v3-IMPLEMENTATION-DELTA.md §3.12`


### `X1.1` — Implement Anthropic InferenceProvider behind llm feature

**Phase:** `CrossCutting`  
**Scope:** `NOW`  
**Dependencies:** `S4.7`  
**Commit:** `X1.1: Implement Anthropic InferenceProvider behind llm feature`

**Write allowlist**

- `crates/plumb-inference/src/anthropic.rs`
- `crates/plumb-inference/src/provider.rs`
- `crates/plumb-inference/tests/provider_contract.rs`
- `scripts/manual-llm-smoke.sh`

**Required actions**

1. Add AnthropicProvider under the plumb-inference Cargo feature anthropic using reqwest. The plumb-api and plumb-cli llm features enable plumb-inference/anthropic as defined in §5.4.
2. Provider accepts only InferenceRequest and returns InferenceArtifact; caller supplies JSON schema/prompt artifacts.
3. Record provider, model, parameters, raw_response_hash and validated_output_hash.
4. No live provider test runs in CI; manual smoke script requires ANTHROPIC_API_KEY and exits with a clear error when absent.
5. Provider/model string is configuration, not hard-coded semantic truth.

**Commands**

```bash
cargo test -p plumb-inference --no-default-features
cargo test -p plumb-inference --features anthropic
if cargo tree -p plumb-inference --no-default-features -e normal | grep -qE '(^|[[:space:]])reqwest v'; then
  echo "ERROR: reqwest present in feature-off plumb-inference dependency graph" >&2
  exit 1
fi
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-inference --no-default-features`
- `cargo test -p plumb-inference --features anthropic`

**Acceptance**

- Mock/Null contract tests pass; feature-off build has no network dependency path; manual script is documented.

**Supporting references**

- `SRCREF-35F1EE7D59` — `docs/plan/merged-implementation-plan-v2.md §7:M12.1`
- `SRCREF-D91B3DB99E` — `docs/architecture/PLUMB-COMPILER-ARCHITECTURE-v3.md §20`

**Task-specific prohibitions**

- Do not let AnthropicProvider access store/graph.
- Do not make Anthropic the only possible trait implementation.


### `X1.2` — Implement lexical BM25 retrieval index

**Phase:** `CrossCutting`  
**Scope:** `NOW`  
**Dependencies:** `X1.1`  
**Commit:** `X1.2: Implement lexical BM25 retrieval index`

**Write allowlist**

- `crates/plumb-intake/src/lib.rs`
- `crates/plumb-intake/src/bm25.rs`
- `crates/plumb-intake/src/index.rs`
- `crates/plumb-intake/tests/bm25.rs`

**Required actions**

1. Index requirement statements, domain concepts, rules, calculations, scenarios and evidence text.
2. Implement deterministic lexical BM25 in Rust; no external search dependency.
3. Incremental update must produce identical ranking to full rebuild for equal corpus.
4. EmbeddingProvider path is deferred; retain extension interface only if already defined in v2-compatible API.

**Commands**

```bash
cargo test -p plumb-intake bm25
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-intake bm25`

**Acceptance**

- Top-k is deterministic and incremental index equals rebuild.

**Supporting references**

- `SRCREF-5BCC6696BB` — `docs/plan/merged-implementation-plan-v2.md §7:M13.1`


### `X1.3` — Implement patch-centric intake gate

**Phase:** `CrossCutting`  
**Scope:** `NOW`  
**Dependencies:** `X1.2`  
**Commit:** `X1.3: Implement patch-centric intake gate`

**Write allowlist**

- `crates/plumb-intake/src/intake.rs`
- `crates/plumb-intake/src/report.rs`
- `crates/plumb-intake/tests/intake.rs`

**Required actions**

1. Input is Proposal containing SemanticPatch plus evidence/derivation refs.
2. Resolve terms, covered/equivalent semantics, duplicate candidates, refinements, contradictions and novelty.
3. Conflict checks reuse validation/typed graph logic; do not implement a second independent conflict engine.
4. Return IntakeReport and requires_reason; intake never commits a patch.
5. A proposal flagged requires_reason cannot be accepted without recorded ResolutionDecision rationale.

**Commands**

```bash
cargo test -p plumb-intake intake
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-intake intake`

**Acceptance**

- Existing requirement proposal reports self coverage; conflicting proposal reuses the same semantic finding as direct validation; no commit occurs inside intake.

**Supporting references**

- `SRCREF-DC2572BD78` — `docs/plan/merged-implementation-plan-v2.md §7:M13.2-M13.3`
- `SRCREF-7CA117913F` — `docs/architecture/PLUMB-COMPILER-ARCHITECTURE-v3.md §5.4`


### `X1.4` — Implement transcript-as-source session pipeline

**Phase:** `CrossCutting`  
**Scope:** `NOW`  
**Dependencies:** `X1.3`  
**Commit:** `X1.4: Implement transcript-as-source session pipeline`

**Write allowlist**

- `crates/plumb-session/src/lib.rs`
- `crates/plumb-session/src/model.rs`
- `crates/plumb-session/src/turn.rs`
- `crates/plumb-session/tests/turn.rs`

**Required actions**

1. Persist each user/assistant turn as SourceArtifact/EvidenceFragment of kind transcript.
2. Turn pipeline is retrieve -> build bounded context -> create InferenceRequest -> acquire persisted artifact -> validate proposals -> intake -> display.
3. Session may ask only an existing open Question ID; an invented question ID is rejected.
4. Every proposal from a turn must cite the user-turn EvidenceFragment or accepted PSG evidence.
5. Session has no direct graph-write method.

**Commands**

```bash
cargo test -p plumb-session
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-session`

**Acceptance**

- Proposal without user-turn/evidence ref is rejected; invented question ID is rejected; context ordering is deterministic.

**Supporting references**

- `SRCREF-FB7BC75809` — `docs/plan/merged-implementation-plan-v2.md §7:M14.1-M14.2`
- `SRCREF-96D5EDE97F` — `docs/architecture/PLUMB-COMPILER-ARCHITECTURE-v3.md §§3,5.4`


### `X1.5` — Implement asynchronous stage job base-revision protection

**Phase:** `CrossCutting`  
**Scope:** `NOW`  
**Dependencies:** `X1.4`  
**Commit:** `X1.5: Implement asynchronous stage job base-revision protection`

**Write allowlist**

- `crates/plumb-compiler/src/job.rs`
- `crates/plumb-compiler/tests/job.rs`

**Required actions**

1. Job captures base_revision, base_semantic_hash, stage, scope, profile_hash and config_hash.
2. Completed proposal is STALE when current branch head differs from base_revision.
3. Stale output may be re-evaluated against current head but must never be directly committed.
4. Expose deterministic job status model: queued, acquiring, evaluating, ready, stale, failed, committed.

**Commands**

```bash
cargo test -p plumb-compiler job
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-compiler job`

**Acceptance**

- A simulated concurrent branch update marks earlier job stale and prevents commit.

**Supporting references**

- `SRCREF-5D455D80AE` — `docs/architecture/PLUMB-COMPILER-ARCHITECTURE-v3.md §21`
- `SRCREF-5FE2D971AC` — `docs/plan/PLUMB-v2-to-v3-IMPLEMENTATION-DELTA.md §3.6`


### `A0.1` — Generate pilot OpenAPI contract with v3 overrides

**Phase:** `Platform`  
**Scope:** `NOW`  
**Dependencies:** `X1.5`  
**Commit:** `A0.1: Generate pilot OpenAPI contract with v3 overrides`

**Write allowlist**

- `crates/plumb-api/src/lib.rs`
- `crates/plumb-api/src/openapi.rs`
- `api/modeller.openapi.json`
- `docs/api.md`
- `crates/plumb-api/tests/openapi.rs`

**Required actions**

1. Implement every P and P-R endpoint listed in v2 plan §5 as a utoipa contract/stub, except apply the explicit v3 overrides in this plan.
2. For each request/response schema, use an HR fixture example when fixture-contract.yaml supplies an object of that semantic type; choose the lexicographically smallest stable ID when multiple fixture objects qualify.
3. If no HR object qualifies, generate the example recursively from the OpenAPI schema with this exact rule: enum -> lexicographically first serialized enum value; boolean -> true; integer -> schema minimum else 1; number -> schema minimum else 1.0; string/date -> `2026-01-15`; string/date-time -> `2026-01-15T12:00:00Z`; other string -> `example`; array -> one generated item; object -> all required properties only, in lexicographic property-name order; oneOf/anyOf -> branch with lexicographically smallest RFC-8785 canonical schema JSON; nullable is ignored when a non-null branch exists.
4. Generated examples must validate against the generated schema. If a format/pattern/constraint makes the deterministic fallback invalid and no HR example exists, stop A0.1 with BLOCKED-TEST-CONTRACT naming the endpoint/schema; do not invent a value.
5. GET /api/model/version response is {version, hash, revision, semantic_hash, evidence_hash}; version/hash are compatibility aliases of revision/semantic_hash.
6. GET /api/gates returns I0, F1, F2, F3, F4 in this order; later gates are omitted in NOW scope.
7. Every write request requires If-Match containing current semantic_hash; 412 response includes current revision and semantic_hash.
8. Job responses include base_revision and stale status.

**Commands**

```bash
cargo test -p plumb-api openapi
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-api openapi`

**Acceptance**

- OpenAPI is generated from Rust, committed, contains all v2 P/P-R endpoints with overrides, and examples round-trip.

**Supporting references**

- `SRCREF-723A7B45BB` — `docs/plan/merged-implementation-plan-v2.md §§5,7:M11.1`
- `SRCREF-48A9B980DD` — `docs/plan/PLUMB-v2-to-v3-IMPLEMENTATION-DELTA.md §10`


### `A0.2` — Implement source, requirement, model and gate HTTP handlers

**Phase:** `Platform`  
**Scope:** `NOW`  
**Dependencies:** `A0.1`  
**Commit:** `A0.2: Implement source, requirement, model and gate HTTP handlers`

**Write allowlist**

- `crates/plumb-api/src/state.rs`
- `crates/plumb-api/src/error.rs`
- `crates/plumb-api/src/routes/meta.rs`
- `crates/plumb-api/src/routes/sources.rs`
- `crates/plumb-api/src/routes/requirements.rs`
- `crates/plumb-api/src/routes/model.rs`
- `crates/plumb-api/src/routes/gates.rs`
- `crates/plumb-api/src/routes/mod.rs`
- `crates/plumb-api/tests/http_core.rs`

**Required actions**

1. Back handlers with branch head/revision store and typed PSG.
2. All writes create Proposal/SemanticPatch then pass intake and CAS commit; no handler mutates Graph directly.
3. Errors use {code,message,details}.
4. List pagination uses {items,next_cursor}; a non-null cursor is lowercase hex of UTF-8 RFC-8785 canonical JSON exactly `{"after_id":"<last-returned-id>","semantic_hash":"psg:sha256:..."}`; a cursor bound to a different semantic_hash returns HTTP 409/E_CURSOR_BASELINE_CHANGED.
5. GET reads return ETag equal to semantic_hash.

**Commands**

```bash
cargo test -p plumb-api http_core
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-api http_core`

**Acceptance**

- Wrong If-Match returns 412; pagination returns next_cursor; no handler bypasses patch commit path in tests.

**Supporting references**

- `SRCREF-E08AB8CE58` — `docs/plan/merged-implementation-plan-v2.md §5 conventions`
- `SRCREF-48A9B980DD` — `docs/plan/PLUMB-v2-to-v3-IMPLEMENTATION-DELTA.md §10`


### `A0.3` — Implement questions, proposals, sessions, scenarios and jobs HTTP handlers

**Phase:** `Platform`  
**Scope:** `NOW`  
**Dependencies:** `A0.2`  
**Commit:** `A0.3: Implement questions, proposals, sessions, scenarios and jobs HTTP handlers`

**Write allowlist**

- `crates/plumb-api/src/routes/questions.rs`
- `crates/plumb-api/src/routes/proposals.rs`
- `crates/plumb-api/src/routes/sessions.rs`
- `crates/plumb-api/src/routes/scenarios.rs`
- `crates/plumb-api/src/routes/jobs.rs`
- `crates/plumb-api/tests/http_workflow.rs`

**Required actions**

1. Implement endpoint behavior exactly from v2 §5 through PSG-backed services.
2. Question answer returns proposal_id, intake and patch_preview before commit action.
3. Proposal accept/link/merge/supersede/create/reject uses intake result and ResolutionDecision semantics.
4. Scenario run returns pass/fail/undecidable with trace reference.
5. Jobs expose stale state and never auto-commit stale proposals.

**Commands**

```bash
cargo test -p plumb-api http_workflow
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-api http_workflow`

**Acceptance**

- End-to-end HTTP test answers one question, previews patch, accepts proposal, advances revision and rejects stale If-Match.

**Supporting references**

- `SRCREF-EE99C074D0` — `docs/plan/merged-implementation-plan-v2.md §5 Questions/Proposals/Sessions/Scenarios/Stages`


### `A0.4` — Implement CLI commands over the same application services

**Phase:** `Platform`  
**Scope:** `NOW`  
**Dependencies:** `A0.3`  
**Commit:** `A0.4: Implement CLI commands over the same application services`

**Write allowlist**

- `crates/plumb-cli/src/main.rs`
- `crates/plumb-cli/src/commands.rs`
- `crates/plumb-cli/tests/cli.rs`

**Required actions**

1. Implement import, lint, vocab, extract, consolidate, calc, rules, analyse, questions, rounds, apply, flows, scenarios, run, ready, export, report, serve and log-effort.
2. Every command supports --json where output exists.
3. Exit codes: 0 success, 2 gate failed, 1 operational/spec error.
4. CLI calls the same services as HTTP handlers and cannot bypass intake/patch/revision semantics.

**Commands**

```bash
cargo test -p plumb-cli
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test -p plumb-cli`

**Acceptance**

- assert_cmd tests cover success, gate failure and operational error exit codes.

**Supporting references**

- `SRCREF-1EFC3F56F4` — `docs/plan/merged-implementation-plan-v2.md §7:M11.2`


### `U0.1` — Scaffold strict React UI and generated API client

**Phase:** `UI`  
**Scope:** `NOW`  
**Dependencies:** `A0.1`  
**Commit:** `U0.1: Scaffold strict React UI and generated API client`

**Write allowlist**

- `ui/package.json`
- `ui/package-lock.json`
- `ui/tsconfig.json`
- `ui/vite.config.ts`
- `ui/eslint.config.js`
- `ui/playwright.config.ts`
- `ui/src/main.tsx`
- `ui/src/app/router.tsx`
- `ui/src/api/client.ts`
- `ui/src/api/schema.d.ts`
- `ui/src/test/msw.ts`
- `ui/src/i18n/en.json`
- `ui/src/i18n/pl.json`

**Required actions**

1. Use React 19, TypeScript strict, Vite, TanStack Router/Query, Zustand view-state only, Radix, Tailwind, ELK.js, openapi-fetch, openapi-typescript, MSW, Playwright, axe and the v2 dependency set.
2. Generate schema.d.ts from api/modeller.openapi.json; never hand-edit it.
3. Configure hard-coded JSX string lint except test files and i18n infrastructure.
4. Set package.json scripts exactly to: "typecheck": "tsc --noEmit"; "lint": "eslint ."; "test": "vitest run"; "e2e": "playwright test"; "api:generate": "openapi-typescript ../api/modeller.openapi.json -o src/api/schema.d.ts"; "api:check": "openapi-typescript ../api/modeller.openapi.json -o /tmp/plumb-schema.d.ts && cmp -s /tmp/plumb-schema.d.ts src/api/schema.d.ts".

**Commands**

```bash
cd ui && npm ci && npm run api:generate && npm run api:check && npm run typecheck && npm run lint && npm run test
cd ui && npm run typecheck
cd ui && npm run lint
```

**Tests**

- `cd ui && npm ci && npm run api:generate && npm run api:check && npm run typecheck && npm run lint && npm run test`

**Acceptance**

- Generated client has no drift and UI checks pass.

**Supporting references**

- `SRCREF-25A1B72D82` — `docs/plan/merged-implementation-plan-v2.md §§1 UI stack,8:U0`


### `U1.1` — Implement application shell, role gating and event/version hooks

**Phase:** `UI`  
**Scope:** `NOW`  
**Dependencies:** `U0.1`, `A0.2`  
**Commit:** `U1.1: Implement application shell, role gating and event/version hooks`

**Write allowlist**

- `ui/src/app/AppShell.tsx`
- `ui/src/app/nav.ts`
- `ui/src/app/auth.ts`
- `ui/src/api/useVersionedMutation.ts`
- `ui/src/api/useEvents.ts`
- `ui/src/features/home/HomePage.tsx`
- `ui/src/features/home/HomePage.test.tsx`

**Required actions**

1. Implement v2 pilot navigation exactly, with Gates showing I0-F4.
2. Local auth uses GET /api/me; server roles are authoritative.
3. useVersionedMutation sends If-Match from latest model semantic_hash and handles 412 by invalidating queries and showing ConflictDialog.
4. useEvents invalidates affected query keys on WebSocket event; it never applies semantic state from event payload directly.

**Commands**

```bash
cd ui && npm run test -- HomePage
cd ui && npm run typecheck
cd ui && npm run lint
```

**Tests**

- `cd ui && npm run test -- HomePage`

**Acceptance**

- Shell renders fixture home and wrong-version mutation surfaces conflict without retrying write automatically.

**Supporting references**

- `SRCREF-0C6AD6DAFA` — `docs/plan/merged-implementation-plan-v2.md §8:U1`


### `U1.2` — Implement shared proposal, provenance, findings, diff and typed-answer components

**Phase:** `UI`  
**Scope:** `NOW`  
**Dependencies:** `U1.1`  
**Commit:** `U1.2: Implement shared proposal, provenance, findings, diff and typed-answer components`

**Write allowlist**

- `ui/src/components/ProposalReviewBar.tsx`
- `ui/src/components/ProvenanceDrawer.tsx`
- `ui/src/components/FindingsPanel.tsx`
- `ui/src/components/DiffViewer.tsx`
- `ui/src/components/TypedAnswerControl.tsx`
- `ui/src/components/ConflictDialog.tsx`
- `ui/src/components/GateBadge.tsx`
- `ui/src/components/DiagramFrame.tsx`
- `ui/src/components/shared.test.tsx`

**Required actions**

1. Implement behaviors inherited from v2 U1.3.
2. ProposalReviewBar displays intake status and cannot enable create/accept when server says reason required and rationale is empty.
3. ProvenanceDrawer distinguishes evidence, deterministic derivation, human resolution and LLM inference artifact.
4. GateBadge displays PASS/FAIL/WARN/WAIVED/ERROR without converting to readiness score.
5. DiagramFrame keeps layout state separate from semantic mutation callbacks.

**Commands**

```bash
cd ui && npm run test -- shared
cd ui && npm run typecheck
cd ui && npm run lint
```

**Tests**

- `cd ui && npm run test -- shared`

**Acceptance**

- All shared behavior tests pass including reason enforcement and gate-state display.

**Supporting references**

- `SRCREF-10AF56ADEE` — `docs/plan/merged-implementation-plan-v2.md §8:U1.3`
- `SRCREF-AD9DA89616` — `docs/standards/PLUMB-VALIDATION-RULEBOOK-2026.1.md §2`


### `U2.1` — Implement Requirements and Glossary views

**Phase:** `UI`  
**Scope:** `NOW`  
**Dependencies:** `U1.2`, `A0.2`  
**Commit:** `U2.1: Implement Requirements and Glossary views`

**Write allowlist**

- `ui/src/features/requirements/RequirementsPage.tsx`
- `ui/src/features/requirements/RequirementDetail.tsx`
- `ui/src/features/requirements/requirements.test.tsx`
- `ui/src/features/glossary/GlossaryPage.tsx`
- `ui/src/features/glossary/glossary.test.tsx`

**Required actions**

1. Implement v2 U2 and U3 behaviors.
2. Requirement detail shows source evidence/provenance, lint findings, original text and EARS proposal separately.
3. Every write uses generated API client and versioned mutation hook.
4. Glossary merge displays resulting semantic diff before commit.

**Commands**

```bash
cd ui && npm run test -- requirements glossary
cd ui && npm run typecheck
cd ui && npm run lint
```

**Tests**

- `cd ui && npm run test -- requirements glossary`

**Acceptance**

- Requirement actions send If-Match; source evidence is visible; glossary merge requires diff confirmation.

**Supporting references**

- `SRCREF-3CE600A754` — `docs/plan/merged-implementation-plan-v2.md §8:U2-U3`


### `U2.2` — Implement Sentences/semantic proposal review and Questions views

**Phase:** `UI`  
**Scope:** `NOW`  
**Dependencies:** `U2.1`, `A0.3`  
**Commit:** `U2.2: Implement Sentences/semantic proposal review and Questions views`

**Write allowlist**

- `ui/src/features/sentences/SentencesPage.tsx`
- `ui/src/features/sentences/sentences.test.tsx`
- `ui/src/features/questions/QuestionsPage.tsx`
- `ui/src/features/questions/questions.test.tsx`

**Required actions**

1. Implement v2 U4/U5 semantics.
2. Inline answer creates server proposal and shows patch preview before acceptance.
3. Bulk accept may call one proposal/action per semantic item but must advance ETag after each successful commit; stop on first 412.
4. Question round manager never permits more than profile max.

**Commands**

```bash
cd ui && npm run test -- sentences questions
cd ui && npm run typecheck
cd ui && npm run lint
```

**Tests**

- `cd ui && npm run test -- sentences questions`

**Acceptance**

- Inline answer path produces decision/proposal preview; bulk accept stops on version conflict.

**Supporting references**

- `SRCREF-50924A0118` — `docs/plan/merged-implementation-plan-v2.md §8:U4-U5`


### `U2.3` — Implement chat session view

**Phase:** `UI`  
**Scope:** `NOW`  
**Dependencies:** `U2.2`, `A0.3`  
**Commit:** `U2.3: Implement chat session view`

**Write allowlist**

- `ui/src/features/sessions/SessionPanel.tsx`
- `ui/src/features/sessions/ProposalCard.tsx`
- `ui/src/features/sessions/session.test.tsx`

**Required actions**

1. Implement project/scoped side panel.
2. Render proposal intake results and engine Question IDs.
3. Do not render a model-generated free-form question as an engine question unless API returns an existing question_id.
4. All commit actions use proposal endpoints, not direct model endpoints.

**Commands**

```bash
cd ui && npm run test -- session
cd ui && npm run typecheck
cd ui && npm run lint
```

**Tests**

- `cd ui && npm run test -- session`

**Acceptance**

- Invented question fixture is displayed only as assistant text, not actionable typed question; proposal commit path uses proposal endpoint.

**Supporting references**

- `SRCREF-CC24FC894A` — `docs/plan/merged-implementation-plan-v2.md §8:U6`
- `SRCREF-7CA117913F` — `docs/architecture/PLUMB-COMPILER-ARCHITECTURE-v3.md §5.4`


### `U2.4` — Implement process swimlane view and semantic step edits

**Phase:** `UI`  
**Scope:** `NOW`  
**Dependencies:** `U2.3`  
**Commit:** `U2.4: Implement process swimlane view and semantic step edits`

**Write allowlist**

- `ui/src/features/processes/ProcessesPage.tsx`
- `ui/src/features/processes/ProcessDiagram.tsx`
- `ui/src/features/processes/processes.test.tsx`

**Required actions**

1. Render lanes, steps, branches, events, triggers and outcomes using ELK layout.
2. Support add existing operation, reorder and move lane only.
3. A move/reorder action sends a semantic process patch proposal; visual drag coordinates alone stay local view metadata.
4. Do not implement branch editing or BPMN import in NOW scope.
5. SVG export is view projection only.

**Commands**

```bash
cd ui && npm run test -- processes
cd ui && npm run typecheck
cd ui && npm run lint
```

**Tests**

- `cd ui && npm run test -- processes`

**Acceptance**

- Layout is deterministic for equal data; visual position change alone sends no semantic mutation; semantic move does.

**Supporting references**

- `SRCREF-174DD5C7FE` — `docs/plan/merged-implementation-plan-v2.md §8:U7`
- `SRCREF-A0B54B112C` — `docs/architecture/PLUMB-METAMODEL-v3.md §19`
- `SRCREF-006359FAE9` — `docs/architecture/PLUMB-COMPILER-ARCHITECTURE-v3.md §23`


### `U2.5` — Implement Scenarios view and business-semantic traces

**Phase:** `UI`  
**Scope:** `NOW`  
**Dependencies:** `U2.4`, `A0.3`  
**Commit:** `U2.5: Implement Scenarios view and business-semantic traces`

**Write allowlist**

- `ui/src/features/scenarios/ScenariosPage.tsx`
- `ui/src/features/scenarios/ScenarioTrace.tsx`
- `ui/src/features/scenarios/scenarios.test.tsx`

**Required actions**

1. Render given/when/then, confirmation status, run status and trace.
2. Correction creates proposal; it does not edit accepted scenario locally.
3. Bulk confirmation is limited to derived scenarios returned as confirmable by API.
4. Undecidable trace displays missing semantic element IDs/names.

**Commands**

```bash
cd ui && npm run test -- scenarios
cd ui && npm run typecheck
cd ui && npm run lint
```

**Tests**

- `cd ui && npm run test -- scenarios`

**Acceptance**

- Correction path uses proposal; undecidable fixture names missing element.

**Supporting references**

- `SRCREF-49008E6B37` — `docs/plan/merged-implementation-plan-v2.md §8:U8`


### `U2.6` — Implement Findings, Entities, Rules and Calculations analyst views

**Phase:** `UI`  
**Scope:** `NOW`  
**Dependencies:** `U2.5`, `A0.2`  
**Commit:** `U2.6: Implement Findings, Entities, Rules and Calculations analyst views`

**Write allowlist**

- `ui/src/features/findings/FindingsPage.tsx`
- `ui/src/features/entities/EntitiesPage.tsx`
- `ui/src/features/rules/RulesPage.tsx`
- `ui/src/features/calculations/CalculationsPage.tsx`
- `ui/src/features/analyst/analyst.test.tsx`

**Required actions**

1. Implement v2 U9 NOW/P-R behaviors.
2. Entities view is read-only ER plus CRUD matrix in NOW scope.
3. Rules view exposes uncovered/overlap findings and explicit cell-fill proposal.
4. Calculations show worked example read-only; no free-form formula editor in NOW scope.
5. Findings view distinguishes rule_class external_standard, external_interoperability, plumb_core and profile/organization.

**Commands**

```bash
cd ui && npm run test -- analyst
cd ui && npm run typecheck
cd ui && npm run lint
```

**Tests**

- `cd ui && npm run test -- analyst`

**Acceptance**

- All four views render fixture data and rule-class distinction is visible.

**Supporting references**

- `SRCREF-A59FD84DEF` — `docs/plan/merged-implementation-plan-v2.md §8:U9`
- `SRCREF-62FBC0BA0F` — `docs/standards/PLUMB-VALIDATION-RULEBOOK-2026.1.md §1`


### `U2.7` — Implement Gates and Decisions views

**Phase:** `UI`  
**Scope:** `NOW`  
**Dependencies:** `U2.6`  
**Commit:** `U2.7: Implement Gates and Decisions views`

**Write allowlist**

- `ui/src/features/gates/GatesPage.tsx`
- `ui/src/features/decisions/DecisionsPage.tsx`
- `ui/src/features/gates/gates.test.tsx`
- `ui/src/features/decisions/decisions.test.tsx`

**Required actions**

1. Gates page displays I0-F4 and individual rule results/waivers from GateReport.
2. Do not display a single 'ISO compliant' badge.
3. Decisions log distinguishes ResolutionDecision from architecture/business decision types even when only ResolutionDecision exists in pilot data.
4. Readiness may appear only as advisory secondary data labeled uncalibrated when present.

**Commands**

```bash
cd ui && npm run test -- gates decisions
cd ui && npm run typecheck
cd ui && npm run lint
```

**Tests**

- `cd ui && npm run test -- gates decisions`

**Acceptance**

- A failed blocker remains visually failed even with high readiness; waived rule remains distinct from pass.

**Supporting references**

- `SRCREF-4800CCFA7A` — `docs/standards/PLUMB-VALIDATION-RULEBOOK-2026.1.md §§5-6`
- `SRCREF-FBD9DC478A` — `docs/plan/merged-implementation-plan-v2.md §8:U10`


### `E0.1` — Generate derived HR reference artifacts from the authoritative fixture contract

**Phase:** `E2E`  
**Scope:** `NOW`  
**Dependencies:** `A0.4`, `U2.7`  
**Commit:** `E0.1: Create HR leave pilot fixtures on PSG and compatibility formats`

**Write allowlist**

- `tests/support/hr_fixture.rs`
- `fixtures/hr-leave/reference-psg.yaml`
- `fixtures/hr-leave/reference-functional.yaml`
- `fixtures/hr-leave/inference-fixtures.jsonl`

**Required actions**

1. Treat fixtures/hr-leave/README.md, requirements.md, requirements.docx, fixture-contract.yaml, scenarios.yaml, expert-answers.yaml, stakeholders.yaml and profile.yaml as immutable authoritative fixture inputs.
2. Implement tests/support/hr_fixture.rs to construct the reference accepted PSG deterministically from fixture-contract.yaml and the metamodel; use sorted IDs and exact values from the contract.
3. The builder must leave the three ambiguity answers unresolved in the pre-answer graph and apply them only by passing each expert answer to the S3.3 answer application service, producing a ResolutionDecision and SemanticPatch that commit through the F0.8 CAS revision path.
4. Generate reference-psg.yaml from the post-answer accepted graph and reference-functional.yaml through the functional projection implementation.
5. Generate MockProvider inference-fixtures.jsonl by computing actual InferenceRequest hashes and attaching proposal outputs derived only from fixture-contract.yaml. Mock outputs must not introduce facts absent from the contract.
6. Assert the expected_counts block exactly: 32 requirements, 9 entities, 13 operations, 6 processes, 5 invariants, 2 calculations, 3 rules, 6 events, 44 scenarios.

**Commands**

```bash
cargo test --workspace hr_leave_fixture
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test --workspace hr_leave_fixture`

**Acceptance**

- Derived reference artifacts reproduce the fixture contract exactly and do not mutate authoritative fixture inputs.
- The three ambiguity values appear only after expert answers are applied through ResolutionDecision patches.

**Supporting references**

- `SRCREF-205C5CCA29` — `fixtures/hr-leave/README.md`
- `SRCREF-34CA4A60BC` — `fixtures/hr-leave/fixture-contract.yaml`
- `SRCREF-D47B4005C2` — `docs/plan/merged-implementation-plan-v2.md §§7:M0.3,11`
- `SRCREF-EFAC302AFF` — `docs/plan/PLUMB-v2-to-v3-IMPLEMENTATION-DELTA.md §12`


### `E0.2` — Implement backend end-to-end pilot test

**Phase:** `E2E`  
**Scope:** `NOW`  
**Dependencies:** `E0.1`  
**Commit:** `E0.2: Implement backend end-to-end pilot test`

**Write allowlist**

- `tests/e2e_hr.rs`

**Required actions**

1. Execute import -> I0 -> F1 -> vocabulary/domain -> F2 -> question rounds -> expert answers -> F3 -> scenario derivation/run -> F4 -> export using application services/CLI only.
2. Verify half-day, holiday and inclusive-end ambiguities are Undecidable before answers and resolved only through question answer patches.
3. Verify one conflicting session proposal is rejected through intake.
4. Verify every semantic revision has a serialized patch artifact and every LLM-origin proposal has persisted inference artifact.
5. Verify replay with stored artifacts reproduces identical final semantic_hash without live LLM calls.

**Commands**

```bash
cargo test --test e2e_hr
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
```

**Tests**

- `cargo test --test e2e_hr`

**Acceptance**

- The complete backend pilot reaches F4 and deterministic replay final semantic_hash matches original.

**Supporting references**

- `SRCREF-509798A4E2` — `docs/plan/merged-implementation-plan-v2.md §§11,14`
- `SRCREF-EFAC302AFF` — `docs/plan/PLUMB-v2-to-v3-IMPLEMENTATION-DELTA.md §12`
- `SRCREF-B696289B78` — `docs/architecture/PLUMB-COMPILER-ARCHITECTURE-v3.md §§1-4`


### `E0.3` — Implement Playwright pilot workflow against real backend

**Phase:** `E2E`  
**Scope:** `NOW`  
**Dependencies:** `E0.2`  
**Commit:** `E0.3: Implement Playwright pilot workflow against real backend`

**Write allowlist**

- `ui/e2e/pilot.spec.ts`
- `scripts/run-pilot-e2e.sh`

**Required actions**

1. Start seeded real backend with MockProvider fixture artifacts.
2. Drive UI import -> requirements -> sentences/proposals -> questions -> conflicting chat proposal rejection -> process semantic move -> scenarios -> gates -> export.
3. Assert visible state only; do not inspect application internals from Playwright.
4. Run axe on every navigated route and keyboard-only path through the core workflow.

**Commands**

```bash
make e2e
cd ui && npm run typecheck
cd ui && npm run lint
```

**Tests**

- `make e2e`

**Acceptance**

- Playwright workflow passes with zero serious axe violations and no direct network mocks.

**Supporting references**

- `SRCREF-E16E014AC5` — `docs/plan/merged-implementation-plan-v2.md §8:U11.1-U11.3`


### `E0.4` — Enforce coverage, mutation and no-placeholder quality bar

**Phase:** `E2E`  
**Scope:** `NOW`  
**Dependencies:** `E0.3`  
**Commit:** `E0.4: Enforce coverage, mutation and no-placeholder quality bar`

**Write allowlist**

- `Makefile`
- `.github/workflows/ci.yml`
- `scripts/check-placeholders.sh`

**Required actions**

1. Configure cargo llvm-cov thresholds: >=85% for plumb-psg, plumb-patch, plumb-validation, plumb-expr, plumb-sim, plumb-intake; >=70% other Rust crates.
2. Configure cargo-mutants >=80% caught for validation NOW rules, PlumbExpr evaluator, interpreter and intake.
3. UI Vitest >=80% for src/features and src/components.
4. check-placeholders.sh fails on TODO, FIXME, unimplemented!, todo!, panic!("TODO") and placeholder text in production source; allowlist only generated files and docs.
5. CI runs make all.

**Commands**

```bash
make all
```

**Tests**

- `make all`

**Acceptance**

- All thresholds pass and production source contains no unapproved placeholder implementation.

**Supporting references**

- `SRCREF-C1A6FDDD20` — `docs/plan/merged-implementation-plan-v2.md §§1 Quality bar,10`


### `E0.5` — Produce pilot build and standards-alignment report

**Phase:** `E2E`  
**Scope:** `NOW`  
**Dependencies:** `E0.4`  
**Commit:** `E0.5: Produce pilot build and standards-alignment report`

**Write allowlist**

- `docs/status.md`
- `docs/pilot/BUILD-REPORT.md`
- `docs/pilot/STANDARDS-ALIGNMENT-REPORT.md`

**Required actions**

1. BUILD-REPORT records exact git commit, semantic fixture hash, evidence hash, rule-pack hash, test commands and results.
2. STANDARDS-ALIGNMENT-REPORT reports Plumb Profile Conformant, Standards Aligned, Interchange Valid and Organization Policy Conformant as separate assertions exactly as rulebook §6.
3. For NOW scope, Interchange Valid may report only formats actually validated; it must not claim BPMN/DMN/OpenAPI semantic completeness beyond implemented validators.
4. List every waiver separately.

**Commands**

```bash
make all
```

**Tests**

- `make all`

**Acceptance**

- Reports contain no unsupported 'ISO compliant' claim and all hashes identify the tested baseline.

**Supporting references**

- `SRCREF-62B2049AEA` — `docs/standards/PLUMB-VALIDATION-RULEBOOK-2026.1.md §6`
- `SRCREF-51C477D0D0` — `docs/architecture/PLUMB-METAMODEL-v3.md §2`


### `POST.0` — Post-pilot boundary marker; do not execute under NOW scope

**Phase:** `PostPilot`  
**Scope:** `LATER`  
**Dependencies:** `E0.5`  
**Commit:** `POST.0: Post-pilot boundary marker; do not execute under NOW scope`

**Write allowlist**

- `docs/plan/POST-PILOT-SCOPE.md`

**Required actions**

1. Create a document that points to compiler stages S5-S12 and gates Q1-C1 as post-pilot work.
2. Do not implement architecture generation, contract compilation, delivery slicing, code binding or C1 in pilot-v3-foundation scope.

**Commands**

_None. This task is outside the default NOW execution scope._

**Tests**

- `No code tests.`

**Acceptance**

- Document exists and contains only scope references, no implementation code.

**Supporting references**

- `SRCREF-316C9E60F0` — `docs/architecture/PLUMB-COMPILER-ARCHITECTURE-v3.md §§11-18,32`
- `SRCREF-2992F375D9` — `docs/plan/PLUMB-v2-to-v3-IMPLEMENTATION-DELTA.md §5`

**Task-specific prohibitions**

- Do not create post-pilot crates or production code.


---

## 13. Pilot definition of done

The default execution scope is complete only when all `NOW` tasks are `done` and all of the following are true:

1. The HR pilot runs entirely on PSG-backed accepted state.
2. `functional.yaml` is generated from PSG and never acts as canonical storage.
3. Every accepted semantic mutation is represented by a serialized `SemanticPatch`.
4. Every LLM-origin semantic proposal retains a persisted `InferenceArtifact`.
5. Replaying with stored inference artifacts produces the same final `semantic_hash` without a live LLM call.
6. A stale asynchronous stage result cannot commit.
7. I0/F1/F2/F3/F4 are evaluated through the validation profile and supplied rule pack.
8. Exactly all 54 rules in those five gates have evaluator implementations.
9. Readiness does not affect gate pass/fail.
10. Business and security roles remain distinct types.
11. Visual layout changes do not change `semantic_hash`.
12. HR half-day, holiday and inclusive-end ambiguities are `Undecidable` before explicit answers and resolved only through question/decision patches.
13. The conflicting chat proposal is rejected through the same intake engine used by other mutation channels.
14. Backend e2e, Playwright e2e, coverage, mutation tests, accessibility checks and placeholder scan pass.
15. Build and standards-alignment reports identify the exact tested commit/hashes and make no unsupported compliance claim.

---

## 14. Failure and blocker protocol

When blocked:

1. Stop modifying production code.
2. Revert partial production changes made only as guesses.
3. Create `docs/blockers/<TASK_ID>.md` from the blocker template.
4. Cite exact local supporting-doc paths/sections.
5. State the single missing decision/input.
6. Mark the task `blocked` in `EXECUTION-STATE.yaml`.
7. Do not start any other task.
8. Do not commit an implementation pretending the task is complete.

Environment failures that are demonstrably transient may be retried. Semantic/specification blockers may not be worked around.

### Human-approved unblock procedure

A task in blocked state may return to pending only after the blocking condition has been corrected or an authoritative human-approved specification hotfix has been applied.

For an environment blocker:

- retain `docs/blockers/<TASK_ID>.md` as historical evidence;
- do not delete the blocker;
- verify the required environment now satisfies the task contract;
- change only that task from `blocked` to `pending`;
- rerun both repository preflight commands;
- execute the task again from the beginning;
- if successful, mark it `done` and commit using the exact task commit message;
- if it blocks again, update the existing blocker record and set it back to `blocked`.

The unblock state change itself does not need a separate commit.

---

## 15. Commit and branch rules

- Default branch: `main`.
- One completed task = one commit.
- Exact commit message comes from task manifest.
- Do not squash multiple tasks into one commit during execution.
- Do not amend a previous completed task unless the human explicitly requests it.
- A task may not depend on uncommitted changes from another task.
- Generated `api/modeller.openapi.json`, golden projections and lockfiles are committed when their task requires them.
- Supporting specification documents are read-only during implementation.

---

## 16. Definition of "no hallucination" for this build

For this plan, an AI implementation hallucination means any production behavior or data model choice that cannot be traced to:

1. an explicit task statement in this plan/manifest;
2. an explicit semantic definition in the metamodel;
3. an explicit compiler rule in compiler architecture;
4. an explicit validation rule in the supplied profile/rulebook;
5. an explicitly inherited v2 behavior named by this plan; or
6. a human decision recorded after a blocker.

If a proposed code change cannot cite one of those six authorities, it is out of scope.

---

## 17. Post-pilot boundary

The presence of architecture, contract, delivery and verification node types in `NodePayload` is intentional foundation work. It does not authorize implementation of compiler stages S5-S12 under the default scope.

A human must edit `EXECUTION-SCOPE.yaml`, update the implementation plan/task manifest for the newly authorized stages, and regenerate `SUPPORTING-DOCS.sha256` before Claude Code may implement post-pilot behavior.
