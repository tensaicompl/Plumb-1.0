# Plumb Functional Modeller — Merged Implementation Plan v2.0 (backend + API + UI)

**Target executor:** Claude Code CLI, two sessions (backend, UI) sharing one repository. This document is self-contained: it merges the backend plan v1.0, the v0.3 amendments, the UI plan v1.0 and the pilot scope addendum. Where it differs from those documents, this one wins.

**How to read the scope tags.** Every task carries one tag: **[P]** build now for the pilot; **[P-R: …]** build now with the stated reduction; **[D]** deferred until after the pilots; do not start [D] tasks. The full v1.0 scope is [P] + [P-R] fully expanded + [D].

**Executor rules (apply to both sessions):**
1. Read this entire document before writing code. Do not ask the human for decisions already fixed in §1. If something is missing, write an ADR in `docs/adr/NNNN-title.md` with your chosen default and continue.
2. Tests first where feasible. One commit per task: `M<phase>.<task>: <summary>` (backend) or `U<phase>.<task>: <summary>` (UI). Never commit with failing tests or lint.
3. No dependency outside §3 without an ADR. No network in tests. Time only through the injected `Clock`. Randomness only through a seeded RNG. LLM only through `LlmProvider`; tests use `MockProvider`.
4. Determinism: same inputs → byte-identical outputs. Use BTree collections or explicit sorting; canonical JSON for hashing.
5. Snapshot updates only via `cargo insta review` / Playwright `--update-snapshots`, with a commit message stating why the output changed.
6. The OpenAPI file `api/modeller.openapi.json` is generated from the Rust code and committed. The UI session consumes it. Backend must publish it with examples for every endpoint in §5 by the end of week 2, stubbing handlers that are not implemented yet (they return the example). The UI never invents endpoints; if it needs one, it files `docs/api-requests/NNNN.md` and mocks it.
7. Put this file and `docs/formats.md` in the repo root `CLAUDE.md` references (see §13).

---

## 1. Fixed decisions

| Area | Decision |
|---|---|
| Languages | Backend: Rust stable, edition 2021, one Cargo workspace. UI: TypeScript strict, React 19, Vite. |
| Shared core | `plumb-core` crate: `Graph`, `Node`, `Edge`, `Origin`, `GraphStore` (SQLite), blob store, `LedgerSink`, IDs, canonical JSON + SHA-256 hash, `Clock`, `LlmProvider`/`EmbeddingProvider` traits with `Null`/`Mock` implementations. If a designer prototype exists in the workspace, it depends on `plumb-core` too; extraction must not change its behaviour. |
| Storage | SQLite via `rusqlite` (bundled) behind `GraphStore`. Sources as content-addressed blobs. PostgreSQL **[D]**. |
| Import formats | Markdown, plain text, DOCX (own minimal parser over the zip/XML: paragraphs, headings, numbered/bulleted lists, tables). CSV, ReqIF, PDF, Confluence/Jira connectors **[D]**. |
| Expression language | Own DSL "PlumbExpr": `pest` grammar, typed AST, unit-aware type checker, deterministic evaluator (`rust_decimal`, `time`). No embedded scripting engine. |
| Interpreter | Pure `run(model, scenario) -> ScenarioRun`; no I/O. |
| Gap/inconsistency rules | One registry, each rule a data-shaped entry `{id, family, severity, query_fn, question_template, fixtures[]}`. No pack engine **[D]**; the registry shape makes later extraction mechanical. |
| Lint | Deterministic rule pack; per-rule precision measured on a labelled corpus; rules under 0.85 precision are `warn`. |
| Retrieval | `EmbeddingProvider` with a lexical BM25 default (own implementation) and API embeddings under the `llm` cargo feature; vectors stored in the graph store; brute-force cosine. |
| LLM | Anthropic provider under feature `llm`; default `NullProvider`; every stage and test passes with the feature off using `MockProvider`. |
| HTTP API | `axum` + `utoipa` (OpenAPI 3.1 from code) + `tower-http`; WebSocket `/api/events`; `If-Match` versioning on all writes; errors `{code, message, details}`. |
| MCP | `rmcp`, stdio **[D]**. |
| CLI | `clap`; exit codes 0 / 2 (gate failed) / 1 (error); `--json` on every command. |
| UI stack | TanStack Router + Query, Zustand (view state only), Radix primitives, Tailwind, ELK.js for layout, SVG rendering. Generated client via `openapi-typescript` + `openapi-fetch`; drift check in CI. Prism mock server for dev; MSW for component tests; Playwright against the seeded real binary for e2e. |
| Auth | Local mode: implicit user from `GET /api/me`. OIDC (`oidc-client-ts`) **[D]**. Roles: `business`, `analyst`, `architect`, `auditor`; server authoritative. |
| i18n | `en` and `pl` JSON catalogues; lint forbids hard-coded JSX strings; `pl` may lag during the pilot. |
| Embedding | `ui/dist` embedded in the API binary, served at `/`; API at `/api`. |
| Quality bar (pilot) | Rust: fmt, clippy `-D warnings`, tests; coverage ≥ 85% on `plumb-core`, `plumb-expr`, `plumb-model-sim`, gap-rule registry, intake gate; ≥ 70% elsewhere; `cargo mutants` ≥ 80% caught on gap rules, evaluator, interpreter, intake. UI: tsc, ESLint (strict + jsx-a11y), Vitest ≥ 80% on `src/features` and `src/components`, Playwright e2e green on `hr-leave`, axe zero serious violations. |
| IDs | `src:<hash8>`, `span:<src>:<start>-<end>`, `req:<PREFIX>-<NNN>`, `term:<Name>`, `ent:<Name>`, `attr:<Entity>.<name>`, `rel:<A>-<B>`, `op:<Name>`, `out:<op>.<name>`, `proc:<Name>`, `step:<proc>.<n>`, `evt:<Name>`, `actor:<Name>`, `role:<Name>`, `calc:<Name>`, `rule:<Name>`, `cal:<Name>`, `scn:<req>-<n>`, `fnd:<code>:<hash8>`, `q:<fnd>`, `dec:<ulid>`, `asm:<fnd>`, `ses:<ulid>`, `prop:<ulid>`. IDs are immutable; renames add aliases. |
| Error codes | `E_*` errors, `G_*` gaps, `I_*` inconsistencies, `W_*` warnings; stable strings; listed in `docs/codes.md` generated from the registries. |

