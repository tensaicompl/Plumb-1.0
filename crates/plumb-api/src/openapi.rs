//! The pilot HTTP API contract (plan A0.1, Hotfix 062): compatibility DTOs, contract-only
//! utoipa operation stubs for the 74 NOW operations (the 72 of Hotfix 062 plus the two U2.1
//! semantic change previews of Hotfix 073), deterministic compatibility patches,
//! HR-fixture-seeded and schema-fallback examples, and the rendered OpenAPI document and API
//! reference.
//!
//! This module defines the contract only. It has no router, handler body, application state,
//! store, commit, intake or session execution, and reads no file, clock, network or environment:
//! the HR fixture is embedded at compile time. Runtime handlers belong to A0.2, A0.3 and A0.4
//! (contract version 3, Hotfix 065).

use plumb_core::to_canonical_json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use thiserror::Error;
use utoipa::{OpenApi, ToSchema};

/// The contract version recorded as `x-plumb-contract-version`.
pub const CONTRACT_VERSION: u64 = 3;

const FIXTURE_CONTRACT: &str = include_str!("../../../fixtures/hr-leave/fixture-contract.yaml");
const COMPONENT_PREFIX: &str = "#/components/schemas/";
const FALLBACK_DATE: &str = "2026-01-15";
const FALLBACK_DATE_TIME: &str = "2026-01-15T12:00:00Z";
const FALLBACK_STRING: &str = "example";
const NOW_GATES: [&str; 5] = ["I0", "F1", "F2", "F3", "F4"];

/// Why the contract could not be generated or rendered.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum OpenApiContractError {
    #[error("HR fixture contract cannot be parsed: {0}")]
    FixtureParse(String),
    #[error("HR fixture seed is invalid: {0}")]
    InvalidFixtureSeed(String),
    #[error("schema not found: {0}")]
    MissingSchema(String),
    #[error("BLOCKED-TEST-CONTRACT: unsupported example fallback at {location}: {construct}")]
    UnsupportedFallback { location: String, construct: String },
    #[error("BLOCKED-TEST-CONTRACT: example invalid at {location}: {reason}")]
    ExampleValidation { location: String, reason: String },
    #[error("contract invariant violated: {0}")]
    ContractInvariant(String),
    #[error("serialization failed: {0}")]
    Serialization(String),
}

// ============================================================================ inventory

/// One frozen operation of the pilot contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationSpec {
    pub number: u32,
    pub method: &'static str,
    pub path: &'static str,
    pub operation_id: &'static str,
    pub tag: &'static str,
    /// Returns `Page {items, next_cursor}` and takes `cursor` and `limit`.
    pub pageable: bool,
    /// The implementation task that owns the runtime handler (Hotfix 063; Hotfix 065 moves
    /// `run_stage` and `get_job` to A0.4; Hotfix 073 adds the two U2.1 previews).
    pub owner: &'static str,
}

const fn op(
    number: u32,
    method: &'static str,
    path: &'static str,
    operation_id: &'static str,
    tag: &'static str,
    pageable: bool,
    owner: &'static str,
) -> OperationSpec {
    OperationSpec {
        number,
        method,
        path,
        operation_id,
        tag,
        pageable,
        owner,
    }
}

/// The 74 NOW operations in their frozen order: the v2 P and P-R operations plus proposal
/// accept (01-72, Hotfix 062), then the two U2.1 semantic change previews (73-74, Hotfix 073).
pub const OPERATIONS: [OperationSpec; 74] = [
    op(1, "GET", "/api/me", "get_me", "meta", false, "A0.2"),
    op(
        2,
        "GET",
        "/api/model/version",
        "get_model_version",
        "meta",
        false,
        "A0.2",
    ),
    op(3, "GET", "/api/gates", "get_gates", "meta", false, "A0.2"),
    op(
        4,
        "GET",
        "/api/search",
        "search_model",
        "meta",
        true,
        "A0.3",
    ),
    op(
        5,
        "GET",
        "/api/events",
        "events_websocket",
        "meta",
        false,
        "A0.2",
    ),
    op(
        6,
        "POST",
        "/api/sources",
        "create_source",
        "sources",
        false,
        "A0.2",
    ),
    op(
        7,
        "GET",
        "/api/sources/{id}",
        "get_source",
        "sources",
        false,
        "A0.2",
    ),
    op(
        8,
        "GET",
        "/api/sources/{id}/spans",
        "get_source_spans",
        "sources",
        true,
        "A0.2",
    ),
    op(
        9,
        "POST",
        "/api/stages/{stage}/run",
        "run_stage",
        "stages",
        false,
        "A0.4",
    ),
    op(
        10,
        "GET",
        "/api/jobs/{id}",
        "get_job",
        "stages",
        false,
        "A0.4",
    ),
    op(
        11,
        "GET",
        "/api/requirements",
        "list_requirements",
        "requirements",
        true,
        "A0.2",
    ),
    op(
        12,
        "GET",
        "/api/requirements/{id}",
        "get_requirement",
        "requirements",
        false,
        "A0.2",
    ),
    op(
        13,
        "POST",
        "/api/requirements/{id}/classify",
        "classify_requirement",
        "requirements",
        false,
        "A0.2",
    ),
    op(
        14,
        "POST",
        "/api/requirements/{id}/accept-ears",
        "accept_requirement_ears",
        "requirements",
        false,
        "A0.2",
    ),
    op(
        15,
        "POST",
        "/api/requirements/{id}/split",
        "split_requirement",
        "requirements",
        false,
        "A0.3",
    ),
    op(
        16,
        "POST",
        "/api/requirements/{id}/waive-finding",
        "waive_requirement_finding",
        "requirements",
        false,
        "A0.3",
    ),
    op(
        17,
        "POST",
        "/api/requirements/{id}/mark-duplicate",
        "mark_requirement_duplicate",
        "requirements",
        false,
        "A0.3",
    ),
    op(
        18,
        "GET",
        "/api/glossary",
        "list_glossary",
        "glossary",
        true,
        "A0.2",
    ),
    op(
        19,
        "POST",
        "/api/glossary/{id}/confirm",
        "confirm_glossary",
        "glossary",
        false,
        "A0.2",
    ),
    op(
        20,
        "POST",
        "/api/glossary/{id}/edit",
        "edit_glossary",
        "glossary",
        false,
        "A0.2",
    ),
    op(
        21,
        "POST",
        "/api/glossary/{id}/merge",
        "merge_glossary",
        "glossary",
        false,
        "A0.3",
    ),
    op(
        22,
        "GET",
        "/api/model/sentences",
        "list_sentences",
        "sentences",
        true,
        "A0.3",
    ),
    op(
        23,
        "POST",
        "/api/model/sentences/{id}/accept",
        "accept_sentence",
        "sentences",
        false,
        "A0.3",
    ),
    op(
        24,
        "POST",
        "/api/model/sentences/{id}/edit",
        "edit_sentence",
        "sentences",
        false,
        "A0.3",
    ),
    op(
        25,
        "POST",
        "/api/model/sentences/{id}/reject",
        "reject_sentence",
        "sentences",
        false,
        "A0.3",
    ),
    op(
        26,
        "GET",
        "/api/model/entities",
        "list_entities",
        "entities",
        true,
        "A0.2",
    ),
    op(
        27,
        "GET",
        "/api/model/entities/{id}",
        "get_entity",
        "entities",
        false,
        "A0.2",
    ),
    op(
        28,
        "GET",
        "/api/model/crud",
        "get_crud_matrix",
        "entities",
        false,
        "A0.3",
    ),
    op(
        29,
        "POST",
        "/api/model/attributes/{id}/type",
        "set_attribute_type",
        "entities",
        false,
        "A0.2",
    ),
    op(
        30,
        "POST",
        "/api/model/attributes/{id}/unit",
        "set_attribute_unit",
        "entities",
        false,
        "A0.2",
    ),
    op(
        31,
        "POST",
        "/api/model/attributes/{id}/class",
        "set_attribute_class",
        "entities",
        false,
        "A0.2",
    ),
    op(
        32,
        "GET",
        "/api/model/calculations",
        "list_calculations",
        "calculations",
        true,
        "A0.2",
    ),
    op(
        33,
        "GET",
        "/api/model/calculations/{id}/example",
        "get_calculation_example",
        "calculations",
        false,
        "A0.3",
    ),
    op(
        34,
        "POST",
        "/api/model/calculations/{id}/confirm",
        "confirm_calculation",
        "calculations",
        false,
        "A0.2",
    ),
    op(
        35,
        "GET",
        "/api/model/rules",
        "list_rules",
        "rules",
        true,
        "A0.3",
    ),
    op(
        36,
        "GET",
        "/api/model/rules/{id}",
        "get_rule",
        "rules",
        false,
        "A0.3",
    ),
    op(
        37,
        "POST",
        "/api/model/rules/{id}/cells",
        "set_rule_cell",
        "rules",
        false,
        "A0.3",
    ),
    op(
        38,
        "GET",
        "/api/processes",
        "list_processes",
        "processes",
        true,
        "A0.2",
    ),
    op(
        39,
        "GET",
        "/api/processes/{id}",
        "get_process",
        "processes",
        false,
        "A0.2",
    ),
    op(
        40,
        "POST",
        "/api/processes/{id}/steps",
        "mutate_process_steps",
        "processes",
        false,
        "A0.3",
    ),
    op(
        41,
        "POST",
        "/api/processes/import",
        "import_process",
        "processes",
        false,
        "A0.3",
    ),
    op(
        42,
        "GET",
        "/api/processes/{id}/export",
        "export_process",
        "processes",
        false,
        "A0.3",
    ),
    op(
        43,
        "GET",
        "/api/scenarios",
        "list_scenarios",
        "scenarios",
        true,
        "A0.3",
    ),
    op(
        44,
        "POST",
        "/api/scenarios/{id}/confirm",
        "confirm_scenario",
        "scenarios",
        false,
        "A0.3",
    ),
    op(
        45,
        "POST",
        "/api/scenarios/{id}/correct",
        "correct_scenario",
        "scenarios",
        false,
        "A0.3",
    ),
    op(
        46,
        "POST",
        "/api/scenarios/confirm-derived",
        "confirm_derived_scenarios",
        "scenarios",
        false,
        "A0.3",
    ),
    op(
        47,
        "POST",
        "/api/scenarios/run",
        "run_scenarios",
        "scenarios",
        false,
        "A0.3",
    ),
    op(
        48,
        "GET",
        "/api/scenarios/{id}/run",
        "get_scenario_run",
        "scenarios",
        false,
        "A0.3",
    ),
    op(
        49,
        "GET",
        "/api/findings",
        "list_findings",
        "findings",
        true,
        "A0.2",
    ),
    op(
        50,
        "POST",
        "/api/findings/{id}/waive",
        "waive_finding",
        "findings",
        false,
        "A0.3",
    ),
    op(
        51,
        "GET",
        "/api/assumptions",
        "list_assumptions",
        "assumptions",
        true,
        "A0.2",
    ),
    op(
        52,
        "POST",
        "/api/assumptions/{id}/resolve",
        "resolve_assumption",
        "assumptions",
        false,
        "A0.3",
    ),
    op(
        53,
        "GET",
        "/api/questions",
        "list_questions",
        "questions",
        true,
        "A0.3",
    ),
    op(
        54,
        "POST",
        "/api/questions/{id}/answer",
        "answer_question",
        "questions",
        false,
        "A0.3",
    ),
    op(
        55,
        "POST",
        "/api/rounds",
        "create_round",
        "questions",
        false,
        "A0.3",
    ),
    op(
        56,
        "GET",
        "/api/rounds/{id}",
        "get_round",
        "questions",
        false,
        "A0.3",
    ),
    op(
        57,
        "POST",
        "/api/proposals/intake",
        "intake_proposal",
        "proposals",
        false,
        "A0.3",
    ),
    op(
        58,
        "POST",
        "/api/proposals/{id}/link",
        "link_proposal",
        "proposals",
        false,
        "A0.3",
    ),
    op(
        59,
        "POST",
        "/api/proposals/{id}/merge",
        "merge_proposal",
        "proposals",
        false,
        "A0.3",
    ),
    op(
        60,
        "POST",
        "/api/proposals/{id}/supersede",
        "supersede_proposal",
        "proposals",
        false,
        "A0.3",
    ),
    op(
        61,
        "POST",
        "/api/proposals/{id}/create",
        "create_proposal",
        "proposals",
        false,
        "A0.3",
    ),
    op(
        62,
        "POST",
        "/api/proposals/{id}/reject",
        "reject_proposal",
        "proposals",
        false,
        "A0.3",
    ),
    op(
        63,
        "POST",
        "/api/proposals/{id}/accept",
        "accept_proposal",
        "proposals",
        false,
        "A0.3",
    ),
    op(
        64,
        "POST",
        "/api/sessions",
        "create_session",
        "sessions",
        false,
        "A0.3",
    ),
    op(
        65,
        "POST",
        "/api/sessions/{id}/turns",
        "create_session_turn",
        "sessions",
        false,
        "A0.3",
    ),
    op(
        66,
        "GET",
        "/api/sessions/{id}",
        "get_session",
        "sessions",
        false,
        "A0.3",
    ),
    op(
        67,
        "GET",
        "/api/sessions/{id}/metrics",
        "get_session_metrics",
        "sessions",
        false,
        "A0.3",
    ),
    op(
        68,
        "GET",
        "/api/decisions",
        "list_decisions",
        "decisions",
        true,
        "A0.2",
    ),
    op(
        69,
        "GET",
        "/api/requirements/{id}/readiness",
        "get_requirement_readiness",
        "readiness",
        false,
        "A0.2",
    ),
    op(
        70,
        "GET",
        "/api/readiness",
        "get_readiness",
        "readiness",
        false,
        "A0.2",
    ),
    op(
        71,
        "GET",
        "/api/model/export",
        "export_model",
        "export",
        false,
        "A0.2",
    ),
    op(
        72,
        "GET",
        "/api/model/diff",
        "get_model_diff",
        "export",
        false,
        "A0.2",
    ),
    op(
        73,
        "POST",
        "/api/requirements/{id}/split/preview",
        "preview_requirement_split",
        "requirements",
        false,
        "U2.1",
    ),
    op(
        74,
        "POST",
        "/api/glossary/{id}/merge/preview",
        "preview_glossary_merge",
        "glossary",
        false,
        "U2.1",
    ),
];

