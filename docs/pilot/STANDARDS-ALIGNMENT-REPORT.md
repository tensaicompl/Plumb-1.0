# Plumb HR pilot — standards-alignment report

Branch `plumb-goal-approach`, build `879c0c3`. This report follows the four separate assertions of
the validation rulebook §6. It makes no "ISO compliant" or certification claim: every external
standard below is a declared alignment, never verified clause-level coverage.

## Evaluated baseline

| Item | Value |
|---|---|
| Profile | `profile:plumb-software-2026.1`, profile version `2026.1-draft.1`, metamodel `3.0-draft.1` (status: implementation draft) |
| Rule pack | `config/profiles/plumb-software-2026.1-rules.yaml`; rule-pack hash `sha256:b3a48aa0d0dc47b612697bbf35fa778499509343f74de43459fbcc145e21d33e`; profile hash `sha256:e6dc73ec3f1847e5e7fc5d71aef64b71c15a3e93838e64c7f246f275c65af055` |
| Validator | `plumb-validation` 0.1.0 (workspace build `879c0c3`); inference-output schemas checked with the `jsonschema` crate 0.18.3 (JSON Schema 2020-12) |
| Baseline | `rev:60:06937019ab7406cb`, semantic hash `psg:sha256:06937019ab7406cb17872b23d10107772d89af0f9fe8e99ebf55c8e3f28f4e75` |
| Evidence | one source, `fixtures/hr-leave/requirements.md`; evidence hash as in `docs/pilot/BUILD-REPORT.md` |
| How produced | `cargo test -p plumb-cli --test sim_hr hr_pilot_simulation` (scripted model, replayed through the CLI); gate reports read from the resulting project database |

Gates in scope: I0, F1, F2, F3, F4. Gates Q1, A1–A4, D1, D2 and C1 belong to post-pilot stages
S5–S12 and are not evaluated (see `docs/plan/POST-PILOT-SCOPE.md`).

## 1. Plumb Profile Conformant

**Asserted for gates I0–F4 of the pilot baseline**, under the profile's waiver policy with one
waiver. Every applicable blocker rule is PASS or WAIVED where its waiver policy permits.

| Gate | Result | Blocker failed | Blocker waived | Rules PASS / FAIL (non-blocking) / NOT_APPLICABLE / WAIVED |
|---|---|---|---|---|
| I0 | PASS | 0 | 0 | 5 / 0 / 1 / 0 |
| F1 | PASS | 0 | 1 | 7 / 2 / 1 / 1 |
| F2 | PASS | 0 | 0 | 15 / 1 / 8 / 0 |
| F3 | PASS | 0 | 0 | 4 / 0 / 2 / 0 |
| F4 | PASS | 0 | 0 | 0 / 0 / 7 / 0 |

Non-blocking failures (severity error, recorded as findings, not promoted by the profile):

| Rule | Why it fails in the pilot |
|---|---|
| `ISO29148.F1.REQ.LINEAGE` | No Goal/Need/Concern elements are produced, so no requirement traces to an intent element. |
| `PLUMB.F1.REQ.CRITERIA_FOR_BEHAVIOR` | Behavioral requirements without a `specified_by` operation or scenario link. |
| `PLUMB.F2.EVENT.CONSUMER` | Events with no consuming operation or process and no governed external/terminal disposition. |

Limits of this assertion:
- F4 passes vacuously. The pilot configuration lists no in-scope scenarios, so all seven F4 rules
  are NOT_APPLICABLE even though 7 scenarios were derived and run.
- `PLUMB.F2.STATE.REACHABILITY` is NOT_APPLICABLE because no governed initial state is declared
  for the leave-request state machine.
- BPMN and process rules are NOT_APPLICABLE because no process is modelled.

## 2. Standards Aligned

PSG concepts used by the pilot gates map to the following standards. The mapping strengths are
those declared in the rulebook. **This is not external certification.**

| Standard | Version | Profile role | Declared mapping strength | Pilot rules | Pilot result |
|---|---|---|---|---|---|
| ISO/IEC/IEEE 29148 | 2018 | semantic_alignment | compatible | `ISO29148.F1.REQ.STATEMENT_PRESENT`, `ISO29148.F1.REQ.LINEAGE` | PASS; FAIL (error, non-blocking) |
| INCITS 359 (RBAC) | 2012 | semantic_alignment | compatible | `RBAC.F2.PERMISSION.CONCRETE`, `RBAC.F2.ROLE.HIERARCHY_ACYCLIC` | PASS; PASS |
| OMG PPMN | 1.0 | interchange | compatible | `PPMN.I0.PROVENANCE.AGENT_IDENTIFIED` | NOT_APPLICABLE |
| OMG DMN | 1.6 | interchange | subset | `DMN.F2.TABLE.NO_OVERLAP`, `DMN.F2.TABLE.COVERAGE` | PASS; PASS |
| OMG BPMN | 2.0.2 | interchange | subset | `BPMN.F2.PROCESS.START_END`, `BPMN.F2.PROCESS.REACHABLE` | NOT_APPLICABLE |

The profile also lists ISO/IEC 25010:2023, ISO/IEC/IEEE 42010:2022, 12207:2026, 15289:2019,
29119-2:2021, OMG SysML 2.0, OpenAPI 3.2.1, AsyncAPI 3.0.0, Arazzo 1.1.0 and JSON Schema 2020-12.
None of their rules belongs to a pilot gate, so no alignment is asserted for them.

The DMN and BPMN rules are applied to the PSG's own decision-table and process structures, which
are a declared subset of those standards' semantics. No DMN or BPMN document is read or written.

## 3. Interchange Valid

**Not asserted for any specification artifact.** In NOW scope the pilot neither imports nor
generates BPMN, DMN, OpenAPI, AsyncAPI or Arazzo documents for the specified system.

What is actually validated:
- **Imported sources:** Markdown, plain text and a minimal DOCX subset. Parse status and evidence
  locators are checked by I0. These are input formats, not interchange-validity claims.
- **Inference outputs:** every model output is validated against its JSON Schema 2020-12 document
  in `schemas/inference/` before use.
- **Plumb's own HTTP API:** its OpenAPI contract (`api/modeller.openapi.json`) is generated and
  drift-checked. It describes the tool, not the specified HR system, and is not part of this
  assertion.

## 4. Organization Policy Conformant

**Not evaluated.** No customer rule pack is configured; the `organization_policy` rule class has no
rules in the pilot profile.

## Deviations

- The rule adjudications and behavioural deviations of this branch are listed in
  `docs/plan/GOAL-APPROACH-DELTA.md`. Those that affect the results above:
  - **`PLUMB.F1.REQ.NO_CONTRADICTION`:** NOT_APPLICABLE without contradiction finding material.
  - **`PLUMB.F1.REQ.TYPE_KNOWN`:** pass-by-construction is declared in the result evidence.
  - **`PLUMB.F2.STATE.REACHABILITY` and `PLUMB.F2.EVENT.*`:** use governed disposition extensions.
  - **Gate analysis:** carried forward after edits.
- Lint corpus qualification: not run — external human-labelled corpus not supplied. No
  lint-precision claim is made.

## Waivers

| Finding | Rule | Waiver policy | Targets | Rationale |
|---|---|---|---|---|
| `fnd:14f0eaebd698b9c9` | `PLUMB.F1.REQ.NO_DUPLICATE_ACCEPTED` (blocker, decision_required) | governed waiver decision | HR-023, HR-024 | "HR-023 and HR-024 notify different outcomes; they are distinct requirements." (the fixture's Jaccard threshold 0.8 flags them) |

No other waiver is applied in any gate.