---

## 2. Repository layout

```
plumb/
  Cargo.toml  rust-toolchain.toml  Makefile  CLAUDE.md
  crates/
    plumb-core/            graph, store, blobs, ledger, ids, hash, clock, llm/embedding traits
    plumb-model-core/      model types, loaders, consolidation, gap registry, questions, patches, gates, exports, readiness
    plumb-model-lint/      sentence lint rules + corpus benchmark
    plumb-expr/            PlumbExpr grammar, types, evaluator, example renderer
    plumb-model-sim/       scenario interpreter
    plumb-model-import/    md/txt/docx → Source + Span
    plumb-model-intake/    retrieval index + proposal intake gate
    plumb-model-session/   chat sessions (turn pipeline)
    plumb-model-testgen/   test specification derivation [D]
    plumb-model-api/       axum HTTP + WS + OpenAPI + static UI
    plumb-model-cli/       binary `plumb-model`
    plumb-model-mcp/       binary `plumb-model-mcp` [D]
  api/modeller.openapi.json
  schemas/                 JSON Schemas for every file format
  fixtures/
    hr-leave/              requirements.docx + .md, reference-model.yaml, scenarios.yaml, expert-answers.yaml, stakeholders.yaml
    expense-claims/        [D]
    lint-corpus/           200 labelled sentences (2 domains) [P-R], 500 (3 domains) [D]
    synthetic-500/         [D]
  ui/
    package.json vite.config.ts tsconfig.json playwright.config.ts
    src/app  src/api  src/components  src/features/<area>  src/i18n  src/test
    e2e/  docs/adr/
  docs/formats.md  docs/codes.md  docs/api.md (generated)  docs/adr/  docs/api-requests/
```

---

## 3. Dependencies

**Rust:** `serde`, `serde_json`, `serde_yaml`, `jsonschema`, `rusqlite` (bundled), `petgraph`, `rand`, `rand_chacha`, `pest`, `pest_derive`, `rust_decimal`, `time`, `unicode-normalization`, `regex`, `zip`, `quick-xml`, `thiserror`, `tracing`, `tracing-subscriber`, `axum`, `tokio`, `tower-http`, `utoipa`, `utoipa-swagger-ui` (dev), `clap`, `sha2`, `hex`, `ulid`, `reqwest` (feature `llm` only), `rmcp` [D].
Dev: `insta`, `proptest`, `criterion`, `assert_cmd`, `predicates`, `tempfile`, `cargo-llvm-cov`, `cargo-mutants`.

**UI runtime:** `react`, `react-dom`, `@tanstack/react-router`, `@tanstack/react-query`, `zustand`, `@radix-ui/react-*` (dialog, dropdown-menu, tabs, tooltip, popover, select, checkbox, radio-group, toast), `tailwindcss`, `elkjs`, `openapi-fetch`, `date-fns`, `zod`.
**UI dev:** `typescript`, `vite`, `vitest`, `@testing-library/react`, `@testing-library/user-event`, `msw`, `openapi-typescript`, `@stoplight/prism-cli`, `playwright`, `@axe-core/playwright`, `eslint`, `eslint-plugin-jsx-a11y`, `prettier`.

---

## 4. File formats (authoritative summaries; JSON Schemas in `schemas/`, prose in `docs/formats.md`)

### 4.1 `functional.yaml` (output; consumed by the designer)
```yaml
version: 2
model_hash: "fm:…"
glossary: [{id, name, definition, aliases[]}]
actors: [{id, kind: human|system|external}]
roles: [{id, actor, operations[]}]
entities:
  - id: ent:LeaveRequest
    attributes: [{id, type, class: pii|financial|confidential|none, unit?, precision?, enum?}]
    relationships: [{id, to, card_from, card_to, snapshot: bool}]
    states: [{id}] ; transitions: [{from, to, operation, actor, precondition?}]
    invariants: [{id, expr}]
    nfr: {consistency, availability, volatility, residency, retention}
calendars: [{id, region, tz, holidays_ref, week_pattern}]
calculations: [{id, target: attr, expr, unit, rounding: {mode, step}, inputs[], examples[]}]
rules: [{id, conditions: [{attr|state, type}], rows: [{when: {...}, then: outcome|action}], default?}]
operations:
  - id: op:ApproveLeaveRequest
    kind: command|query ; actor ; reads[attr] ; writes[attr]
    pre[expr] ; post[expr] ; outcomes: [{id, kind: success|failure}] ; governed_by[rule] ; nfr: {latency_ms?, audit?, idempotent?}
processes: [{id, trigger: {kind, ref}, steps: [{id, actor, operation, emits[], next: [{to, when?}]}], outcomes[]}]
events: [{id, payload[attr]}]
requirements: [{id, text, normalised?, class, criteria[], operations[], status}]
scenarios: [{id, requirement, operation, given: [{entity, values}], when: {operation, actor, inputs}, then: {states, values, outcome, events}, status, run: {result, trace_ref}}]
assumptions: [{id, finding, default, owner, expires}]
```

### 4.2 Imported document → `Source` + `Span`
`Source{id, kind: docx|md|txt|transcript, hash, name}`; `Span{id, source, start, end, kind: heading|paragraph|list-item|table-row|non-requirement|turn, text, speaker?, ts?}`. Byte offsets refer to the extracted text, which is stored alongside the original.