/// The WebSocket compatibility operation.
pub const EVENTS_OPERATION_ID: &str = "events_websocket";

// ============================================================================ common DTOs

/// A design change notification: what changed and at which revision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DesignEvent {
    pub kind: String,
    pub ids: Vec<String>,
    /// The revision compatibility value associated with the event.
    pub version: String,
}

/// The result of a successful write. `version` is the compatibility alias of `revision`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct WriteResult {
    pub version: String,
    pub revision: String,
    pub semantic_hash: String,
    pub events: Vec<DesignEvent>,
}

/// 412: the request's If-Match was not the current semantic hash. `revision` and
/// `semantic_hash` are the current branch-head values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PreconditionFailedResponse {
    pub code: String,
    pub message: String,
    #[schema(value_type = Object)]
    pub details: Value,
    pub revision: String,
    pub semantic_hash: String,
}

/// The only cursor error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum CursorErrorCode {
    #[serde(rename = "E_CURSOR_BASELINE_CHANGED")]
    #[schema(rename = "E_CURSOR_BASELINE_CHANGED")]
    CursorBaselineChanged,
}

/// 409: the cursor was issued for another semantic hash.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CursorConflictResponse {
    pub code: CursorErrorCode,
    pub message: String,
    #[schema(value_type = Object)]
    pub details: Value,
}

/// A JSON scalar: string, number (integers included) or boolean. The branches are disjoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(untagged)]
pub enum ScalarValue {
    Text(String),
    Number(f64),
    Boolean(bool),
}

/// A binary file body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[schema(value_type = String, format = Binary)]
pub struct BinaryFile(pub Vec<u8>);

macro_rules! page {
    ($name:ident, $item:ty) => {
        /// A page of results; `next_cursor` is null on the last page.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            pub items: Vec<$item>,
            pub next_cursor: Option<String>,
        }
    };
}

// ============================================================================ meta

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MeResponse {
    pub user: String,
    pub roles: Vec<String>,
    pub mode: String,
}

/// Model identity. `version` == `revision` and `hash` == `semantic_hash` (compatibility
/// aliases).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelVersionResponse {
    /// Compatibility alias of `revision`.
    pub version: String,
    /// Compatibility alias of `semantic_hash`.
    pub hash: String,
    pub revision: String,
    pub semantic_hash: String,
    pub evidence_hash: String,
}

/// A NOW-scope gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum GateName {
    I0,
    F1,
    F2,
    F3,
    F4,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GateStatus {
    pub gate: GateName,
    pub passed: bool,
    /// Finding IDs.
    pub findings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchHit {
    pub id: String,
    /// The exact retrieval document kind wire string.
    pub kind: String,
    /// The exact detail route of the hit; `null` means there is no exact detail route for its
    /// indexed family (Hotfix 065).
    #[schema(required = true)]
    pub path: Option<String>,
    pub score: f64,
    pub snippet: String,
}
page!(SearchPage, SearchHit);

/// The indexed retrieval families of `GET /api/search` (Hotfix 065).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchScope {
    Requirement,
    Concept,
    Rule,
    Calculation,
    Scenario,
    Evidence,
}

// ============================================================================ sources and stages

/// Multipart source upload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct SourceUpload {
    #[schema(value_type = String, format = Binary)]
    pub file: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceUploadResponse {
    pub version: String,
    pub revision: String,
    pub semantic_hash: String,
    pub events: Vec<DesignEvent>,
    pub id: String,
    pub hash: String,
    pub spans_count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: String,
    pub hash: String,
    pub media_type: String,
    pub source_kind: String,
}

/// One evidence span of a source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Span {
    pub id: String,
    pub source_ref: String,
    pub locator_kind: String,
    pub start: Option<u64>,
    pub end: Option<u64>,
    pub content_hash: String,
}
page!(SpanPage, Span);