### 4.3 Standards profile (input, pilot: one fixed file)
`profile.yaml`: `{lint_thresholds, required_nfr_categories[], default_data_classes: {pattern: class}, default_retention: {class: days}, question_round_size: 15, novelty_min: 0.35, duplicate_jaccard: 0.8, similarity_threshold: 0.82}`.

### 4.4 `decisions.jsonl`, `findings.json`, `questions.json`, `scenario_runs.json`, `test_specs/*.json` [D], `readiness.json`
Each a JSON Schema in `schemas/`.

---

## 5. API contract (authoritative once generated; this table is the specification the generator must satisfy)

Conventions: JSON; `ETag` on reads, `If-Match` on writes (412 on mismatch, body includes current version); every write returns `{version, events: [DesignEvent]}`; pagination `?cursor=&limit=`; errors `{code, message, details}`; all list endpoints accept `?q=` where noted.

| Area | Method and path | Request → Response (essentials) | Scope |
|---|---|---|---|
| Meta | `GET /api/me` | → `{user, roles[], mode}` | P |
| | `GET /api/model/version` | → `{version, hash}` | P |
| | `GET /api/gates` | → `[{gate: F1..F4, passed, findings[]}]` | P |
| | `GET /api/search?q&scope` | → `[{id, kind, path, score, snippet}]` | P |
| | `WS /api/events` | server → `{kind, ids[], version}` | P |
| Sources | `POST /api/sources` (multipart) | → `{id, hash, spans_count}` | P |
| | `GET /api/sources/{id}`, `GET /api/sources/{id}/spans` | → `Source`, `[Span]` | P |
| Stages | `POST /api/stages/{intake\|lint\|extract\|consolidate\|calc\|rules\|analyse\|flows\|scenarios\|tags\|run}/run` body `{scope?}` | → `{job_id}` | P |
| | `GET /api/jobs/{id}` | → `{status, progress, result_version?, error?}` | P |
| Requirements | `GET /api/requirements?status&class&q` | → `[{id, text, class, status, smells, coverage: {operations, scenarios}, readiness}]` | P |
| | `GET /api/requirements/{id}` | → `{…, spans[], lint_findings[], ears_suggestion?, linked: {operations[], processes[], scenarios[]}}` | P |
| | `POST /api/requirements/{id}/classify` `{class}` · `/accept-ears` `{text?}` · `/split` `{parts[]}` · `/waive-finding` `{finding_id, reason}` · `/mark-duplicate` `{of}` | → write result | P |
| Glossary | `GET /api/glossary`; `POST /api/glossary/{id}/confirm\|edit\|merge` | → write result | P |
| Sentences | `GET /api/model/sentences?entity&status` | → `[{id, text, slots: {…typed…}, status, evidence[], unknowns: [{slot, question_id}]}]` | P |
| | `POST /api/model/sentences/{id}/accept\|edit\|reject` (`edit` body = slots) | → write result (goes through intake) | P |
| Entities | `GET /api/model/entities`, `GET /api/model/entities/{id}` | → entity with attributes, relationships, states, transitions, tags | P |
| | `GET /api/model/crud` | → `{operations[], entities[], cells: [{op, ent, c,r,u,d}]}` | P |
| | `POST /api/model/attributes/{id}/type\|unit\|class` | → write result | P |
| Calculations | `GET /api/model/calculations`; `GET /api/model/calculations/{id}/example?inputs=…` | → `{expr, rendered_example, value}` | P-R (example read-only) |
| | `POST /api/model/calculations/{id}/confirm\|edit` | | P-R (confirm only) |
| Rules | `GET /api/model/rules`, `GET /api/model/rules/{id}` | → `{conditions[], rows[], uncovered_cells[], overlaps[]}` | P |
| | `POST /api/model/rules/{id}/cells` `{cell, outcome}` | → write result | P |
| Processes | `GET /api/processes`, `GET /api/processes/{id}` | → `{lanes[], steps[], branches[], validation[]}` | P |
| | `POST /api/processes/{id}/steps` `{op: add\|move\|remove\|branch, …}` | → write result | P-R (add/move/remove) |
| | `POST /api/processes/import` `{format: mermaid, text}` · `GET /api/processes/{id}/export?format=mermaid\|svg` | | P-R (mermaid only) |
| Scenarios | `GET /api/scenarios?requirement&status&derived` | → `[{id, given, when, then, status, derived_from?, run?}]` | P |
| | `POST /api/scenarios/{id}/confirm` · `/correct` `{then}` · `POST /api/scenarios/confirm-derived` `{table_id}` · `POST /api/scenarios/run` `{scope}` · `GET /api/scenarios/{id}/run` | → run `{result: pass\|fail\|undecidable, diff?, missing?, trace[]}` | P |
| Findings | `GET /api/findings?status&severity&code&family` ; `POST /api/findings/{id}/waive` `{reason}` | | P |
| Assumptions | `GET /api/assumptions`; `POST /api/assumptions/{id}/resolve` | | P |
| Questions | `GET /api/questions?stakeholder&round&status` | → `[{id, kind, text, slots, context: {sentence, flow_fragment?, scenario?}, priority}]` | P |
| | `POST /api/questions/{id}/answer` `{value \| text}` | → `{proposal_id, intake, patch_preview}` | P |
| | `POST /api/rounds` `{stakeholder, max: 15}`; `GET /api/rounds/{id}` | | P |
| Proposals | `POST /api/proposals/intake` (dry run) `{kind, payload, evidence[]}` | → `IntakeReport{covered_by[], conflicts_with[], refines[], new_terms[], novelty}` | P |
| | `POST /api/proposals/{id}/link\|merge\|supersede\|create\|reject` `{reason?}` | → write result; reason required when `intake` demands | P |
| Sessions | `POST /api/sessions` `{scope}`; `POST /api/sessions/{id}/turns` `{text}` | → `{reply, proposals: [{id, kind, payload, intake}], asked_question?}` | P |
| | `GET /api/sessions/{id}`, `GET /api/sessions/{id}/metrics` | | P |
| Decisions | `GET /api/decisions?by&kind&since` | | P |
| Readiness | `GET /api/requirements/{id}/readiness`; `GET /api/readiness?group=feature\|project` | → `{overall, sub: {clarity, completeness, consistency, verifiability, confirmation}, contributing[]}` | P-R (sub-scores; composite uncalibrated) |
| Export | `GET /api/model/export?format=functional\|report\|decisions\|glossary` | → file | P |
| | `GET /api/model/diff?from&to` | → structured diff | P |
| Tests | `POST /api/tests/generate`; `GET /api/tests?kind&requirement`; `GET /api/tests/matrix`; `GET /api/tests/export?format` | | D |