/// Compatibility stage names of `POST /api/stages/{stage}/run`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum StageName {
    Intake,
    Lint,
    Extract,
    Consolidate,
    Calc,
    Rules,
    Analyse,
    Flows,
    Scenarios,
    Tags,
    Run,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct StageRunRequest {
    pub scope: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct StageRunResponse {
    pub version: String,
    pub revision: String,
    pub semantic_hash: String,
    pub events: Vec<DesignEvent>,
    pub job_id: String,
}

/// Stage job status (X1.5); `stale` is a status, not a flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Acquiring,
    Evaluating,
    Ready,
    Stale,
    Failed,
    Committed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct JobResponse {
    pub status: JobStatus,
    #[schema(minimum = 0.0, maximum = 1.0)]
    pub progress: f64,
    /// The branch revision the job captured.
    pub base_revision: String,
    pub result_version: Option<String>,
    pub error: Option<String>,
}

// ============================================================================ requirements

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RequirementCoverage {
    pub operations: Vec<String>,
    pub scenarios: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RequirementSummary {
    pub id: String,
    pub text: String,
    pub class: String,
    pub status: String,
    /// Error-severity lint rule IDs recorded for this requirement by the current F1 gate report
    /// (F1 evaluates Accepted Requirements only).
    pub smells: Vec<String>,
    pub coverage: RequirementCoverage,
    /// Advisory readiness; `null` means not calculated by the current pilot runtime.
    pub readiness: Option<f64>,
}
page!(RequirementPage, RequirementSummary);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RequirementLinks {
    pub operations: Vec<String>,
    pub processes: Vec<String>,
    pub scenarios: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RequirementDetail {
    pub id: String,
    pub text: String,
    pub class: String,
    pub status: String,
    pub smells: Vec<String>,
    pub coverage: RequirementCoverage,
    pub readiness: Option<f64>,
    pub spans: Vec<String>,
    /// IDs of the current F1 gate report's findings whose affected refs include this
    /// requirement.
    pub lint_findings: Vec<String>,
    pub ears_suggestion: Option<String>,
    pub linked: RequirementLinks,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ClassifyRequest {
    pub class: String,
}

/// The exact EARS-normalized statement the caller reviewed. EARS suggestions are not graph
/// state, so the server never rediscovers or regenerates one; the text replaces the current
/// statement through a human-confirm proposal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AcceptEarsRequest {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SplitRequest {
    pub parts: Vec<String>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct WaiveRequirementFindingRequest {
    pub finding_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MarkDuplicateRequest {
    pub of: String,
    pub reason: String,
}

// ============================================================================ glossary and sentences

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GlossaryTerm {
    pub id: String,
    pub term: String,
    pub language: String,
    pub aliases: Vec<String>,
    pub definition_ref: Option<String>,
    pub status: String,
}
page!(GlossaryPage, GlossaryTerm);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GlossaryEditRequest {
    pub term: String,
    pub aliases: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GlossaryMergeRequest {
    pub into: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SentenceUnknown {
    pub slot: String,
    pub question_id: String,
}

/// The typed slots of a Sentence: the exact requirement_kind, level and modality wire values
/// of its Requirement (Hotfix 065).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SentenceSlots {
    pub class: String,
    pub level: String,
    pub modality: String,
}

/// The supplied slot changes of a Sentence edit; at least one is required at runtime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SentenceSlotPatch {
    pub class: Option<String>,
    pub level: Option<String>,
    pub modality: Option<String>,
}

/// A compatibility view of exactly one Requirement (there is no Sentence node type).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Sentence {
    pub id: String,
    pub text: String,
    pub slots: SentenceSlots,
    pub status: String,
    pub evidence: Vec<String>,
    pub unknowns: Vec<SentenceUnknown>,
}
page!(SentencePage, Sentence);

/// The edited typed slots of a sentence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SentenceEditRequest {
    pub slots: SentenceSlotPatch,
}

// ============================================================================ entities

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EntityAttribute {
    pub name: String,
    #[serde(rename = "type")]
    #[schema(rename = "type")]
    pub value_type: String,
    pub nullable: bool,
    pub unit: Option<String>,
    pub class: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EntityRelationship {
    pub id: String,
    pub target: String,
    pub kind: String,
    pub cardinality: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EntityTransition {
    pub from: String,
    pub to: String,
    pub operation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Entity {
    pub id: String,
    pub name: String,
    pub attributes: Vec<EntityAttribute>,
    pub relationships: Vec<EntityRelationship>,
    pub states: Vec<String>,
    pub transitions: Vec<EntityTransition>,
    pub tags: Vec<String>,
}
page!(EntityPage, Entity);

/// One CRUD cell. The PSG distinguishes only `reads` and `writes`: `null` in `c`, `u` or `d`
/// means not distinguishable from current PSG semantics, not false (Hotfix 065).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CrudCell {
    pub op: String,
    pub ent: String,
    #[schema(required = true)]
    pub c: Option<bool>,
    pub r: bool,
    #[schema(required = true)]
    pub u: Option<bool>,
    #[schema(required = true)]
    pub d: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CrudMatrix {
    pub operations: Vec<String>,
    pub entities: Vec<String>,
    pub cells: Vec<CrudCell>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AttributeTypeRequest {
    #[serde(rename = "type")]
    #[schema(rename = "type")]
    pub value_type: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AttributeUnitRequest {
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AttributeClassRequest {
    pub class: String,
}

// ============================================================================ calculations and rules

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Calculation {
    pub id: String,
    pub name: String,
    pub expr: String,
}
page!(CalculationPage, Calculation);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CalculationExample {
    pub expr: String,
    pub rendered_example: String,
    pub value: CalculationValue,
}

/// An exact, non-recursive PlumbExpr value (Hotfix 065). Decimal, duration and quantity values
/// are exact decimal strings and never pass through a binary float.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CalculationValue {
    Integer { value: i64 },
    Decimal { value: String },
    Boolean { value: bool },
    String { value: String },
    Date { value: String },
    DateTime { value: String },
    Duration { value: String, unit: String },
    Quantity { value: String, unit: String },
    Enum { enum_ref: String, member: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleRow {
    pub when: Vec<String>,
    pub then: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub kind: RuleViewKind,
    pub conditions: Vec<String>,
    /// `null` means not applicable or not analyzable, not empty (Hotfix 065).
    #[schema(required = true)]
    pub rows: Option<Vec<RuleRow>>,
    #[schema(required = true)]
    pub uncovered_cells: Option<Vec<String>>,
    #[schema(required = true)]
    pub overlaps: Option<Vec<String>>,
}
page!(RulePage, Rule);

/// Whether a Rule view projects a scalar PSG Rule or a PSG DecisionTable (Hotfix 065).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuleViewKind {
    Rule,
    DecisionTable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleCellRequest {
    /// `row:<row>:output:<output column>`, zero-based.
    pub cell: String,
    /// The exact RFC 8785 JSON of one DecisionOutputCell.
    pub outcome: String,
    pub reason: String,
}

// ============================================================================ processes

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Process {
    pub id: String,
    pub name: String,
    pub lanes: Vec<String>,
    /// Step operation names in process order.
    pub steps: Vec<String>,
    pub branches: Vec<String>,
    pub validation: Vec<String>,
}
page!(ProcessPage, Process);

/// NOW process-step mutations of a strict linear Process, tagged by `op` (no `branch`;
/// Hotfix 065). `position` is zero-based among interior steps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "op", rename_all = "lowercase", deny_unknown_fields)]
pub enum ProcessStepRequest {
    Add {
        operation_ref: String,
        position: u64,
    },
    Move {
        node_ref: String,
        position: u64,
    },
    Remove {
        node_ref: String,
    },
}

/// NOW process interchange format (no `svg`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ProcessFormat {
    Mermaid,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessImportRequest {
    pub format: ProcessFormat,
    pub text: String,
}

// ============================================================================ scenarios, findings, assumptions

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ScenarioRunResult {
    Pass,
    Fail,
    Undecidable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ScenarioRun {
    pub result: ScenarioRunResult,
    pub diff: Option<Vec<String>>,
    pub missing: Option<Vec<String>>,
    pub trace: Vec<String>,
}

/// A Scenario view (Hotfix 065). `given`, `when` and `then` are RFC 8785 canonical JSON texts of
/// the exact PSG values; `status` is the node ElementStatus and `confirmation_status` the
/// Scenario confirmation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub id: String,
    pub given: Vec<String>,
    pub when: String,
    pub then: Vec<String>,
    pub status: ScenarioStatus,
    pub confirmation_status: ScenarioConfirmationStatus,
    pub derived_from: Vec<String>,
    #[schema(required = true)]
    pub run: Option<ScenarioRun>,
}
page!(ScenarioPage, Scenario);

/// The ElementStatus of a Scenario node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum ScenarioStatus {
    Proposed,
    Accepted,
    Rejected,
    Superseded,
    Deprecated,
    Suspect,
}

/// The pilot Scenario confirmation status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum ScenarioConfirmationStatus {
    Derived,
    Confirmed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ScenarioCorrectRequest {
    pub then: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfirmDerivedRequest {
    pub table_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ScenarioRunRequest {
    pub scope: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub id: String,
    pub code: String,
    pub family: String,
    pub severity: String,
    pub status: String,
    pub message: String,
    pub affected_refs: Vec<String>,
    pub waiver_ref: Option<String>,
}
page!(FindingPage, Finding);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReasonRequest {
    pub reason: String,
}

/// A recorded assumption; `default_value` is recorded material, not executable truth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Assumption {
    pub id: String,
    pub statement: String,
    pub owner_ref: String,
    pub status: String,
    pub finding_ref: Option<String>,
    pub default_value: Option<ScalarValue>,
    #[schema(format = DateTime)]
    pub expires_at: Option<String>,
    pub risk_ref: Option<String>,
}
page!(AssumptionPage, Assumption);

// ============================================================================ questions and proposals

/// The exact PSG `Question.question_kind` spellings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum QuestionKind {
    YesNo,
    PickOne,
    PickMany,
    Number,
    Text,
    Cardinality,
    Unit,
    Precision,
    Rounding,
    FormulaConfirm,
    RuleCell,
    Calendar,
    RoleAssignment,
    Permission,
    QualityThreshold,
    ArchitectureChoice,
    TechnologyChoice,
    InterfaceChoice,
    VerificationMethod,
}

/// The persisted Question lifecycle statuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum QuestionStatus {
    Open,
    Answered,
    Closed,
    Superseded,
}

/// The persisted projection of one Accepted PSG Question (Hotfix 065). Generation-time slots
/// are not persisted and are not part of the contract; `answer_schema` is the typed-answer
/// authority and `context_refs` are the exact persisted context IDs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Question {
    pub id: String,
    pub kind: QuestionKind,
    /// The persisted prompt.
    pub text: String,
    pub status: QuestionStatus,
    #[schema(value_type = Option<Object>, required = true)]
    pub answer_schema: Option<Value>,
    #[schema(required = true)]
    pub stakeholder_ref: Option<String>,
    /// The canonical unsigned priority.
    #[schema(required = true)]
    pub priority: Option<u64>,
    #[schema(required = true)]
    pub round_ref: Option<String>,
    pub context_refs: Vec<String>,
}
page!(QuestionPage, Question);

/// An answer to a Question (Hotfix 065): `answer` is validated against the persisted
/// `Question.answer_schema` (every pilot answer schema is a closed JSON object).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AnswerRequest {
    #[schema(value_type = Object)]
    pub answer: Value,
    pub rationale: Option<String>,
    /// The Accepted ResolutionDecision this answer supersedes.
    pub supersedes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CoverageView {
    pub subject_ref: String,
    pub covered_by_ref: String,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ConflictView {
    pub target_ref: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DuplicateCandidateView {
    pub subject_ref: String,
    pub candidate_ref: String,
    pub score: f64,
}

/// X1.3 intake reason codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum IntakeReasonCode {
    MaterialDecision,
    HumanDecision,
    ExactEquivalent,
    DeclaredConflict,
    ValidationConflict,
    AmbiguousTerm,
}

/// The public compatibility projection of an X1.3 intake report (report data only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct IntakeReportView {
    pub covered_by: Vec<CoverageView>,
    /// Declared conflicts plus surfaced validation conflict findings.
    pub conflicts_with: Vec<ConflictView>,
    pub refines: Vec<String>,
    /// Terms resolved as New.
    pub new_terms: Vec<String>,
    #[schema(minimum = 0.0, maximum = 1.0)]
    pub novelty: Option<f64>,
    pub duplicate_candidates: Vec<DuplicateCandidateView>,
    pub validation_findings: Vec<Finding>,
    pub reason_codes: Vec<IntakeReasonCode>,
    pub requires_reason: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AnswerResponse {
    pub version: String,
    pub revision: String,
    pub semantic_hash: String,
    pub events: Vec<DesignEvent>,
    pub proposal_id: String,
    /// The authoritative workflow class of the exact pending Proposal (Hotfix 076): the one
    /// existing proposal action endpoint that can confirm it.
    pub action: ProposalKind,
    pub intake: IntakeReportView,
    #[schema(value_type = Object)]
    pub patch_preview: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateRoundRequest {
    pub stakeholder: String,
    pub max: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Round {
    pub id: String,
    pub stakeholder: String,
    pub questions: Vec<String>,
}

/// The composed round of `POST /api/rounds` and the write that assigned it (Hotfix 065).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateRoundResponse {
    pub version: String,
    pub revision: String,
    pub semantic_hash: String,
    pub events: Vec<DesignEvent>,
    pub round: Round,
}

/// A dry-run intake of one complete typed Proposal (Hotfix 065): `payload` is the exact JSON
/// of a `plumb_patch::Proposal`, `kind` its derived ProposalKind and `evidence` exactly its
/// sorted unique evidence refs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct IntakeRequest {
    pub kind: ProposalKind,
    #[schema(value_type = Object)]
    pub payload: Value,
    pub evidence: Vec<String>,
}

/// The closed workflow action classification of a Proposal (Hotfix 065); it does not change
/// materiality or acceptance policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProposalKind {
    Link,
    Merge,
    Supersede,
    Create,
    Accept,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct IntakeResponse {
    pub version: String,
    pub revision: String,
    pub semantic_hash: String,
    pub events: Vec<DesignEvent>,
    pub intake: IntakeReportView,
}

/// The semantic element effect of dry-applying one exact server-built domain Proposal to the
/// baseline graph (Hotfix 073). `candidate_semantic_hash` is the semantic hash of that candidate
/// graph only: it is not a revision identity and not a prediction of the committed
/// `semantic_hash`, because acceptance governance (the proposal-acceptance ResolutionDecision)
/// may change it. The ID lists hold only semantic-hash-participating nodes and edges, each
/// unique and sorted ascending.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SemanticElementDiff {
    pub base_semantic_hash: String,
    pub candidate_semantic_hash: String,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub modified: Vec<String>,
}

/// A server-authoritative semantic change preview (Hotfix 073): the exact domain Proposal is
/// persisted as a pending workflow proposal bound to the baseline revision and nothing is
/// committed. `version`, `revision` and `semantic_hash` are the baseline; `events` is empty.
/// Confirmation is the existing proposal action on `proposal_id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SemanticChangePreviewResponse {
    pub version: String,
    pub revision: String,
    pub semantic_hash: String,
    pub events: Vec<DesignEvent>,
    pub proposal_id: String,
    pub action: ProposalKind,
    pub intake: IntakeReportView,
    pub diff: SemanticElementDiff,
}

/// A proposal action; `reason` is required at runtime when intake demands one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProposalActionRequest {
    pub reason: Option<String>,
}

// ============================================================================ sessions, decisions, readiness, export

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateSessionRequest {
    pub scope: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateSessionResponse {
    pub version: String,
    pub revision: String,
    pub semantic_hash: String,
    pub events: Vec<DesignEvent>,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionTurnRequest {
    pub text: String,
    pub stage: SessionStage,
}

/// The active compiler stage of a session turn, exactly `plumb_core::StageId` (Hotfix 065).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum SessionStage {
    S0,
    S1,
    S2,
    S3,
    S4,
    S5,
    S6,
    S7,
    S8,
    S9,
    S10,
    S11,
    S12,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionProposalView {
    pub id: String,
    pub kind: String,
    #[schema(value_type = Object)]
    pub payload: Value,
    pub intake: IntakeReportView,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionTurnResponse {
    pub version: String,
    pub revision: String,
    pub semantic_hash: String,
    pub events: Vec<DesignEvent>,
    pub reply: String,
    pub proposals: Vec<SessionProposalView>,
    pub asked_question: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionDetail {
    pub id: String,
    pub scope: Vec<String>,
    pub assistant_actor_ref: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SessionMetrics {
    pub turns: u64,
    pub proposals: u64,
    pub requires_reason: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub id: String,
    pub proposal_ref: Option<String>,
    pub question_ref: Option<String>,
    pub decided_by: String,
    #[schema(format = DateTime)]
    pub decided_at: String,
    pub rationale: Option<String>,
}
page!(DecisionPage, Decision);

/// Readiness sub-scores. `null` means not calculated by the current pilot runtime, not zero.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadinessSubScores {
    #[schema(required = true)]
    pub clarity: Option<f64>,
    #[schema(required = true)]
    pub completeness: Option<f64>,
    #[schema(required = true)]
    pub consistency: Option<f64>,
    #[schema(required = true)]
    pub verifiability: Option<f64>,
    #[schema(required = true)]
    pub confirmation: Option<f64>,
}

/// Readiness; `overall` is an uncalibrated, non-authoritative pilot composite. `null` means not
/// calculated by the current pilot runtime; readiness never influences a gate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Readiness {
    pub overall: Option<f64>,
    pub sub: ReadinessSubScores,
    pub contributing: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ReadinessGroup {
    Feature,
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Functional,
    Report,
    Decisions,
    Glossary,
}

/// Stable element IDs changed between two revisions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelDiff {
    pub from: String,
    pub to: String,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub modified: Vec<String>,
}

// ============================================================================ operation stubs
//
// Contract-only: each function exists solely to carry its #[utoipa::path] metadata. If-Match,
// 412, ETag and the pagination parameters and 409 response are added uniformly from
// `OPERATIONS` when the document is generated.

#[allow(dead_code)]
mod stubs {
    use super::*;

    #[utoipa::path(get, path = "/api/me", operation_id = "get_me", tag = "meta",
        responses((status = 200, description = "Current user", body = MeResponse)))]
    pub fn get_me() {}

    #[utoipa::path(get, path = "/api/model/version", operation_id = "get_model_version", tag = "meta",
        responses((status = 200, description = "Model identity; version == revision and hash == semantic_hash", body = ModelVersionResponse)))]
    pub fn get_model_version() {}

    #[utoipa::path(get, path = "/api/gates", operation_id = "get_gates", tag = "meta",
        responses((status = 200, description = "NOW gates in the order I0, F1, F2, F3, F4", body = Vec<GateStatus>)))]
    pub fn get_gates() {}

    #[utoipa::path(get, path = "/api/search", operation_id = "search_model", tag = "meta",
        params(("q" = Option<String>, Query, description = "Search text"),
               ("scope" = Option<SearchScope>, Query, description = "Search scope")),
        responses((status = 200, description = "Search hits", body = SearchPage)))]
    pub fn search_model() {}

    #[utoipa::path(get, path = "/api/events", operation_id = "events_websocket", tag = "meta",
        responses((status = 101, description = "Switching Protocols: WebSocket upgrade; server messages are DesignEvent")))]
    pub fn events_websocket() {}

    #[utoipa::path(post, path = "/api/sources", operation_id = "create_source", tag = "sources",
        request_body(content = SourceUpload, content_type = "multipart/form-data"),
        responses((status = 200, description = "Source imported", body = SourceUploadResponse)))]
    pub fn create_source() {}

    #[utoipa::path(get, path = "/api/sources/{id}", operation_id = "get_source", tag = "sources",
        params(("id" = String, Path, description = "Source ID")),
        responses((status = 200, description = "Source", body = Source)))]
    pub fn get_source() {}

    #[utoipa::path(get, path = "/api/sources/{id}/spans", operation_id = "get_source_spans", tag = "sources",
        params(("id" = String, Path, description = "Source ID")),
        responses((status = 200, description = "Spans", body = SpanPage)))]
    pub fn get_source_spans() {}

    #[utoipa::path(post, path = "/api/stages/{stage}/run", operation_id = "run_stage", tag = "stages",
        params(("stage" = StageName, Path, description = "Compatibility stage name")),
        request_body(content = StageRunRequest),
        responses((status = 200, description = "Job queued", body = StageRunResponse)))]
    pub fn run_stage() {}

    #[utoipa::path(get, path = "/api/jobs/{id}", operation_id = "get_job", tag = "stages",
        params(("id" = String, Path, description = "Job ID")),
        responses((status = 200, description = "Job status, base revision and result", body = JobResponse)))]
    pub fn get_job() {}

    #[utoipa::path(get, path = "/api/requirements", operation_id = "list_requirements", tag = "requirements",
        params(("status" = Option<String>, Query, description = "Status filter"),
               ("class" = Option<String>, Query, description = "Class filter"),
               ("q" = Option<String>, Query, description = "Text filter")),
        responses((status = 200, description = "Requirements", body = RequirementPage)))]
    pub fn list_requirements() {}

    #[utoipa::path(get, path = "/api/requirements/{id}", operation_id = "get_requirement", tag = "requirements",
        params(("id" = String, Path, description = "Requirement ID")),
        responses((status = 200, description = "Requirement", body = RequirementDetail)))]
    pub fn get_requirement() {}

    #[utoipa::path(post, path = "/api/requirements/{id}/classify", operation_id = "classify_requirement", tag = "requirements",
        params(("id" = String, Path, description = "Requirement ID")),
        request_body(content = ClassifyRequest),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn classify_requirement() {}

    #[utoipa::path(post, path = "/api/requirements/{id}/accept-ears", operation_id = "accept_requirement_ears", tag = "requirements",
        params(("id" = String, Path, description = "Requirement ID")),
        request_body(content = AcceptEarsRequest),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn accept_requirement_ears() {}

    #[utoipa::path(post, path = "/api/requirements/{id}/split", operation_id = "split_requirement", tag = "requirements",
        params(("id" = String, Path, description = "Requirement ID")),
        request_body(content = SplitRequest),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn split_requirement() {}

    #[utoipa::path(post, path = "/api/requirements/{id}/waive-finding", operation_id = "waive_requirement_finding", tag = "requirements",
        params(("id" = String, Path, description = "Requirement ID")),
        request_body(content = WaiveRequirementFindingRequest),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn waive_requirement_finding() {}

    #[utoipa::path(post, path = "/api/requirements/{id}/mark-duplicate", operation_id = "mark_requirement_duplicate", tag = "requirements",
        params(("id" = String, Path, description = "Requirement ID")),
        request_body(content = MarkDuplicateRequest),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn mark_requirement_duplicate() {}

    #[utoipa::path(get, path = "/api/glossary", operation_id = "list_glossary", tag = "glossary",
        responses((status = 200, description = "Glossary terms", body = GlossaryPage)))]
    pub fn list_glossary() {}

    #[utoipa::path(post, path = "/api/glossary/{id}/confirm", operation_id = "confirm_glossary", tag = "glossary",
        params(("id" = String, Path, description = "Term ID")),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn confirm_glossary() {}

    #[utoipa::path(post, path = "/api/glossary/{id}/edit", operation_id = "edit_glossary", tag = "glossary",
        params(("id" = String, Path, description = "Term ID")),
        request_body(content = GlossaryEditRequest),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn edit_glossary() {}

    #[utoipa::path(post, path = "/api/glossary/{id}/merge", operation_id = "merge_glossary", tag = "glossary",
        params(("id" = String, Path, description = "Term ID")),
        request_body(content = GlossaryMergeRequest),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn merge_glossary() {}

    #[utoipa::path(get, path = "/api/model/sentences", operation_id = "list_sentences", tag = "sentences",
        params(("entity" = Option<String>, Query, description = "Entity filter"),
               ("status" = Option<String>, Query, description = "Status filter")),
        responses((status = 200, description = "Sentences", body = SentencePage)))]
    pub fn list_sentences() {}

    #[utoipa::path(post, path = "/api/model/sentences/{id}/accept", operation_id = "accept_sentence", tag = "sentences",
        params(("id" = String, Path, description = "Sentence ID")),
        responses((status = 200, description = "Write result (through intake)", body = WriteResult)))]
    pub fn accept_sentence() {}

    #[utoipa::path(post, path = "/api/model/sentences/{id}/edit", operation_id = "edit_sentence", tag = "sentences",
        params(("id" = String, Path, description = "Sentence ID")),
        request_body(content = SentenceEditRequest),
        responses((status = 200, description = "Write result (through intake)", body = WriteResult)))]
    pub fn edit_sentence() {}

    #[utoipa::path(post, path = "/api/model/sentences/{id}/reject", operation_id = "reject_sentence", tag = "sentences",
        params(("id" = String, Path, description = "Sentence ID")),
        responses((status = 200, description = "Write result (through intake)", body = WriteResult)))]
    pub fn reject_sentence() {}

    #[utoipa::path(get, path = "/api/model/entities", operation_id = "list_entities", tag = "entities",
        responses((status = 200, description = "Entities", body = EntityPage)))]
    pub fn list_entities() {}

    #[utoipa::path(get, path = "/api/model/entities/{id}", operation_id = "get_entity", tag = "entities",
        params(("id" = String, Path, description = "Entity ID")),
        responses((status = 200, description = "Entity", body = Entity)))]
    pub fn get_entity() {}

    #[utoipa::path(get, path = "/api/model/crud", operation_id = "get_crud_matrix", tag = "entities",
        responses((status = 200, description = "CRUD matrix", body = CrudMatrix)))]
    pub fn get_crud_matrix() {}

    #[utoipa::path(post, path = "/api/model/attributes/{id}/type", operation_id = "set_attribute_type", tag = "entities",
        params(("id" = String, Path, description = "Attribute ID")),
        request_body(content = AttributeTypeRequest),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn set_attribute_type() {}

    #[utoipa::path(post, path = "/api/model/attributes/{id}/unit", operation_id = "set_attribute_unit", tag = "entities",
        params(("id" = String, Path, description = "Attribute ID")),
        request_body(content = AttributeUnitRequest),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn set_attribute_unit() {}

    #[utoipa::path(post, path = "/api/model/attributes/{id}/class", operation_id = "set_attribute_class", tag = "entities",
        params(("id" = String, Path, description = "Attribute ID")),
        request_body(content = AttributeClassRequest),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn set_attribute_class() {}

    #[utoipa::path(get, path = "/api/model/calculations", operation_id = "list_calculations", tag = "calculations",
        responses((status = 200, description = "Calculations", body = CalculationPage)))]
    pub fn list_calculations() {}

    #[utoipa::path(get, path = "/api/model/calculations/{id}/example", operation_id = "get_calculation_example", tag = "calculations",
        params(("id" = String, Path, description = "Calculation ID"),
               ("inputs" = Option<String>, Query, description = "Example inputs")),
        responses((status = 200, description = "Read-only rendered example", body = CalculationExample)))]
    pub fn get_calculation_example() {}

    #[utoipa::path(post, path = "/api/model/calculations/{id}/confirm", operation_id = "confirm_calculation", tag = "calculations",
        params(("id" = String, Path, description = "Calculation ID")),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn confirm_calculation() {}

    #[utoipa::path(get, path = "/api/model/rules", operation_id = "list_rules", tag = "rules",
        responses((status = 200, description = "Rules", body = RulePage)))]
    pub fn list_rules() {}

    #[utoipa::path(get, path = "/api/model/rules/{id}", operation_id = "get_rule", tag = "rules",
        params(("id" = String, Path, description = "Rule ID")),
        responses((status = 200, description = "Rule", body = Rule)))]
    pub fn get_rule() {}

    #[utoipa::path(post, path = "/api/model/rules/{id}/cells", operation_id = "set_rule_cell", tag = "rules",
        params(("id" = String, Path, description = "Rule ID")),
        request_body(content = RuleCellRequest),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn set_rule_cell() {}

    #[utoipa::path(get, path = "/api/processes", operation_id = "list_processes", tag = "processes",
        responses((status = 200, description = "Processes", body = ProcessPage)))]
    pub fn list_processes() {}

    #[utoipa::path(get, path = "/api/processes/{id}", operation_id = "get_process", tag = "processes",
        params(("id" = String, Path, description = "Process ID")),
        responses((status = 200, description = "Process", body = Process)))]
    pub fn get_process() {}

    #[utoipa::path(post, path = "/api/processes/{id}/steps", operation_id = "mutate_process_steps", tag = "processes",
        params(("id" = String, Path, description = "Process ID")),
        request_body(content = ProcessStepRequest),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn mutate_process_steps() {}

    #[utoipa::path(post, path = "/api/processes/import", operation_id = "import_process", tag = "processes",
        request_body(content = ProcessImportRequest),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn import_process() {}

    #[utoipa::path(get, path = "/api/processes/{id}/export", operation_id = "export_process", tag = "processes",
        params(("id" = String, Path, description = "Process ID"),
               ("format" = ProcessFormat, Query, description = "Export format")),
        responses((status = 200, description = "Mermaid text", body = String, content_type = "text/plain")))]
    pub fn export_process() {}

    #[utoipa::path(get, path = "/api/scenarios", operation_id = "list_scenarios", tag = "scenarios",
        params(("requirement" = Option<String>, Query, description = "Requirement filter"),
               ("status" = Option<ScenarioStatus>, Query, description = "Status filter"),
               ("derived" = Option<bool>, Query, description = "Derived filter")),
        responses((status = 200, description = "Scenarios", body = ScenarioPage)))]
    pub fn list_scenarios() {}

    #[utoipa::path(post, path = "/api/scenarios/{id}/confirm", operation_id = "confirm_scenario", tag = "scenarios",
        params(("id" = String, Path, description = "Scenario ID")),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn confirm_scenario() {}

    #[utoipa::path(post, path = "/api/scenarios/{id}/correct", operation_id = "correct_scenario", tag = "scenarios",
        params(("id" = String, Path, description = "Scenario ID")),
        request_body(content = ScenarioCorrectRequest),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn correct_scenario() {}

    #[utoipa::path(post, path = "/api/scenarios/confirm-derived", operation_id = "confirm_derived_scenarios", tag = "scenarios",
        request_body(content = ConfirmDerivedRequest),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn confirm_derived_scenarios() {}

    #[utoipa::path(post, path = "/api/scenarios/run", operation_id = "run_scenarios", tag = "scenarios",
        request_body(content = ScenarioRunRequest),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn run_scenarios() {}

    #[utoipa::path(get, path = "/api/scenarios/{id}/run", operation_id = "get_scenario_run", tag = "scenarios",
        params(("id" = String, Path, description = "Scenario ID")),
        responses((status = 200, description = "Scenario run", body = ScenarioRun)))]
    pub fn get_scenario_run() {}

    #[utoipa::path(get, path = "/api/findings", operation_id = "list_findings", tag = "findings",
        params(("status" = Option<String>, Query, description = "Status filter"),
               ("severity" = Option<String>, Query, description = "Severity filter"),
               ("code" = Option<String>, Query, description = "Code filter"),
               ("family" = Option<String>, Query, description = "Family filter")),
        responses((status = 200, description = "Findings", body = FindingPage)))]
    pub fn list_findings() {}

    #[utoipa::path(post, path = "/api/findings/{id}/waive", operation_id = "waive_finding", tag = "findings",
        params(("id" = String, Path, description = "Finding ID")),
        request_body(content = ReasonRequest),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn waive_finding() {}

    #[utoipa::path(get, path = "/api/assumptions", operation_id = "list_assumptions", tag = "assumptions",
        responses((status = 200, description = "Assumptions", body = AssumptionPage)))]
    pub fn list_assumptions() {}

    #[utoipa::path(post, path = "/api/assumptions/{id}/resolve", operation_id = "resolve_assumption", tag = "assumptions",
        params(("id" = String, Path, description = "Assumption ID")),
        request_body(content = ReasonRequest),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn resolve_assumption() {}

    #[utoipa::path(get, path = "/api/questions", operation_id = "list_questions", tag = "questions",
        params(("stakeholder" = Option<String>, Query, description = "Stakeholder filter"),
               ("round" = Option<String>, Query, description = "Round filter"),
               ("status" = Option<QuestionStatus>, Query, description = "Status filter")),
        responses((status = 200, description = "Questions", body = QuestionPage)))]
    pub fn list_questions() {}

    #[utoipa::path(post, path = "/api/questions/{id}/answer", operation_id = "answer_question", tag = "questions",
        params(("id" = String, Path, description = "Question ID")),
        request_body(content = AnswerRequest),
        responses((status = 200, description = "Proposal, intake and patch preview before acceptance", body = AnswerResponse)))]
    pub fn answer_question() {}

    #[utoipa::path(post, path = "/api/rounds", operation_id = "create_round", tag = "questions",
        request_body(content = CreateRoundRequest),
        responses((status = 200, description = "The composed round", body = CreateRoundResponse)))]
    pub fn create_round() {}

    #[utoipa::path(get, path = "/api/rounds/{id}", operation_id = "get_round", tag = "questions",
        params(("id" = String, Path, description = "Round ID")),
        responses((status = 200, description = "Round", body = Round)))]
    pub fn get_round() {}

    #[utoipa::path(post, path = "/api/proposals/intake", operation_id = "intake_proposal", tag = "proposals",
        request_body(content = IntakeRequest),
        responses((status = 200, description = "Dry-run intake report against the current baseline", body = IntakeResponse)))]
    pub fn intake_proposal() {}

    #[utoipa::path(post, path = "/api/proposals/{id}/link", operation_id = "link_proposal", tag = "proposals",
        params(("id" = String, Path, description = "Proposal ID")),
        request_body(content = ProposalActionRequest),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn link_proposal() {}

    #[utoipa::path(post, path = "/api/proposals/{id}/merge", operation_id = "merge_proposal", tag = "proposals",
        params(("id" = String, Path, description = "Proposal ID")),
        request_body(content = ProposalActionRequest),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn merge_proposal() {}

    #[utoipa::path(post, path = "/api/proposals/{id}/supersede", operation_id = "supersede_proposal", tag = "proposals",
        params(("id" = String, Path, description = "Proposal ID")),
        request_body(content = ProposalActionRequest),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn supersede_proposal() {}

    #[utoipa::path(post, path = "/api/proposals/{id}/create", operation_id = "create_proposal", tag = "proposals",
        params(("id" = String, Path, description = "Proposal ID")),
        request_body(content = ProposalActionRequest),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn create_proposal() {}

    #[utoipa::path(post, path = "/api/proposals/{id}/reject", operation_id = "reject_proposal", tag = "proposals",
        params(("id" = String, Path, description = "Proposal ID")),
        request_body(content = ProposalActionRequest),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn reject_proposal() {}

    #[utoipa::path(post, path = "/api/proposals/{id}/accept", operation_id = "accept_proposal", tag = "proposals",
        params(("id" = String, Path, description = "Proposal ID")),
        request_body(content = ProposalActionRequest),
        responses((status = 200, description = "Write result", body = WriteResult)))]
    pub fn accept_proposal() {}

    #[utoipa::path(post, path = "/api/sessions", operation_id = "create_session", tag = "sessions",
        request_body(content = CreateSessionRequest),
        responses((status = 200, description = "Session created", body = CreateSessionResponse)))]
    pub fn create_session() {}

    #[utoipa::path(post, path = "/api/sessions/{id}/turns", operation_id = "create_session_turn", tag = "sessions",
        params(("id" = String, Path, description = "Session ID")),
        request_body(content = SessionTurnRequest),
        responses((status = 200, description = "Assistant reply, proposal cards and asked question", body = SessionTurnResponse)))]
    pub fn create_session_turn() {}

    #[utoipa::path(get, path = "/api/sessions/{id}", operation_id = "get_session", tag = "sessions",
        params(("id" = String, Path, description = "Session ID")),
        responses((status = 200, description = "Session", body = SessionDetail)))]
    pub fn get_session() {}

    #[utoipa::path(get, path = "/api/sessions/{id}/metrics", operation_id = "get_session_metrics", tag = "sessions",
        params(("id" = String, Path, description = "Session ID")),
        responses((status = 200, description = "Session counts", body = SessionMetrics)))]
    pub fn get_session_metrics() {}

    #[utoipa::path(get, path = "/api/decisions", operation_id = "list_decisions", tag = "decisions",
        params(("by" = Option<String>, Query, description = "Decider filter"),
               ("kind" = Option<String>, Query, description = "Kind filter"),
               ("since" = Option<String>, Query, description = "Since filter")),
        responses((status = 200, description = "Decisions", body = DecisionPage)))]
    pub fn list_decisions() {}

    #[utoipa::path(get, path = "/api/requirements/{id}/readiness", operation_id = "get_requirement_readiness", tag = "readiness",
        params(("id" = String, Path, description = "Requirement ID")),
        responses((status = 200, description = "Advisory readiness; the composite is uncalibrated", body = Readiness)))]
    pub fn get_requirement_readiness() {}

    #[utoipa::path(get, path = "/api/readiness", operation_id = "get_readiness", tag = "readiness",
        params(("group" = Option<ReadinessGroup>, Query, description = "Grouping")),
        responses((status = 200, description = "Advisory readiness; the composite is uncalibrated", body = Readiness)))]
    pub fn get_readiness() {}

    #[utoipa::path(get, path = "/api/model/export", operation_id = "export_model", tag = "export",
        params(("format" = ExportFormat, Query, description = "Export format")),
        responses((status = 200, description = "Exported file", body = BinaryFile, content_type = "application/octet-stream")))]
    pub fn export_model() {}

    #[utoipa::path(get, path = "/api/model/diff", operation_id = "get_model_diff", tag = "export",
        params(("from" = Option<String>, Query, description = "From revision"),
               ("to" = Option<String>, Query, description = "To revision")),
        responses((status = 200, description = "Structured diff", body = ModelDiff)))]
    pub fn get_model_diff() {}

    #[utoipa::path(post, path = "/api/requirements/{id}/split/preview", operation_id = "preview_requirement_split", tag = "requirements",
        params(("id" = String, Path, description = "Requirement ID")),
        request_body(content = SplitRequest),
        responses((status = 200, description = "Pending create-class split proposal, intake and semantic element diff; nothing is committed", body = SemanticChangePreviewResponse)))]
    pub fn preview_requirement_split() {}

    #[utoipa::path(post, path = "/api/glossary/{id}/merge/preview", operation_id = "preview_glossary_merge", tag = "glossary",
        params(("id" = String, Path, description = "Term ID")),
        request_body(content = GlossaryMergeRequest),
        responses((status = 200, description = "Pending merge proposal, intake and semantic element diff; nothing is committed", body = SemanticChangePreviewResponse)))]
    pub fn preview_glossary_merge() {}
}