Every endpoint must have at least one request and one response example in the OpenAPI file, taken from `fixtures/hr-leave`.

---

## 6. Core types (Rust; extend only by adding)

```rust
// plumb-core
pub type Id = String;
pub enum Origin { Deterministic, Llm, Human, Recovered }
pub enum Status { Proposed, Confirmed, Rejected, Superseded, Suspect }
pub struct Node { id: Id, kind: String, props: BTreeMap<String, Value>, origin: Origin, status: Status, confidence: f32, version: u32 }
pub struct Edge { id: Id, kind: String, from: Id, to: Id, props: BTreeMap<String, Value>, origin: Origin, status: Status, version: u32 }
pub struct Graph { /* BTreeMap nodes/edges + indexes by kind, out, in */ }
impl Graph { fn canonical_json(&self)->String; fn hash(&self)->String; }
pub trait GraphStore { fn load(&self)->Result<Graph>; fn save(&self,&Graph)->Result<u32>; fn snapshot(&self,label:&str)->Result<String>; fn version(&self)->Result<u32>; }
pub trait BlobStore { fn put(&self, bytes:&[u8]) -> Result<String>; fn get(&self, hash:&str)->Result<Vec<u8>>; }
pub trait LedgerSink { fn emit(&self, e: LedgerEvent) -> Result<()>; }
pub struct LedgerEvent { ts: Timestamp, actor: Actor, kind: String, ids: Vec<Id>, payload: Value }
pub trait Clock { fn now(&self)->Timestamp; }
pub trait LlmProvider { fn complete_json(&self, schema:&Value, prompt:&Prompt)->Result<Value>; }
pub trait EmbeddingProvider { fn embed(&self, texts:&[String])->Result<Vec<Vec<f32>>>; }

// plumb-model-core
pub struct RuleEntry { id:&'static str, family: Family, severity: Severity,
  query: fn(&Graph)->Vec<Finding>, question: Option<QuestionTemplate>, fixtures: &'static [&'static str] }
pub static RULES: &[RuleEntry];   // the registry
pub struct Finding { id: Id, code: String, severity: Severity, nodes: Vec<Id>, edges: Vec<Id>, message: String, status: FindingStatus }
pub enum QuestionKind { YesNo, PickOne, Number{unit:Option<String>}, Cardinality, Unit, Rounding, FormulaConfirm, RuleCell, Calendar, Free }
pub struct Question { id: Id, finding: Id, kind: QuestionKind, text: String, slots: BTreeMap<String,Value>, stakeholder: Option<Id>, priority: f64, round: Option<Id>, status: QStatus }
pub trait Patch { fn apply(&self, g:&Graph)->Result<GraphDelta>; fn invert(&self)->Box<dyn Patch>; fn describe(&self)->String; }
pub struct Decision { id: Id, question: Id, answer: Value, by: String, at: Timestamp, patch: Box<dyn Patch> }
pub struct IntakeReport { covered_by: Vec<(Id,f32)>, conflicts_with: Vec<(Id,String)>, refines: Vec<Id>, new_terms: Vec<String>, novelty: f32, requires_reason: bool }
pub struct Readiness { overall: u8, clarity: u8, completeness: u8, consistency: u8, verifiability: u8, confirmation: u8, contributing: Vec<Id> }

// plumb-expr
pub enum Ty { Int, Decimal(u8), Bool, Date, DateTime, Duration(Unit), Quantity(Unit), Enum(Id), Ref(Id), List(Box<Ty>) }
pub fn parse(&str)->Result<Ast>; pub fn typecheck(&Ast,&TypeEnv)->Result<Ty>; pub fn eval(&Ast,&ValueEnv,&dyn CalendarProvider)->Result<Value>;
pub fn render_example(&Ast,&ValueEnv,&Value)->String;

// plumb-model-sim
pub fn run(model:&Model, scenario:&Scenario)->ScenarioRun;  // Pass | Fail{diff} | Undecidable{missing: Vec<Missing>} + trace
```

Stage functions are pure: `fn stage(&Graph, &Config) -> Result<StageOutput>`; the API/CLI applies outputs, bumps versions, emits ledger events, and notifies the WebSocket.

---

## 7. Backend phases and tasks

### M0 — Core, formats, fixtures
- **M0.1 [P]** Workspace, `plumb-core` (types in §6, SQLite store, blob store, JSONL ledger sink, ids, hash, `Clock`, provider traits with `Null`/`Mock`), Makefile targets `check test cov mutants e2e openapi`, CI. Tests: graph round-trip; hash stable across insertion order (proptest); store save→load identity; snapshot restore. Acceptance: CI green on an empty model.
- **M0.2 [P]** JSON Schemas for §4 formats; `docs/formats.md` with doc-tests validating every example block. Acceptance: schema tests pass; invalid crafted files rejected with the right JSON pointer.
- **M0.3 [P-R]** `fixtures/hr-leave`: 32 requirements as DOCX (headings, numbered and bulleted lists, two tables) and Markdown; `reference-model.yaml` (9 entities, 13 operations, 6 processes, 5 invariants, 2 calculations, 3 rules, 6 events); `scenarios.yaml` (44, including the half-day, holiday-spanning, inclusive-end and contractor cases); `expert-answers.yaml` (typed answers for every question the fixture should generate); `stakeholders.yaml`; `profile.yaml`. `lint-corpus`: 200 sentences over two domains, labelled per lint rule by two annotators, agreement recorded. **[D]** `expense-claims`, `synthetic-500`, corpus to 500/3 domains.
- **M0.4 [P]** `docs/codes.md` generator from the rule registries; `CLAUDE.md` per §13.