/// The utoipa base contract of the 74 operations.
#[derive(OpenApi)]
#[openapi(
    info(title = "Plumb Modeller API"),
    paths(
        stubs::get_me, stubs::get_model_version, stubs::get_gates, stubs::search_model,
        stubs::events_websocket, stubs::create_source, stubs::get_source, stubs::get_source_spans,
        stubs::run_stage, stubs::get_job, stubs::list_requirements, stubs::get_requirement,
        stubs::classify_requirement, stubs::accept_requirement_ears, stubs::split_requirement,
        stubs::waive_requirement_finding, stubs::mark_requirement_duplicate, stubs::list_glossary,
        stubs::confirm_glossary, stubs::edit_glossary, stubs::merge_glossary, stubs::list_sentences,
        stubs::accept_sentence, stubs::edit_sentence, stubs::reject_sentence, stubs::list_entities,
        stubs::get_entity, stubs::get_crud_matrix, stubs::set_attribute_type,
        stubs::set_attribute_unit, stubs::set_attribute_class, stubs::list_calculations,
        stubs::get_calculation_example, stubs::confirm_calculation, stubs::list_rules,
        stubs::get_rule, stubs::set_rule_cell, stubs::list_processes, stubs::get_process,
        stubs::mutate_process_steps, stubs::import_process, stubs::export_process,
        stubs::list_scenarios, stubs::confirm_scenario, stubs::correct_scenario,
        stubs::confirm_derived_scenarios, stubs::run_scenarios, stubs::get_scenario_run,
        stubs::list_findings, stubs::waive_finding, stubs::list_assumptions,
        stubs::resolve_assumption, stubs::list_questions, stubs::answer_question,
        stubs::create_round, stubs::get_round, stubs::intake_proposal, stubs::link_proposal,
        stubs::merge_proposal, stubs::supersede_proposal, stubs::create_proposal,
        stubs::reject_proposal, stubs::accept_proposal, stubs::create_session,
        stubs::create_session_turn, stubs::get_session, stubs::get_session_metrics,
        stubs::list_decisions, stubs::get_requirement_readiness, stubs::get_readiness,
        stubs::export_model, stubs::get_model_diff, stubs::preview_requirement_split,
        stubs::preview_glossary_merge,
    ),
    components(schemas(
        DesignEvent, PreconditionFailedResponse, CursorConflictResponse, CursorErrorCode, StageName,
        ProcessFormat, ExportFormat, ReadinessGroup, SearchScope,
    )),
    tags(
        (name = "meta"), (name = "sources"), (name = "stages"), (name = "requirements"),
        (name = "glossary"), (name = "sentences"), (name = "entities"), (name = "calculations"),
        (name = "rules"), (name = "processes"), (name = "scenarios"), (name = "findings"),
        (name = "assumptions"), (name = "questions"), (name = "proposals"), (name = "sessions"),
        (name = "decisions"), (name = "readiness"), (name = "export"),
    )
)]
pub struct ApiDoc;

// ============================================================================ fixture seeds

/// A semantic family whose examples are seeded from the HR fixture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SeedFamily {
    Requirement,
    Entity,
    Calculation,
    Rule,
    Process,
    Operation,
}

/// The lexicographically smallest fixture object of one family and its frozen seed fields.
#[derive(Debug, Clone, PartialEq)]
pub struct FixtureSeed {
    pub family: SeedFamily,
    pub id: String,
    pub fields: Map<String, Value>,
}

fn seed_error(what: &str) -> OpenApiContractError {
    OpenApiContractError::InvalidFixtureSeed(what.to_owned())
}

fn smallest_key<'a>(
    map: &'a Map<String, Value>,
    what: &str,
) -> Result<(&'a String, &'a Value), OpenApiContractError> {
    map.iter()
        .min_by(|a, b| a.0.cmp(b.0))
        .ok_or_else(|| seed_error(&format!("{what} is empty")))
}

fn text(value: &Value, what: &str) -> Result<Value, OpenApiContractError> {
    value
        .as_str()
        .map(|s| Value::String(s.to_owned()))
        .ok_or_else(|| seed_error(&format!("{what} is not a string")))
}

/// Parses the embedded HR fixture contract and selects one seed per family, sorting candidates
/// explicitly.
pub fn fixture_seeds() -> Result<Vec<FixtureSeed>, OpenApiContractError> {
    let fixture: Value = serde_yaml::from_str(FIXTURE_CONTRACT)
        .map_err(|e| OpenApiContractError::FixtureParse(e.to_string()))?;
    let section = |key: &str| {
        fixture
            .get(key)
            .ok_or_else(|| OpenApiContractError::FixtureParse(format!("missing section {key}")))
    };
    let object = |key: &str| {
        section(key)?
            .as_object()
            .ok_or_else(|| OpenApiContractError::FixtureParse(format!("{key} is not a mapping")))
    };
    let mut seeds = Vec::new();

    let requirements = section("requirements")?
        .as_array()
        .ok_or_else(|| OpenApiContractError::FixtureParse("requirements is not a list".into()))?;
    let requirement = requirements
        .iter()
        .min_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()))
        .ok_or_else(|| seed_error("requirements is empty"))?;
    let id = text(&requirement["id"], "requirement id")?;
    seeds.push(FixtureSeed {
        family: SeedFamily::Requirement,
        id: id.as_str().unwrap_or_default().to_owned(),
        fields: Map::from_iter([
            ("id".to_owned(), id),
            (
                "text".to_owned(),
                text(&requirement["statement"], "requirement statement")?,
            ),
            (
                "class".to_owned(),
                text(&requirement["kind"], "requirement kind")?,
            ),
        ]),
    });

    let entities = section("entities")?
        .as_array()
        .ok_or_else(|| OpenApiContractError::FixtureParse("entities is not a list".into()))?;
    let mut names = entities
        .iter()
        .map(|e| {
            e.as_str()
                .map(str::to_owned)
                .ok_or_else(|| seed_error("entity is not a string"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    names.sort();
    let entity = names
        .first()
        .ok_or_else(|| seed_error("entities is empty"))?;
    seeds.push(FixtureSeed {
        family: SeedFamily::Entity,
        id: entity.clone(),
        fields: Map::from_iter([
            ("id".to_owned(), json!(entity)),
            ("name".to_owned(), json!(entity)),
        ]),
    });

    let (key, expression) = smallest_key(object("calculations")?, "calculations")?;
    seeds.push(FixtureSeed {
        family: SeedFamily::Calculation,
        id: key.clone(),
        fields: Map::from_iter([
            ("id".to_owned(), json!(key)),
            ("name".to_owned(), json!(key)),
            (
                "expr".to_owned(),
                text(expression, "calculation expression")?,
            ),
        ]),
    });

    let (key, rule_text) = smallest_key(object("rules")?, "rules")?;
    seeds.push(FixtureSeed {
        family: SeedFamily::Rule,
        id: key.clone(),
        fields: Map::from_iter([
            ("id".to_owned(), json!(key)),
            ("name".to_owned(), json!(key)),
            ("text".to_owned(), text(rule_text, "rule text")?),
        ]),
    });

    let (key, steps) = smallest_key(object("process_specs")?, "process_specs")?;
    let steps = steps
        .as_array()
        .filter(|s| s.iter().all(Value::is_string))
        .ok_or_else(|| seed_error("process steps are not a list of operation names"))?;
    seeds.push(FixtureSeed {
        family: SeedFamily::Process,
        id: key.clone(),
        fields: Map::from_iter([
            ("id".to_owned(), json!(key)),
            ("name".to_owned(), json!(key)),
            ("steps".to_owned(), Value::Array(steps.clone())),
        ]),
    });

    let (key, spec) = smallest_key(object("operation_specs")?, "operation_specs")?;
    let spec = spec
        .as_object()
        .ok_or_else(|| seed_error("operation spec is not a mapping"))?;
    let mut fields = Map::from_iter([
        ("id".to_owned(), json!(key)),
        ("name".to_owned(), json!(key)),
    ]);
    for field in ["kind", "performer", "reads", "writes", "outcomes", "events"] {
        if let Some(value) = spec.get(field) {
            fields.insert(field.to_owned(), value.clone());
        }
    }
    seeds.push(FixtureSeed {
        family: SeedFamily::Operation,
        id: key.clone(),
        fields,
    });
    Ok(seeds)
}

/// The family whose seed qualifies for a component schema.
fn component_family(name: &str) -> Option<SeedFamily> {
    match name {
        "RequirementSummary" | "RequirementDetail" => Some(SeedFamily::Requirement),
        "Entity" => Some(SeedFamily::Entity),
        "Calculation" | "CalculationExample" => Some(SeedFamily::Calculation),
        "Rule" => Some(SeedFamily::Rule),
        "Process" => Some(SeedFamily::Process),
        _ => None,
    }
}

/// The family whose seed ID qualifies for the `{id}` path parameter of `path`.
fn path_family(path: &str) -> Option<SeedFamily> {
    if path.starts_with("/api/requirements/{id}") {
        Some(SeedFamily::Requirement)
    } else if path.starts_with("/api/model/entities/{id}") {
        Some(SeedFamily::Entity)
    } else if path.starts_with("/api/model/calculations/{id}") {
        Some(SeedFamily::Calculation)
    } else if path.starts_with("/api/model/rules/{id}") {
        Some(SeedFamily::Rule)
    } else if path.starts_with("/api/processes/{id}") {
        Some(SeedFamily::Process)
    } else {
        None
    }
}

// ============================================================================ example generation

/// The deterministic example generator: fixture seeds for qualified semantic components and
/// the frozen schema fallback for everything else.
pub struct ExampleGenerator<'a> {
    schemas: &'a Map<String, Value>,
    seeds: &'a [FixtureSeed],
}

fn unsupported(location: &str, construct: impl Into<String>) -> OpenApiContractError {
    OpenApiContractError::UnsupportedFallback {
        location: location.to_owned(),
        construct: construct.into(),
    }
}

fn canonical(value: &Value) -> Result<Vec<u8>, OpenApiContractError> {
    to_canonical_json(value).map_err(|e| OpenApiContractError::Serialization(e.to_string()))
}

fn is_null_schema(schema: &Value) -> bool {
    schema.get("type") == Some(&json!("null"))
}

const UNSUPPORTED_KEYWORDS: [&str; 9] = [
    "allOf",
    "not",
    "if",
    "then",
    "else",
    "dependentSchemas",
    "patternProperties",
    "const",
    "$defs",
];

impl<'a> ExampleGenerator<'a> {
    pub fn new(schemas: &'a Map<String, Value>, seeds: &'a [FixtureSeed]) -> ExampleGenerator<'a> {
        ExampleGenerator { schemas, seeds }
    }

    fn seed(&self, family: SeedFamily) -> Option<&'a FixtureSeed> {
        self.seeds.iter().find(|s| s.family == family)
    }

    /// Generates the example of `schema` at `location`.
    pub fn example(&self, schema: &Value, location: &str) -> Result<Value, OpenApiContractError> {
        self.generate(schema, None, location, &mut Vec::new())
    }

    fn generate(
        &self,
        schema: &Value,
        seed: Option<&FixtureSeed>,
        location: &str,
        visiting: &mut Vec<String>,
    ) -> Result<Value, OpenApiContractError> {
        let Some(object) = schema.as_object() else {
            return Err(unsupported(location, format!("non-object schema {schema}")));
        };
        if let Some(reference) = object.get("$ref") {
            let name = reference
                .as_str()
                .and_then(|r| r.strip_prefix(COMPONENT_PREFIX))
                .ok_or_else(|| unsupported(location, format!("non-local $ref {reference}")))?;
            if visiting.iter().any(|v| v == name) {
                return Err(unsupported(
                    location,
                    format!("recursive $ref cycle through {name}"),
                ));
            }
            let target = self.schemas.get(name).ok_or_else(|| {
                OpenApiContractError::MissingSchema(format!("{name} at {location}"))
            })?;
            let seed = component_family(name).and_then(|f| self.seed(f));
            visiting.push(name.to_owned());
            let generated = self.generate(target, seed, &format!("{location} -> {name}"), visiting);
            visiting.pop();
            return generated;
        }
        if let Some(keyword) = UNSUPPORTED_KEYWORDS
            .iter()
            .find(|k| object.contains_key(**k))
        {
            return Err(unsupported(location, *keyword));
        }
        if let Some(values) = object.get("enum") {
            let values = values
                .as_array()
                .filter(|v| !v.is_empty())
                .ok_or_else(|| unsupported(location, "empty or malformed enum"))?;
            let mut candidates = Vec::new();
            for value in values {
                let wire = serde_json::to_string(value)
                    .map_err(|e| OpenApiContractError::Serialization(e.to_string()))?;
                candidates.push((wire, value));
            }
            candidates.sort_by(|a, b| a.0.cmp(&b.0));
            return Ok(candidates[0].1.clone());
        }
        for keyword in ["oneOf", "anyOf"] {
            if let Some(branches) = object.get(keyword) {
                let branches = branches
                    .as_array()
                    .ok_or_else(|| unsupported(location, format!("malformed {keyword}")))?;
                let mut candidates = Vec::new();
                for branch in branches.iter().filter(|b| !is_null_schema(b)) {
                    candidates.push((canonical(branch)?, branch));
                }
                candidates.sort_by(|a, b| a.0.cmp(&b.0));
                let (_, branch) = candidates.first().ok_or_else(|| {
                    unsupported(location, format!("{keyword} without a non-null branch"))
                })?;
                return self.generate(branch, seed, &format!("{location} {keyword}"), visiting);
            }
        }
        let kind: &str = match object.get("type") {
            Some(Value::String(kind)) => kind.as_str(),
            Some(Value::Array(kinds)) => {
                let mut non_null = kinds
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|k| *k != "null");
                match (non_null.next(), non_null.next()) {
                    (Some(kind), None) => kind,
                    _ => return Err(unsupported(location, format!("type union {kinds:?}"))),
                }
            }
            _ => return Err(unsupported(location, "schema without type")),
        };
        match kind {
            "boolean" => Ok(Value::Bool(true)),
            "integer" => Ok(object.get("minimum").cloned().unwrap_or(json!(1))),
            "number" => {
                let minimum = match object.get("minimum") {
                    Some(m) => m
                        .as_f64()
                        .ok_or_else(|| unsupported(location, format!("number minimum {m}")))?,
                    None => 1.0,
                };
                Ok(json!(minimum))
            }
            "string" => Ok(match object.get("format").and_then(Value::as_str) {
                Some("date") => json!(FALLBACK_DATE),
                Some("date-time") => json!(FALLBACK_DATE_TIME),
                _ => json!(FALLBACK_STRING),
            }),
            "array" => {
                let items = object
                    .get("items")
                    .ok_or_else(|| unsupported(location, "array without items"))?;
                Ok(Value::Array(vec![self.generate(
                    items,
                    None,
                    &format!("{location}[]"),
                    visiting,
                )?]))
            }
            "object" => self.object(object, seed, location, visiting),
            other => Err(unsupported(location, format!("type {other}"))),
        }
    }

    fn object(
        &self,
        object: &Map<String, Value>,
        seed: Option<&FixtureSeed>,
        location: &str,
        visiting: &mut Vec<String>,
    ) -> Result<Value, OpenApiContractError> {
        let empty = Map::new();
        let properties = object
            .get("properties")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        let mut generated = Map::new();
        if let Some(seed) = seed {
            for (field, value) in &seed.fields {
                let Some(property) = properties.get(field) else {
                    continue;
                };
                check_seed_type(property, value, &format!("{location}.{field}"))?;
                generated.insert(field.clone(), value.clone());
            }
        }
        let mut required: Vec<&str> = object
            .get("required")
            .and_then(Value::as_array)
            .map(|r| r.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        required.sort();
        for name in required {
            if generated.contains_key(name) {
                continue;
            }
            let property = properties.get(name).ok_or_else(|| {
                OpenApiContractError::MissingSchema(format!(
                    "required property {name} at {location}"
                ))
            })?;
            let value = self.generate(property, None, &format!("{location}.{name}"), visiting)?;
            generated.insert(name.to_owned(), value);
        }
        Ok(Value::Object(generated))
    }
}