### M1 — Import and intake
- **M1.1 [P-R]** `plumb-model-import`: md/txt/docx → `Source` + ordered `Span`s with kinds; tables → rows with headers. Tests: byte-offset round trip; nested lists; merged-cell table; determinism. **[D]** CSV, ReqIF, PDF.
- **M1.2 [P]** Segmentation: `Segmenter` trait; LLM implementation (span-anchored, schema-validated) and deterministic fallback (one requirement per list item or per sentence containing shall/must/should/can). Post-check: full coverage, no overlap, valid kinds, unassigned text → `non-requirement` flagged. Tests: fallback on fixture; mock LLM with a gap → `E_INTAKE_UNCOVERED`.
- **M1.3 [P]** Requirement records: IDs (project prefix + counter; same span hash → same id on re-import), classification (LLM proposal + deterministic override table), EARS suggestion (LLM, stored as proposal; original kept). Tests: override cases; id stability.
- **M1.4 [P-R]** Duplicates: Jaccard ≥ `duplicate_jaccard` on normalised statements; semantic pairs via M13.1 once available. Tests: fixture pairs.
- **M1.5 [P]** Gate F1: all spans assigned; all classified; density under threshold or waived. Tests: per-code fixtures.

### M2 — Sentence lint
- **M2.1 [P-R]** Rule pack, 15 rules: vague terms, passive without actor, unmeasurable qualifier, compound "shall", negation stacking, escape clause, open list, pronoun without antecedent, UI-phrased, undefined term (post-glossary), EARS trigger/precondition/response order, unit-less number, relative time without anchor, missing actor, ambiguous quantifier. Each: id, description, default severity, span extraction. **[D]** remaining rules to ≥ 25.
- **M2.2 [P]** Corpus benchmark under `--features corpus`: precision/recall per rule; `fixtures/lint-corpus/baseline.json`; regression test fails on > 0.03 precision drop; rules under 0.85 downgrade to `warn` via config, never silently.
- **M2.3 [P]** Findings integration; density per requirement and document. Tests: snapshot.

### M3 — Vocabulary
- **M3.1 [P]** Term candidates (LLM, span-anchored) + deterministic normalisation (NFKC, case, singularisation with exception list, determiner stripping), frequency and co-occurrence. Tests: normalisation table; determinism.
- **M3.2 [P]** Term typing heuristics (entity/attribute/actor/value/system/unknown) with evidence; conflicts → `I_TERM_TYPE`, `I_TERM_CONFLICT`. Tests: HR typing vs reference ≥ 90%.
- **M3.3 [P]** Round-0 questions for unknown and conflicting terms; glossary export. Tests: question set snapshot; routing.

### M4 — Entities, relationships, enumerations, quantities, data classes
- **M4.1 [P]** Entity/attribute extraction (LLM anchored, closed vocabulary: unknown terms rejected by schema); consolidation; enumeration detection (closed lists, tables); quantity detection (numbers with units, day/hour/amount words). Tests: entity survival vs reference ≥ 85%.
- **M4.2 [P]** Relationships with cardinality only when stated; states and transitions from lifecycle verbs; invariants from business rules (PlumbExpr proposals). Tests: HR state machines; `G_REL_NO_CARD` produced.
- **M4.3 [P]** Data-class dictionary (email, dob, salary, iban, …) + LLM proposal for the rest. Tests: dictionary hits; mock path.

### M5 — PlumbExpr
- **M5.1 [P]** Grammar (pest): literals, dotted identifiers, arithmetic, comparison, boolean, calls, `in`, lists. Tests: parse/unparse round trip (proptest); precedence table.
- **M5.2 [P]** Type checker with units: same-unit add/sub, scalar mul/div, unit mismatch error; date/duration; enum membership; `Ref` navigation. Tests: accept/reject table; proptest that well-typed expressions evaluate without type errors.
- **M5.3 [P]** Evaluator: decimals, rounding modes (half-up, half-even, floor, ceil, to-step), dates, `working_days(period, calendar, inclusive)`, `days_between`, aggregates, `as_of(version)` [D]. `CalendarProvider` trait with fixture implementation. Tests: rounding table; working days across weekends/holidays; inclusive vs exclusive; property `working_days(p) ≤ days_between(p)+1`.
- **M5.4 [P-R]** Example renderer to plain English (no i18n in pilot). Tests: snapshots.

### M6 — Calculations, rules, calendars, gap registry, operations
- **M6.1 [P]** Calculation candidates (LLM → PlumbExpr proposals); deterministic checks → `G_QTY_NO_UNIT`, `G_QTY_NO_PRECISION`, `G_CALC_MISSING`, `G_CALC_NO_ROUNDING`, `G_CALC_UNDEFINED_INPUT`, `G_CALC_UNIT_MISMATCH`, `G_CALC_CIRCULAR`, `G_TIME_NO_CALENDAR`, `G_TIME_NO_TZ`, `G_TIME_BOUNDARY`, `G_TIME_NO_UNIT`. Tests: the HR half-day/holiday/inclusive-end cases produce exactly the expected codes.
- **M6.2 [P]** Decision tables: typed condition columns; completeness (enumerate enum/bool space; interval coverage for numbers); overlap; default row. Tests: complete/incomplete/overlap fixtures; proptest random tables.
- **M6.3 [P]** Full registry (§6 `RULES`) covering entities, relationships, states, operations, actors/roles, flows, events, NFR, quantities/calculations, rules, calendars/time, inconsistencies (`I_NUMERIC_RANGE`, `I_MODALITY`, `I_STATE_FORK`, `I_ACTOR_EXCLUSIVE`, `I_TERM_CONFLICT`, `I_CARD_CONFLICT`, `I_CYCLE_PRECOND`, `I_DUP_REQ`, `I_RULE_OVERLAP`). Findings upsert keyed by (code, nodes); waivers persist. Tests: one fixture per code (referenced from the entry); proptest: removing a random creator yields `G_ENT_NO_CREATE`; `docs/codes.md` regenerated and committed.
- **M6.4 [P]** Operations: read/write sets from criteria, invariants, calculations, rules; failure outcomes; inputs/outputs. Tests: HR vs reference; `ApproveLeaveRequest.reads ⊇ {LeaveBalance.remaining_days, Employee.manager, Contract.type}`.