/// A seed value must have a JSON type the target property declares; full validation is done by
/// the contract tests.
fn check_seed_type(
    property: &Value,
    value: &Value,
    location: &str,
) -> Result<(), OpenApiContractError> {
    let declared: Vec<&str> = match property.get("type") {
        Some(Value::String(kind)) => vec![kind.as_str()],
        Some(Value::Array(kinds)) => kinds.iter().filter_map(Value::as_str).collect(),
        _ => return Ok(()),
    };
    let actual = match value {
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::Null => "null",
    };
    if declared.contains(&actual) || (actual == "integer" && declared.contains(&"number")) {
        Ok(())
    } else {
        Err(OpenApiContractError::InvalidFixtureSeed(format!(
            "BLOCKED-TEST-CONTRACT: {location} expects {declared:?}, fixture has {actual}"
        )))
    }
}

// ============================================================================ document

fn invariant(message: impl Into<String>) -> OpenApiContractError {
    OpenApiContractError::ContractInvariant(message.into())
}

fn component_ref(name: &str) -> Value {
    json!({ "$ref": format!("{COMPONENT_PREFIX}{name}") })
}

/// Sorts every object's keys; arrays keep their order.
fn sort_keys(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(
                entries
                    .into_iter()
                    .map(|(k, v)| (k, sort_keys(v)))
                    .collect(),
            )
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sort_keys).collect()),
        other => other,
    }
}