### M7 — Question engine, decisions, assumptions
- **M7.1 [P]** Templates per code with slot filling; typed kinds; suppression by (template, slots); routing via `stakeholders.yaml`; priority = severity × blast radius (reachability count). Tests: HR round-1 snapshot (set and order); determinism.
- **M7.2 [P]** Rounds: ≤ `question_round_size` per stakeholder, blocking first, grouped by entity. Tests: batching properties.
- **M7.3 [P]** Answer application: one `Patch` per kind (cardinality, state/transition, outcome, actor allowance, unit/precision/rounding, formula, rule cell, calendar, attribute type); diff preview; `Decision` nodes; supersession applies inverse then new. Tests: apply/invert round trip (proptest); applying `expert-answers.yaml` leaves zero blocking findings.
- **M7.4 [P-R]** Assumptions with expiry via `Clock`; `W_ASSUMPTION_EXPIRING`. **[D]** scheduled re-asks.

### M8 — Processes and scenarios
- **M8.1 [P]** Flow derivation from pre/postconditions and requirement order; branches from failure outcomes and rule rows; events on steps; validation (one trigger, reachability, outcomes, operation per step, expressible conditions). Tests: HR `LeaveApproval` with contractor branch; validation codes.
- **M8.2 [P-R]** Mermaid export and import (as proposals); SVG export via the same layout data. **[D]** BPMN.
- **M8.3 [P]** Scenario generation (LLM anchored) from criteria, failure outcomes, rule rows, rounding boundaries; values validated against enumerations, units, ranges; `derived_from` set for row/transition scenarios. Tests: mock provider; boundary generator yields the 0.5-day and holiday cases deterministically.

### M9 — Interpreter
- **M9.1 [P]** Materialisation from `Given`; validation → `Undecidable{missing}`.
- **M9.2 [P]** Operation execution: actor rights, preconditions, rule lookup, calculations, invariants, writes, transitions, events; trace.
- **M9.3 [P]** `Then` comparison with structured diff. Tests: all 44 HR scenarios: reference model ≥ 40 pass, the designated ones undecidable; pre-answer model: half-day, holiday, inclusive-end scenarios undecidable naming the right elements; proptest determinism; mutation ≥ 80%.
- **M9.4 [P]** Dirty-set execution on change; findings from fail/undecidable (`I_SCENARIO_FAIL`, `G_SCENARIO_UNDECIDABLE`).

### M10 — Tags, gates, exports
- **M10.1 [P-R]** Data class and consistency tags with propagation (PII → security group; financial write → audit). **[D]** retention/residency beyond profile defaults.
- **M10.2 [P]** Gates F2, F3, F4 as specified (F4 = designer G1 rules + every requirement has ≥ 1 executed-pass-and-confirmed scenario + zero undecidable + assumptions listed). Tests: per-rule fixtures; HR passes F4 after expert answers and confirmations.
- **M10.3 [P-R]** Exports: `functional.yaml` (hashed), `glossary.md`, `scenarios.yaml`, `decisions.jsonl`, `model-report.md`, `readiness.json`. **[D]** ReqIF, BPMN, test specs. Tests: golden files; `functional.yaml` validates against `schemas/functional.schema.json`.

### M11 — API and CLI
- **M11.1 [P]** `plumb-model-api`: every P and P-R endpoint in §5 with `utoipa` annotations; OpenAPI committed and checked in CI; examples from the fixture; `If-Match`; WebSocket events on every ledger append; role checks; static UI serving. Contract tests: each endpoint's example round-trips; 412 on wrong version; role denial. **Week-2 milestone:** the OpenAPI file with examples for all P/P-R endpoints, stubs allowed.
- **M11.2 [P]** CLI: `import lint vocab extract consolidate calc rules analyse questions rounds apply flows scenarios run tags ready export report serve`; `--json`; exit codes. Tests: `assert_cmd`; e2e script `tests/e2e_hr.rs` from import to export.
- **M11.3 [D]** MCP. **M11.4 [D]** performance benches.

### M12 — LLM provider
- **M12.1 [P]** `AnthropicProvider` under `llm`; prompt templates per stage as files with JSON schemas; exemplar injection from accepted outputs; per-project cost accounting. Tests: mock only; documented manual smoke script.

### M13 — Retrieval and intake gate
- **M13.1 [P-R]** Index over requirement statements, sentences, rule rows, calculations, scenario texts; lexical BM25 default; API embeddings under `llm`; mock provider with deterministic hashed vectors; incremental update. Tests: top-k determinism; incremental equals rebuild.
- **M13.2 [P]** `intake()` with term resolution, duplicate/near-duplicate, coverage (typed deterministic; assisted for prose, labelled), conflict via the inconsistency rules on model ∪ proposal, novelty. Tests: HR fixtures per outcome; intake of an existing requirement's text → `covered_by` self at 1.0; conflicts reuse M6.3 (no second implementation).
- **M13.3 [P]** Wire into every write path (import, sentence edits, process edits, answers, sessions). Tests: flagged proposal without reason → `E_INTAKE_REASON_REQUIRED` on every channel.