fn operation_mut<'v>(
    doc: &'v mut Value,
    spec: &OperationSpec,
) -> Result<&'v mut Map<String, Value>, OpenApiContractError> {
    doc.get_mut("paths")
        .and_then(|p| p.get_mut(spec.path))
        .and_then(|p| p.get_mut(spec.method.to_ascii_lowercase()))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| invariant(format!("missing operation {} {}", spec.method, spec.path)))
}

/// Checks that the utoipa paths are exactly the frozen inventory.
fn check_inventory(doc: &Value) -> Result<(), OpenApiContractError> {
    let paths = doc
        .get("paths")
        .and_then(Value::as_object)
        .ok_or_else(|| invariant("document has no paths"))?;
    let mut found = Vec::new();
    for (path, item) in paths {
        let item = item
            .as_object()
            .ok_or_else(|| invariant(format!("path {path} is not an object")))?;
        for (method, operation) in item {
            let operation_id = operation
                .get("operationId")
                .and_then(Value::as_str)
                .ok_or_else(|| invariant(format!("{method} {path} has no operationId")))?;
            found.push((
                method.to_ascii_uppercase(),
                path.clone(),
                operation_id.to_owned(),
            ));
        }
    }
    found.sort();
    let mut expected: Vec<(String, String, String)> = OPERATIONS
        .iter()
        .map(|s| {
            (
                s.method.to_owned(),
                s.path.to_owned(),
                s.operation_id.to_owned(),
            )
        })
        .collect();
    expected.sort();
    if found != expected {
        return Err(invariant(
            "operations differ from the frozen 74-operation inventory",
        ));
    }
    Ok(())
}

/// Adds If-Match and 412 to every POST, ETag to every ordinary GET 2xx response, and the
/// cursor/limit parameters and 409 response to every pageable operation.
fn apply_http_contract(doc: &mut Value) -> Result<(), OpenApiContractError> {
    for spec in &OPERATIONS {
        let operation = operation_mut(doc, spec)?;
        if spec.method == "POST" {
            let parameters = operation
                .entry("parameters")
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .ok_or_else(|| invariant("parameters is not an array"))?;
            parameters.push(json!({
                "name": "If-Match",
                "in": "header",
                "required": true,
                "description": "The current semantic_hash of the branch head.",
                "schema": {"type": "string"}
            }));
            responses_mut(operation)?.insert(
                "412".to_owned(),
                json!({
                    "description": "Precondition Failed: If-Match is not the current semantic_hash; revision and semantic_hash are the current branch-head values.",
                    "content": {"application/json": {"schema": component_ref("PreconditionFailedResponse")}}
                }),
            );
        } else if spec.operation_id != EVENTS_OPERATION_ID {
            for (status, response) in responses_mut(operation)?.iter_mut() {
                if status.starts_with('2') {
                    let response = response
                        .as_object_mut()
                        .ok_or_else(|| invariant("response is not an object"))?;
                    response
                        .entry("headers")
                        .or_insert_with(|| json!({}))
                        .as_object_mut()
                        .ok_or_else(|| invariant("headers is not an object"))?
                        .insert(
                            "ETag".to_owned(),
                            json!({
                                "description": "The current semantic_hash.",
                                "schema": {"type": "string"}
                            }),
                        );
                }
            }
        }
        if spec.pageable {
            let parameters = operation
                .entry("parameters")
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .ok_or_else(|| invariant("parameters is not an array"))?;
            parameters.push(json!({
                "name": "cursor",
                "in": "query",
                "required": false,
                "description": "Opaque cursor from next_cursor (cursor v2): lowercase hex of the UTF-8 RFC 8785 JSON {\"after_id\":\"<last-returned-id>\",\"request_hash\":\"sha256:...\",\"revision\":\"rev:...\",\"semantic_hash\":\"psg:sha256:...\"} with exactly these four fields. revision is the branch-head RevisionId of the page; request_hash is the generic SHA-256 of the RFC 8785 JSON {operation_id, path, query} of the request (all path parameters, only present query parameters other than cursor and limit). A malformed or non-canonical cursor is 400 E_CURSOR_INVALID and a cursor of another request is 400 E_CURSOR_REQUEST_MISMATCH.",
                "schema": {"type": "string"}
            }));
            parameters.push(json!({
                "name": "limit",
                "in": "query",
                "required": false,
                "description": "Maximum number of items (at least 1). Absent: all remaining items and next_cursor null.",
                "schema": {"type": "integer", "minimum": 1}
            }));
            responses_mut(operation)?.insert(
                "409".to_owned(),
                json!({
                    "description": "Conflict: the cursor was issued at another branch-head revision or semantic_hash (E_CURSOR_BASELINE_CHANGED).",
                    "content": {"application/json": {"schema": component_ref("CursorConflictResponse")}}
                }),
            );
        }
    }
    Ok(())
}

/// The internally tagged request/response unions whose serde form rejects unknown fields
/// (Hotfix 065); utoipa cannot express that per variant.
pub const CLOSED_TAGGED_UNIONS: [&str; 2] = ["CalculationValue", "ProcessStepRequest"];

/// Closes every `oneOf` branch of [`CLOSED_TAGGED_UNIONS`] with `additionalProperties: false`.
fn close_tagged_unions(doc: &mut Value) -> Result<(), OpenApiContractError> {
    for name in CLOSED_TAGGED_UNIONS {
        let branches = doc
            .pointer_mut(&format!("/components/schemas/{name}/oneOf"))
            .and_then(Value::as_array_mut)
            .ok_or_else(|| invariant(format!("{name} is not a oneOf union")))?;
        for branch in branches {
            branch
                .as_object_mut()
                .filter(|b| b.get("type") == Some(&json!("object")))
                .ok_or_else(|| invariant(format!("{name} has a non-object branch")))?
                .insert("additionalProperties".to_owned(), json!(false));
        }
    }
    Ok(())
}

fn responses_mut(
    operation: &mut Map<String, Value>,
) -> Result<&mut Map<String, Value>, OpenApiContractError> {
    operation
        .get_mut("responses")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| invariant("operation has no responses"))
}