### M14 — Sessions (chat)
- **M14.1 [P]** `Session`, transcript-as-source, turn pipeline (retrieve → compose context with budget and priority → LLM → intake → display → apply on action); asked questions restricted to open engine questions by id. Tests: proposal without user-turn span rejected; invented question id rejected; budget ordering.
- **M14.2 [P]** Session metrics and analyst flags (create-to-link ratio). Tests: thresholds.
- **M14.3 [D]** MCP session tools.

### M15 — Readiness
- **M15.1 [P-R]** Five sub-scores and composite per §C of the v0.3 amendments; explanation payload. Tests: formula table; monotonicity (resolving a finding never lowers a score). Composite shown as uncalibrated.
- **M15.2 [D]** Rework logging and `calibrate`.

### M16 — Test generation **[D]**
- Derivation per the v0.3 amendments §D; Gherkin renderer; coverage matrix; salted hidden/visible split; F4 extension.

---

## 8. UI phases and tasks

### U0 — Scaffold
- **U0.1 [P]** Vite/TS/Tailwind/Router/Query; ESLint strict + jsx-a11y; Prettier; CI: lint, type-check, unit, e2e `@mock`, axe. Acceptance: empty app passes CI.
- **U0.2 [P]** Generated client from `api/modeller.openapi.json`; drift check; Prism dev script; MSW handlers generated from the examples. Acceptance: typed `GET /api/model/version` works against Prism and MSW.
- **U0.3 [P]** i18n plumbing; hard-coded-string lint; `en`/`pl` catalogues.

### U1 — Shell and shared components
- **U1.1 [P-R]** Shell: navigation (Home, Requirements, Model {Glossary, Sentences, Entities, Processes, Scenarios, Findings & Assumptions, Rules, Calculations}, Questions, Decisions, Gates), project selector, global search. **[D]** "what changed since" selector.
- **U1.2 [P-R]** Auth: local implicit user; role gating helper. **[D]** OIDC.
- **U1.3 [P]** Shared components: `ProposalReviewBar` (status; intake results; accept/edit/reject/link/merge/supersede/create-with-reason/explain; role gating; reason enforcement), `ProvenanceDrawer`, `FindingsPanel` (pinnable, filters, deep links), `DiffViewer`, `DiagramFrame` (ELK, zoom/pan/fit, export SVG, list-equivalent toggle), `TypedAnswerControl` (one renderer per `QuestionKind`), `ScoreChip` (sub-scores on hover), `ConflictDialog`, hooks `useVersionedMutation`, `useEvents`. Tests: one per behaviour listed.
- **U1.4 [P]** Home: gate cards, my questions, proposals awaiting me, blocking findings, recent decisions, empty-state onboarding. Tests: fixture render; deep links.

### U2 — Requirements
- **U2.1 [P]** Table with filters, pagination, readiness chip. **U2.2 [P]** Detail with lint highlights, EARS side-by-side, links, provenance; actions reclassify/waive/split (diff preview)/mark-duplicate. Tests: each action sends `If-Match`.

### U3 — Glossary **[P]** list, conflicts with pick/merge.

### U4 — Sentences **[P]** grouped by entity; review bar per sentence; structured edit form; inline unknowns via `TypedAnswerControl`; bulk accept per section with remaining counter; accept-without-view warning (logged). Tests: inline answer → decision; bulk accept per id with version; warning fires.

### U5 — Questions **[P]** stakeholder inbox with context block and typed controls; answer → proposal card → confirm; analyst round manager (compose ≤ 15, assign, rates, apply with `DiffViewer`). Tests: composition constraints; diff before commit.

### U6 — Chat sessions **[P]** side panel on scoped views + project room; stream; scope chips; proposal cards with intake results and full action set; engine question rendered with typed control; metrics header; transcript links from provenance. Tests: actions call intake endpoints; conflicting card cannot be created without reason; metrics update.

### U7 — Processes
- **U7.1 [P]** Swimlane read view: lanes, steps, branches, events, trigger, outcomes; ELK layout; validation badges. 
- **U7.2 [P-R]** Edit: add step (existing op or propose new → proposed op + question), reorder, move lane; Mermaid import/export; SVG export. **[D]** branch editing, BPMN. Tests: commands; layout determinism; import round trip.

### U8 — Scenarios **[P]** cards with run badges and trace in business terms; correction form → proposal; filters; bulk confirm of derived scenarios per table with three-sample prompt; run-scope button. Tests: correction → model-change proposal; bulk confirm limited to derived; undecidable trace names the missing element.

### U9 — Analyst views
- **U9.1 [P]** Findings & assumptions. **U9.2 [P-R]** Entities: ER read-only + CRUD matrix; **[D]** state machines, tag editor. **U9.3 [P]** Rules: tables with uncovered cells; cell fill. **U9.4 [P-R]** Calculations: worked example display; **[D]** editable inputs.

### U10 — Gates, decisions, export
- **U10.1 [P-R]** Gates page. **[D]** readiness distribution and trend. **U10.2 [P-R]** Decisions log; **[D]** in-UI exports (CLI in pilot). **U10.3 [D]** Tests view.

### U11 — Hardening and e2e
- **U11.1 [P]** Playwright against the seeded real backend: import → F1 → round 0 → sentences → round 1 with a chat session in which a conflicting proposal is rejected → swimlane edit → scenarios run and confirm → F4 → export. Assert on visible state only.
- **U11.2 [P-R]** Visual regression: swimlane and home. **U11.3 [P]** axe on every route; keyboard-only e2e run. **U11.4 [D]** performance.

---

## 9. Cross-session dependencies and weekly sequence

| Week | Backend | UI | Integration checkpoint |
|---|---|---|---|
| 1 | M0, M1.1–M1.3 | U0, U1.1–U1.3 (Prism) | repo builds; CI green both |
| 2 | M1.4–M1.5, M2, **M11.1 OpenAPI with examples (stubs allowed)** | U1.4, U2 | UI regenerates client from the real file; drift check green |
| 3 | M3, M4 | U3, U4 | sentences endpoint real |
| 4 | M5, M6.1–M6.2 | U5, U9.3 | questions and rules endpoints real |
| 5 | M6.3–M6.4, M7 | U9.1, U6 (sessions via Prism examples) | findings, rounds, decisions real |
| 6 | M9, M8.1, M8.3 | U8, U7.1 | scenario run results real |
| 7 | M10, M11.2, M12.1 | U7.2, U9.2, U9.4, U10 | F4 reachable on the fixture via CLI |
| 8 | M13, M14, M15.1; `tests/e2e_hr.rs` green | U11.1 e2e on real binary, U11.3 | **pilot readiness: `make e2e` green in both** |
| 9–10 | Pilot support; fixes only | Pilot support; fixes only | |

If the week-2 OpenAPI milestone slips, the UI continues against Prism and the slip becomes integration risk; report it in `docs/status.md` weekly (both sessions append a dated entry).

---

## 10. Testing strategy (both sessions)

| Level | Tooling | Proves |
|---|---|---|
| Unit | `cargo test` + `insta`; Vitest + Testing Library + MSW | each stage and component behaves per contract |
| Property | `proptest` | determinism; patch apply/invert; typed expressions evaluate; rule tables; findings under random deletions; corridor-style invariants |
| Corpus | `lint-corpus` | per-rule lint precision; regression guard |
| Reference | `hr-leave` reference model and expert answers | extraction survival, typing, operations, zero blocking findings after answers |
| Interpreter | 44 HR scenarios | undecidable before answers, pass after; trace correctness |
| Contract | `utoipa` file + generated TS types + drift check + example round-trips | API and UI cannot diverge silently |
| Integration | `assert_cmd` e2e; Playwright on seeded binary | the full business loop |
| Visual | Playwright screenshots | deterministic diagrams stay identical |
| Accessibility | axe + keyboard e2e | WCAG AA |
| Mutation | `cargo mutants` | gap rules, evaluator, interpreter, intake are constrained by tests |

---

## 11. Definition of done

**Pilot (end of week 8):**
- All [P] and [P-R] tasks complete; quality bar green in both sessions; `make e2e` passes on `hr-leave` with `--features ""` (mock) and `--features llm` (real provider, smoke script).
- `api/modeller.openapi.json` committed, matched by contract tests, consumed by the UI without local edits.
- The three HR ambiguities (half day, holiday within period, inclusive end date) are caught by registry rules and resolved through questions in the e2e run; the fixture's expert answers are applied only through the question endpoints, never as a shortcut.
- A chat session in the e2e run rejects a conflicting proposal through the intake gate.
- `README.md`: run backend, run UI against Prism, run against the seeded binary, run e2e, run a pilot (import → rounds → F4 → export).
- `docs/status.md` has weekly entries from both sessions.

**v1.0 (after pilots):** all [D] tasks; corpus to 500/3 domains; ReqIF/BPMN/CSV; MCP; OIDC; readiness calibration; test generation; performance targets (intake of 500 requirements < 60 s with fallback segmenter; analysis recompute < 2 s; 200 scenario runs < 1 s; table interaction < 100 ms; swimlane layout < 2 s for 60 steps).

---

## 12. Pilot metrics the software must record (backend logs to `metrics/*.jsonl`; UI events via the API)

Extraction survival per element kind; lint precision on sampled findings; findings confirmed as real by the business, by rule family; questions marked needed; questions per requirement; scenarios undecidable on first run and how many became decisions; intake outcomes (duplicates/conflicts caught, create-anyway overrides, analyst disagreement); session metrics; stakeholder hours and analyst days (entered manually via CLI `plumb-model log-effort`); proposals accepted without being viewed.

Stop rules for the humans running the pilot, restated here so the software surfaces them: calculations/time family > 50% noise on the real spec → stop and rework rules; questions marked needed < 40% → stop and rework batching/templates.

---

## 13. `CLAUDE.md` (place at repo root; both sessions read it)

```
# Plumb Functional Modeller — working rules
- The plan is docs/plan/merged-implementation-plan-v2.md. Read it fully. Scope tags: [P] build, [P-R] build reduced, [D] do not start.
- Fixed decisions are in plan §1; do not revisit. Missing decision → ADR in docs/adr, choose a default, continue.
- Backend session: Rust workspace; commit prefix M<phase>.<task>. UI session: ui/; commit prefix U<phase>.<task>.
- api/modeller.openapi.json is the contract. Backend generates it (utoipa) and commits it; UI generates types from it; never hand-edit either. UI needs → docs/api-requests/NNNN.md + mock.
- Tests first where feasible; no failing tests or lint in commits; no new dependencies without ADR; no network in tests; Clock injected; seeded RNG; LLM via LlmProvider (MockProvider in tests).
- Determinism: BTree collections or explicit sorts; canonical JSON hashing; same input → identical output.
- Snapshot updates only via `cargo insta review` / `--update-snapshots`, with a commit message explaining the change.
- Append a dated entry to docs/status.md at the end of each working session: done, blocked, next.
- Weekly checkpoints and the week-2 OpenAPI milestone are in plan §9.
```

---

## 14. Start here

**Backend session:** M0.1 → M0.2 → M0.3 → M0.4 → M1.1 → M1.2 → M1.3 → M2.1 → M2.2 → M1.4 → M1.5 → M2.3 → M11.1 (OpenAPI milestone) → M3 → M4 → M5 → M6 → M7 → M9 → M8 → M10 → M11.2 → M12 → M13 → M14 → M15.1 → `tests/e2e_hr.rs`.

**UI session:** U0 → U1.1 → U1.2 → U1.3 → U1.4 → U2 → (regenerate client at week 2) → U3 → U4 → U5 → U9.3 → U9.1 → U6 → U8 → U7.1 → U7.2 → U9.2 → U9.4 → U10 → U11.1 → U11.3 → U11.2.