/// Adds the WebSocket and gate-order compatibility extensions.
fn apply_extensions(doc: &mut Value, generator_doc: &Value) -> Result<(), OpenApiContractError> {
    let schemas = schemas_of(generator_doc)?;
    let seeds = fixture_seeds()?;
    let generator = ExampleGenerator::new(schemas, &seeds);
    let message_schema = component_ref("DesignEvent");
    let message_example = generator.example(&message_schema, "events_websocket message")?;
    for spec in &OPERATIONS {
        if spec.operation_id == EVENTS_OPERATION_ID {
            operation_mut(doc, spec)?.insert(
                "x-plumb-websocket".to_owned(),
                json!({
                    "direction": "server",
                    "message_schema": message_schema,
                    "message_example": message_example,
                }),
            );
        }
        if spec.operation_id == "get_gates" {
            operation_mut(doc, spec)?.insert("x-plumb-gate-order".to_owned(), json!(NOW_GATES));
        }
    }
    let root = doc
        .as_object_mut()
        .ok_or_else(|| invariant("document is not an object"))?;
    root.insert(
        "x-plumb-contract-version".to_owned(),
        json!(CONTRACT_VERSION),
    );
    Ok(())
}

fn schemas_of(doc: &Value) -> Result<&Map<String, Value>, OpenApiContractError> {
    doc.pointer("/components/schemas")
        .and_then(Value::as_object)
        .ok_or_else(|| invariant("document has no component schemas"))
}

/// Attaches one generated example to every parameter, request body media type, response media
/// type and response header that has a schema.
fn apply_examples(doc: &mut Value) -> Result<(), OpenApiContractError> {
    let schemas = schemas_of(doc)?.clone();
    let seeds = fixture_seeds()?;
    let generator = ExampleGenerator::new(&schemas, &seeds);
    for spec in &OPERATIONS {
        let operation = operation_mut(doc, spec)?;
        let at = |part: &str| format!("{} {}", spec.operation_id, part);
        if let Some(parameters) = operation
            .get_mut("parameters")
            .and_then(Value::as_array_mut)
        {
            for parameter in parameters {
                let name = parameter["name"].as_str().unwrap_or_default().to_owned();
                let location = parameter["in"].as_str().unwrap_or_default().to_owned();
                let Some(schema) = parameter.get("schema").cloned() else {
                    continue;
                };
                let seeded = (location == "path" && name == "id")
                    .then(|| path_family(spec.path))
                    .flatten()
                    .and_then(|family| seeds.iter().find(|s| s.family == family));
                let example = match seeded {
                    Some(seed) => json!(seed.id),
                    None => {
                        generator.example(&schema, &at(&format!("{location} parameter {name}")))?
                    }
                };
                parameter
                    .as_object_mut()
                    .ok_or_else(|| invariant("parameter is not an object"))?
                    .insert("example".to_owned(), example);
            }
        }
        if let Some(content) = operation
            .get_mut("requestBody")
            .and_then(|b| b.get_mut("content"))
            .and_then(Value::as_object_mut)
        {
            add_media_examples(&generator, content, &at("request body"))?;
        }
        for (status, response) in responses_mut(operation)?.iter_mut() {
            if let Some(content) = response.get_mut("content").and_then(Value::as_object_mut) {
                add_media_examples(&generator, content, &at(&format!("response {status}")))?;
            }
            if let Some(headers) = response.get_mut("headers").and_then(Value::as_object_mut) {
                add_media_examples(
                    &generator,
                    headers,
                    &at(&format!("response {status} header")),
                )?;
            }
        }
    }
    Ok(())
}

/// Adds an example to each schema-bearing entry of a content or headers map.
fn add_media_examples(
    generator: &ExampleGenerator<'_>,
    content: &mut Map<String, Value>,
    location: &str,
) -> Result<(), OpenApiContractError> {
    for (media_type, media) in content.iter_mut() {
        let Some(schema) = media.get("schema").cloned() else {
            continue;
        };
        let example = generator.example(&schema, &format!("{location} {media_type}"))?;
        media
            .as_object_mut()
            .ok_or_else(|| invariant("media type is not an object"))?
            .insert("example".to_owned(), example);
    }
    Ok(())
}

/// The complete pilot OpenAPI document: the utoipa base contract with the compatibility
/// patches, extensions and examples, object keys sorted.
pub fn openapi_document() -> Result<Value, OpenApiContractError> {
    let mut api = ApiDoc::openapi();
    api.info.version = env!("CARGO_PKG_VERSION").to_owned();
    api.info.description = Some(
        "Plumb pilot HTTP API contract (A0.1): a task-oriented compatibility projection over the PSG. Runtime handlers are provided by A0.2, A0.3 and A0.4 (operations 1-72) and U2.1 (the two semantic change previews, operations 73-74)."
            .to_owned(),
    );
    api.info.license = None;
    api.servers = None;
    let mut doc = serde_json::to_value(&api)
        .map_err(|e| OpenApiContractError::Serialization(e.to_string()))?;
    check_inventory(&doc)?;
    apply_http_contract(&mut doc)?;
    close_tagged_unions(&mut doc)?;
    let base = doc.clone();
    apply_extensions(&mut doc, &base)?;
    apply_examples(&mut doc)?;
    Ok(sort_keys(doc))
}

/// The rendered `api/modeller.openapi.json`: pretty JSON with one trailing newline.
pub fn render_openapi_json() -> Result<String, OpenApiContractError> {
    let doc = openapi_document()?;
    let mut text = serde_json::to_string_pretty(&doc)
        .map_err(|e| OpenApiContractError::Serialization(e.to_string()))?;
    text.push('\n');
    Ok(text)
}

/// The rendered `docs/api.md`, generated from the same frozen inventory.
pub fn render_api_markdown() -> Result<String, OpenApiContractError> {
    let doc = openapi_document()?;
    let version = doc
        .pointer("/info/version")
        .and_then(Value::as_str)
        .ok_or_else(|| invariant("document has no info.version"))?;
    let mut out = String::new();
    out.push_str("# Plumb Pilot API\n\n");
    out.push_str("> Generated from the Rust OpenAPI contract. Do not edit by hand.\n\n");
    out.push_str(&format!(
        "Contract version {CONTRACT_VERSION}, API version {version}, {} operations. \
The machine-readable contract is `api/modeller.openapi.json`, the source of the UI client.\n\n",
        OPERATIONS.len()
    ));
    out.push_str(
        "A0.1 defines the pilot contract only; A0.2, A0.3 and A0.4 provide the runtime handlers of operations 1-72 and U2.1 those of operations 73-74. \
An operation listed here is not operational until its handler task is done.\n\n",
    );
    out.push_str("## Conventions\n\n");
    for line in [
        "The API is a task-oriented compatibility projection over the PSG. Requests and responses are JSON unless stated otherwise; errors carry `code`, `message` and `details`.",
        "Every ordinary `GET` success response carries an `ETag` header equal to the current `semantic_hash`.",
        "Every `POST` (including dry-run intake, stage runs and session creation and turns) requires an `If-Match` header with the current `semantic_hash`. A mismatch returns `412 Precondition Failed` with `code`, `message`, `details` and the current branch-head `revision` and `semantic_hash`.",
        "Every successful write returns `version`, `revision`, `semantic_hash` and `events` (DesignEvent `{kind, ids, version}`); `version` is the compatibility alias of `revision`. Endpoint-specific results add fields to these four.",
        "`GET /api/model/version` returns `version`, `hash`, `revision`, `semantic_hash` and `evidence_hash`, where `version` == `revision` and `hash` == `semantic_hash`.",
        "Paged lists return `{items, next_cursor}` and accept `cursor` and `limit` (minimum 1). Items are filtered, sorted by stable public ID ascending, continued exclusively after the cursor's `after_id` and then limited; without `limit` all remaining items are returned and `next_cursor` is null.",
        "Cursor v2: a non-null cursor is the lowercase hex of the UTF-8 RFC 8785 JSON `{\"after_id\":\"<last-returned-id>\",\"request_hash\":\"sha256:...\",\"revision\":\"rev:...\",\"semantic_hash\":\"psg:sha256:...\"}`. `revision` is the branch-head revision of the page: `semantic_hash` is not a revision identity, and evidence-only revisions can change page content while keeping it. `request_hash` is the generic SHA-256 of the RFC 8785 JSON `{operation_id, path, query}` (all path parameters; only the present query parameters other than `cursor` and `limit`). Another revision or semantic hash returns `409` `E_CURSOR_BASELINE_CHANGED`; another request returns `400` `E_CURSOR_REQUEST_MISMATCH`; a malformed or non-canonical cursor returns `400` `E_CURSOR_INVALID`.",
        "`/api/events` is a WebSocket upgrade endpoint; the OpenAPI GET operation exists for compatibility/documentation and must not be treated as an ordinary JSON GET call. Server messages are DesignEvent (`x-plumb-websocket`).",
        "`GET /api/gates` reports the NOW gates I0, F1, F2, F3 and F4 in this order (`x-plumb-gate-order`); later gates are not in the NOW API.",
        "Stage jobs report `status` (queued, acquiring, evaluating, ready, stale, failed, committed) and the `base_revision` they captured; `stale` is a status, and stale output is never committed directly.",
        "Readiness `overall` and the sub-scores clarity, completeness, consistency, verifiability and confirmation are nullable and advisory: the composite is uncalibrated and non-authoritative in the pilot. `null` means not calculated by the current pilot runtime, not zero; readiness never influences a gate.",
        "`POST /api/requirements/{id}/accept-ears` requires `text`: the exact EARS-normalized statement the caller reviewed. EARS suggestions are not graph state and are never rediscovered by the server.",
        "Examples come from `fixtures/hr-leave/fixture-contract.yaml` (requirement HR-001, entity ApprovalRecord, calculation AnnualBalanceDelta, rule AnnualLeaveEligibility, process CancelApprovedLeave) or from the deterministic schema fallback.",
    ] {
        out.push_str(&format!("- {line}\n"));
    }
    out.push_str("\n## Pilot restrictions and exclusions\n\n");
    for line in [
        "D test endpoints are not in the pilot API.",
        "POST calculation edit is not in NOW P-R scope; calculation examples are read-only.",
        "Process branch mutation is not in NOW P-R scope; step operations are add, move and remove.",
        "Process SVG export is not in NOW P-R scope; process import and export use mermaid only.",
        "The additional generic v3 engineering endpoints are introduced gradually and are not part of A0.1 except proposal accept.",
    ] {
        out.push_str(&format!("- {line}\n"));
    }
    let owned = |owner: &str, method: &str| {
        OPERATIONS
            .iter()
            .filter(|s| s.owner == owner && (method.is_empty() || s.method == method))
            .count()
    };
    out.push_str("\n## Implementation ownership\n\n");
    for owner in ["A0.2", "A0.3", "A0.4", "U2.1"] {
        out.push_str(&format!(
            "- {owner}: {} operations ({} POST, {} GET).\n",
            owned(owner, ""),
            owned(owner, "POST"),
            owned(owner, "GET")
        ));
    }
    for line in [
        "Hotfix 065 froze the deferred A0.3 compatibility semantics (contract version 3).",
        "A0.4 owns `run_stage` and `get_job` and must not start before its separate stage-orchestration contract freeze.",
        "Hotfix 073 appends operations 73 and 74, the U2.1 semantic change previews: the server builds the exact domain Proposal, persists it pending on the baseline revision and returns its semantic element diff without committing; `candidate_semantic_hash` is not a prediction of the committed `semantic_hash`. Confirmation is the existing proposal action: create for a requirement split (Hotfix 074), merge for a Term merge.",
    ] {
        out.push_str(&format!("- {line}\n"));
    }
    out.push_str("\n## Operations\n\n| # | Method | Path | operationId | Tag | Owner |\n|---|---|---|---|---|---|\n");
    for spec in &OPERATIONS {
        out.push_str(&format!(
            "| {} | {} | `{}` | `{}` | {} | {} |\n",
            spec.number, spec.method, spec.path, spec.operation_id, spec.tag, spec.owner
        ));
    }
    Ok(out)
}
