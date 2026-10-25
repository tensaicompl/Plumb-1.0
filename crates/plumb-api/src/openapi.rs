//! The pilot HTTP API contract (plan A0.1, Hotfix 062): compatibility DTOs, contract-only
//! utoipa operation stubs for the 84 NOW operations (the 72 of Hotfix 062, the two U2.1
//! semantic change previews of Hotfix 073, the two H0.11 proposal-governance operations, the
//! three B1 Inbox operations and the five B2 UI pipeline operations),
//! deterministic compatibility patches,
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
    /// `run_stage` and `get_job` to A0.4; Hotfix 073 adds the two U2.1 previews; H0.11 adds
    /// proposal listing and discard).
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

/// The 84 NOW operations in their frozen order: the v2 P and P-R operations plus proposal
/// accept (01-72, Hotfix 062), the two U2.1 semantic change previews (73-74, Hotfix 073),
/// proposal discovery and discard (75-76, H0.11, architecture §33.12), the B1 Inbox proposal
/// detail, set acceptance and governed confirmation (77-79), then the B2 UI pipeline operations
/// (80-84: pipeline state, job retry, question generation, finding materialization and the
/// source list).
pub const OPERATIONS: [OperationSpec; 84] = [
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
    op(
        75,
        "GET",
        "/api/proposals",
        "list_proposals",
        "proposals",
        false,
        "H0.11",
    ),
    op(
        76,
        "POST",
        "/api/proposals/{id}/discard",
        "discard_proposal",
        "proposals",
        false,
        "H0.11",
    ),
    // B1 (UI Inbox, UI-SPEC §5.2): proposal detail, atomic set acceptance and confirmation.
    op(
        77,
        "GET",
        "/api/proposals/{id}",
        "get_proposal",
        "proposals",
        false,
        "B1",
    ),
    op(
        78,
        "POST",
        "/api/proposals/accept-set",
        "accept_proposal_set",
        "proposals",
        false,
        "B1",
    ),
    op(
        79,
        "POST",
        "/api/model/confirm",
        "confirm_elements",
        "proposals",
        false,
        "B1",
    ),
    // B2 (UI pipeline): the backend gaps of the new UI (UI-SPEC §9).
    op(
        80,
        "GET",
        "/api/pipeline",
        "get_pipeline",
        "stages",
        false,
        "B2",
    ),
    op(
        81,
        "POST",
        "/api/jobs/{id}/retry",
        "retry_job",
        "stages",
        false,
        "B2",
    ),
    op(
        82,
        "POST",
        "/api/questions/generate",
        "generate_questions",
        "questions",
        false,
        "B2",
    ),
    op(
        83,
        "POST",
        "/api/findings/materialize",
        "materialize_findings",
        "findings",
        false,
        "B2",
    ),
    op(
        84,
        "GET",
        "/api/sources",
        "list_sources",
        "sources",
        false,
        "B2",
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

/// 412: the request's If-Match did not name the current branch-head revision. `revision` and
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

/// The typed health marker of a collection item whose projection failed (H0.11): the element
/// ID, the typed error code and message. The item's detail route fails with the same error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ItemHealth {
    pub id: String,
    pub code: String,
    pub message: String,
}

macro_rules! health_page {
    ($name:ident, $item:ty) => {
        /// A page of results; `next_cursor` is null on the last page. An item of the page whose
        /// projection failed is in `health` (with its typed error) instead of `items`; corruption
        /// is reported, never hidden, and the remaining items are still returned.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            pub items: Vec<$item>,
            pub health: Vec<ItemHealth>,
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
    /// Whether this server can run inference (sessions and inference-requiring recipes): false
    /// exactly when the configured provider mode is `null` (H0.11, Fable #355).
    pub inference_available: bool,
    /// The configured model name (the provider policy's `model`) while inference is available;
    /// null when inference is off or no model name is configured (B2).
    pub inference_model: Option<String>,
    /// The project this server serves (S1).
    pub project: ProjectInfo,
}

/// The served project (S1, UI-SPEC §5.9): what the toolbar and Settings show.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectInfo {
    /// The head revision's project ID ("project:hr-leave").
    pub id: String,
    /// The display name, humanised from the project ID ("HR leave").
    pub name: String,
    /// The database file the server opened; null when it has none (an in-memory database).
    pub database: Option<String>,
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
    /// When the head revision was written (B2).
    pub created_at: String,
    /// The actor that wrote the head revision (B2).
    pub created_by: String,
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
    /// Whether this entry is the gate's report current for the branch head (H0.11).
    pub state: GateEvaluationState,
    /// Why the gate is not current: the typed failure that prevents its evaluation now; null
    /// exactly when `state` is `current`.
    #[schema(required = true)]
    pub reason: Option<GateStateReason>,
    /// The current report's result; false when the gate is not current.
    pub passed: bool,
    /// Finding IDs.
    pub findings: Vec<String>,
    /// The report's rule results, in report order (Hotfix 082).
    pub rules: Vec<GateRuleResult>,
    /// The report's applied governed waivers, in report order (Hotfix 082).
    pub waivers: Vec<GateWaiver>,
    /// The report's own summary counts (Hotfix 082).
    pub summary: GateSummaryView,
}

/// The evaluation state of a NOW gate on the branch head (H0.11, architecture §33.12):
/// `current` has a report for the head; `stale` has only reports of earlier heads and cannot be
/// evaluated now; `not_evaluated` has never been evaluated and cannot be evaluated now. A gate
/// that is not current carries no rule results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum GateEvaluationState {
    #[serde(rename = "not_evaluated")]
    NotEvaluated,
    #[serde(rename = "current")]
    Current,
    #[serde(rename = "stale")]
    Stale,
}

/// The typed failure that keeps a gate from being evaluated on the branch head.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GateStateReason {
    pub code: String,
    pub message: String,
}

/// The exact result state of one rule in a gate report (Hotfix 082).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum GateRuleState {
    #[serde(rename = "PASS")]
    Pass,
    #[serde(rename = "FAIL")]
    Fail,
    #[serde(rename = "WARN")]
    Warn,
    #[serde(rename = "NOT_APPLICABLE")]
    NotApplicable,
    #[serde(rename = "WAIVED")]
    Waived,
    #[serde(rename = "ERROR")]
    Error,
}

/// Whether a rule applied to the gate's scope (Hotfix 082).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub enum GateRuleApplicabilityState {
    #[serde(rename = "APPLICABLE")]
    Applicable,
    #[serde(rename = "NOT_APPLICABLE")]
    NotApplicable,
}

/// A rule's applicability; `reason` is the exact server reason when not applicable, else null.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GateRuleApplicability {
    pub state: GateRuleApplicabilityState,
    #[schema(required = true)]
    pub reason: Option<String>,
}

/// The evaluator failure of an ERROR rule result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GateRuleError {
    pub code: String,
    pub message: String,
    pub targets: Vec<String>,
    pub evidence: Vec<String>,
}

/// One rule result of a current gate report (Hotfix 082); severities are the exact validation
/// severity strings (`blocker`, `error`, `warn`, `info`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GateRuleResult {
    pub rule_id: String,
    pub state: GateRuleState,
    pub declared_severity: String,
    pub effective_severity: String,
    pub applicability: GateRuleApplicability,
    pub targets: Vec<String>,
    pub evidence: Vec<String>,
    #[schema(required = true)]
    pub semantic_condition_key: Option<String>,
    #[schema(required = true)]
    pub finding_ref: Option<String>,
    #[schema(required = true)]
    pub waiver_ref: Option<String>,
    #[schema(required = true)]
    pub error: Option<GateRuleError>,
}

/// One governed waiver applied by a gate report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GateWaiver {
    pub rule_id: String,
    pub finding_ref: String,
    pub finding_key: String,
    pub decision_ref: String,
}

/// The summary counts of a gate report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GateSummaryView {
    #[schema(minimum = 0)]
    pub blocker_failed: u32,
    #[schema(minimum = 0)]
    pub blocker_waived: u32,
    #[schema(minimum = 0)]
    pub warnings: u32,
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
    /// The text the importer extracted for this span (S1); null when it extracted none.
    pub text: Option<String>,
    /// The requirements whose evidence cites this span, by ID (S1): the passages that became
    /// requirements.
    pub requirements: Vec<SpanRequirement>,
}
page!(SpanPage, Span);

/// A requirement a span became (S1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SpanRequirement {
    pub id: String,
    /// The requirement's identifier in the source document ("HR-009"), when it has one.
    pub source_identifier: Option<String>,
}

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
    // B2: the configured directory (pending actor, role, stakeholder, calendar, rule and
    // decision-table proposals; nothing is committed).
    Directory,
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
    #[schema(value_type = RequirementClass)]
    pub class: String,
    pub status: String,
    /// Error-severity lint rule IDs recorded for this requirement by the current F1 gate report
    /// (F1 evaluates Accepted Requirements only). Always `[]` when `lint_current` is false.
    pub smells: Vec<String>,
    pub coverage: RequirementCoverage,
    /// Advisory readiness; `null` means not calculated by the current pilot runtime. Always
    /// `null` when `lint_current` is false.
    pub readiness: Option<f64>,
    /// Whether a current F1 gate report exists for the branch head (R0.4). When false, lint
    /// results are unavailable (not "no findings").
    pub lint_current: bool,
}
page!(RequirementPage, RequirementSummary);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RequirementLinks {
    pub operations: Vec<String>,
    pub processes: Vec<String>,
    pub scenarios: Vec<String>,
}

/// One evidence reference of a requirement with its source (H0.11, Fable #351): `text` is the
/// exact text the importer extracted for that EvidenceFragment, `null` when the reference is not
/// an EvidenceFragment carrying extracted text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct EvidenceExcerpt {
    pub id: String,
    pub source_ref: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RequirementDetail {
    pub id: String,
    pub text: String,
    #[schema(value_type = RequirementClass)]
    pub class: String,
    pub status: String,
    pub smells: Vec<String>,
    pub coverage: RequirementCoverage,
    pub readiness: Option<f64>,
    pub spans: Vec<String>,
    /// The evidence references of `spans`, in the same order, with their source text.
    pub evidence: Vec<EvidenceExcerpt>,
    /// IDs of the current F1 gate report's findings whose affected refs include this
    /// requirement. Always `[]` when `lint_current` is false.
    pub lint_findings: Vec<String>,
    /// The normalized statement of the pending S1.2 EARS WorkflowProposal of this Requirement
    /// on the current head (R0.6); `null` when there is none. Paired with `ears_proposal_ref`.
    pub ears_suggestion: Option<String>,
    /// The ID of that pending S1.2 EARS WorkflowProposal, accepted through the governed
    /// proposal acceptance (R0.6); `null` exactly when `ears_suggestion` is `null`.
    pub ears_proposal_ref: Option<String>,
    pub linked: RequirementLinks,
    /// Whether a current F1 gate report exists for the branch head (R0.4). When false,
    /// `smells` and `lint_findings` are `[]`, `readiness` is `null` and lint is unavailable.
    pub lint_current: bool,
}

/// The closed requirement class vocabulary (`Requirement.requirement_kind`, §7.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RequirementClass {
    Functional,
    Quality,
    Interface,
    Data,
    Security,
    Operational,
    Compliance,
    Transition,
    Constraint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ClassifyRequest {
    #[schema(value_type = RequirementClass)]
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

/// The reviewed semantic change preview a direct write submits (H0.11, architecture §33.12):
/// the previewed `proposal_id` and the `built_at` instant the preview's Proposal was built at.
/// The write rebuilds the Proposal at `built_at` on the current head and commits it only when
/// it is exactly the previewed one (409 `E_PREVIEW_MISMATCH` otherwise); `built_at` must lie
/// between the head revision's creation and now.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PreviewConfirmation {
    pub proposal_id: String,
    pub built_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SplitRequest {
    pub parts: Vec<String>,
    pub reason: String,
    /// The reviewed preview this split submits; absent for a split without preview.
    #[serde(default)]
    pub preview: Option<PreviewConfirmation>,
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
    /// The reviewed preview this merge submits; absent for a merge without preview.
    #[serde(default)]
    pub preview: Option<PreviewConfirmation>,
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
    /// The Attribute ID (the target of set_attribute_type, _unit and _class).
    pub id: String,
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
health_page!(RulePage, Rule);

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

/// The exact v3 ProcessNode kinds (Hotfix 079).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProcessNodeViewKind {
    Start,
    End,
    HumanTask,
    ServiceTask,
    ExclusiveGateway,
    ParallelSplit,
    ParallelJoin,
    MessageEvent,
    TimerEvent,
    ErrorEvent,
    Subprocess,
}

/// One ProcessNode of a Process with its exact semantic material (Hotfix 079): the payload
/// fields and the targets of its active `performed_by`, `consumes` and `produces` relations,
/// each sorted and unique.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessNodeView {
    pub id: String,
    pub kind: ProcessNodeViewKind,
    #[schema(required = true)]
    pub operation_ref: Option<String>,
    /// Targets of the active `performed_by` relations: the swimlane authority.
    pub performer_refs: Vec<String>,
    /// Targets of the active `consumes` relations: trigger/event inputs.
    pub consumes_refs: Vec<String>,
    /// Targets of the active `produces` relations: outcomes/events.
    pub produces_refs: Vec<String>,
    #[schema(required = true)]
    pub condition_expr: Option<String>,
    #[schema(required = true)]
    pub message_ref: Option<String>,
    #[schema(required = true)]
    pub timer_expr: Option<String>,
}

/// One active canonical `next` relation between two ProcessNodes of the Process (Hotfix 079).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessFlowView {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Process {
    pub id: String,
    pub name: String,
    /// Sorted unique performer refs of the Process's nodes (active `performed_by` targets).
    pub lanes: Vec<String>,
    /// Sorted ProcessNode IDs belonging to the Process; not semantic execution order (use
    /// `flows`).
    pub steps: Vec<String>,
    pub branches: Vec<String>,
    pub validation: Vec<String>,
    /// The Process's ProcessNodes, sorted by ID (Hotfix 079).
    pub nodes: Vec<ProcessNodeView>,
    /// The active `next` topology between the Process's nodes, sorted by `from` then `to`
    /// (Hotfix 079): the flow authority.
    pub flows: Vec<ProcessFlowView>,
}
page!(ProcessPage, Process);

/// NOW process-step mutations, tagged by `op` (no `branch`; Hotfix 065). `add`, `move` and
/// `remove` change a strict linear Process and `position` is zero-based among interior steps;
/// `move_lane` (Hotfix 079) replaces the performers of one human or service task. `reason` is the
/// optional rationale passed to the shared intake/write path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProcessStepRequest {
    Add {
        operation_ref: String,
        position: u64,
        reason: Option<String>,
    },
    Move {
        node_ref: String,
        position: u64,
        reason: Option<String>,
    },
    Remove {
        node_ref: String,
        reason: Option<String>,
    },
    MoveLane {
        node_ref: String,
        performer_ref: String,
        reason: Option<String>,
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
health_page!(ScenarioPage, Scenario);

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

/// Where a finding's rule authority comes from (Hotfix 080): the active validation profile's
/// `RuleMetadata.rule_class` of the rule whose ID equals `Finding.code`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FindingRuleClass {
    ExternalStandard,
    ExternalInterop,
    PlumbCore,
    ProfilePolicy,
    OrganizationPolicy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub id: String,
    pub code: String,
    /// The validation gate/family of the finding (e.g. `I0`, `F1`); not the rule class.
    pub family: String,
    /// The rule class of the active-profile rule whose ID equals `code`; `null` when the code is
    /// not a rule of the active profile (Hotfix 080).
    #[schema(required = true)]
    pub rule_class: Option<FindingRuleClass>,
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
health_page!(AssumptionPage, Assumption);

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
health_page!(QuestionPage, Question);

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

/// A server-authoritative semantic change preview (Hotfix 073, H0.11): the exact domain
/// Proposal built at `built_at` on the baseline revision, with its intake and semantic element
/// diff. Nothing is persisted or committed. `version`, `revision` and `semantic_hash` are the
/// baseline; `events` is empty. Confirmation is the direct write with the same request and
/// `preview: {proposal_id, built_at}`, sent with the baseline revision as If-Match.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SemanticChangePreviewResponse {
    pub version: String,
    pub revision: String,
    pub semantic_hash: String,
    pub events: Vec<DesignEvent>,
    pub proposal_id: String,
    /// The instant the previewed Proposal was built at (part of its identity).
    pub built_at: String,
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

/// Where a pending workflow proposal came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProposalOrigin {
    Intake,
    Question,
    Session,
    Stage,
}

/// One pending workflow proposal occurrence (H0.11 proposal discovery).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PendingProposal {
    pub proposal_id: String,
    /// The one positive action that can commit it.
    pub action: ProposalKind,
    pub origin: ProposalOrigin,
    /// The Question of an answer, the session of a session turn; null otherwise.
    pub origin_ref: Option<String>,
    /// The recipe job and stage job of a stage proposal; null otherwise.
    pub recipe_job_id: Option<String>,
    pub stage_job_id: Option<String>,
    /// The revision it was built on.
    pub base_revision: String,
    /// Whether its stored intake requires a reason for a positive action.
    pub requires_reason: bool,
    /// Built on another revision than the head: it is never rebased; only reject (and, when
    /// discardable, discard) still apply.
    pub stale: bool,
    /// Whether `POST /api/proposals/{id}/discard` applies (an answer or intake proposal).
    pub discardable: bool,
    /// B1 Inbox: a plain-language summary derived deterministically from the stored patch and
    /// the graph (never a model), e.g. "Add operation Submit leave request".
    pub title: String,
    pub element_kind: ProposalElementKind,
    /// When the pending occurrence was persisted (RFC 3339, UTC).
    pub created_at: String,
    /// The stage recipe that produced a stage-origin proposal (e.g. `flows`); null for
    /// question, session and intake origins.
    pub stage: Option<String>,
}

// ---------------------------------------------------------------------------- B1 Inbox

/// What a pending proposal is about, for the Inbox (B1): the kind of its primary model element
/// for an add or a lifecycle change, or `link`, `merge`, `supersede`, `answer` (an answer to a
/// Question) and `directory_element` (a Stakeholder, Actor, BusinessRole or Calendar).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProposalElementKind {
    Operation,
    Event,
    Outcome,
    Requirement,
    GlossaryTerm,
    Entity,
    Attribute,
    State,
    Transition,
    Invariant,
    Calculation,
    Scenario,
    Rule,
    DecisionTable,
    Process,
    Link,
    Merge,
    Supersede,
    Answer,
    DirectoryElement,
    Other,
}

/// One plain-language fact of a pending proposal ("Performed by" / "Employee").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProposalFact {
    pub label: String,
    pub value: String,
}

/// One evidence passage of a pending proposal: an [`EvidenceExcerpt`] with the requirement
/// source identifier and the highlighted range. `highlight_start` and `highlight_end` are UTF-8
/// byte offsets into `text` (start inclusive, end exclusive, on character boundaries); both are
/// null exactly when `text` is null. `id` is the Requirement ID when the passage is a grounded
/// byte range of that Requirement's statement (`text` is the statement), otherwise the
/// EvidenceFragment ID (`text` is its extracted text, highlighted whole unless the proposal
/// grounds a narrower range of it).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProposalEvidence {
    pub id: String,
    pub source_ref: Option<String>,
    pub text: Option<String>,
    /// The `source_identifier` of the Requirement the passage belongs to (e.g. "HR-008").
    pub source_identifier: Option<String>,
    /// The UTF-8 byte offset into `text` where the highlight starts (inclusive).
    pub highlight_start: Option<u64>,
    /// The UTF-8 byte offset into `text` where the highlight ends (exclusive).
    pub highlight_end: Option<u64>,
}

/// One model element a pending proposal adds, modifies or removes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DiffItem {
    pub id: String,
    /// The lower snake_case node type (e.g. `operation`, `term`) or, for a relation, its
    /// relation kind (e.g. `performed_by`).
    pub kind: String,
    /// A plain-language label (the element name; a relation is "<from> <relation> <to>").
    pub label: String,
}

/// "What changes in the model" (B1): the element effect of dry-applying the pending patch to
/// the head (to its own base revision when it no longer applies to the head). Unlike
/// [`SemanticElementDiff`], it lists model elements whatever their lifecycle status (a create
/// proposal adds Proposed elements), excluding governance, provenance and proof material
/// (acceptance decisions, evidence, findings, questions). Each list is sorted by ID.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProposalModelDiff {
    pub added: Vec<DiffItem>,
    pub modified: Vec<DiffItem>,
    pub removed: Vec<DiffItem>,
}

/// One pending proposal as the Inbox inspector shows it (B1): every [`PendingProposal`] field
/// plus its facts, evidence passages and model change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PendingProposalDetail {
    pub proposal_id: String,
    pub action: ProposalKind,
    pub origin: ProposalOrigin,
    pub origin_ref: Option<String>,
    pub recipe_job_id: Option<String>,
    pub stage_job_id: Option<String>,
    pub base_revision: String,
    pub requires_reason: bool,
    pub stale: bool,
    pub discardable: bool,
    pub title: String,
    pub element_kind: ProposalElementKind,
    pub created_at: String,
    pub stage: Option<String>,
    /// For an operation: its performers, reads, writes, produced and consumed events (by target
    /// name); otherwise the key payload fields in plain words.
    pub facts: Vec<ProposalFact>,
    pub evidence: Vec<ProposalEvidence>,
    pub diff: ProposalModelDiff,
}

fn default_true() -> bool {
    true
}

/// Accepts pending proposals together (B1, the Inbox multi-select "Accept N").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AcceptProposalSetRequest {
    /// The pending proposals on the head; each is committed with the action of its stored
    /// class.
    pub proposal_ids: Vec<String>,
    /// One reason for every member (required when any member's intake requires one).
    #[serde(default)]
    pub reason: Option<String>,
    /// Also promote the Proposed elements the members created to Accepted through the
    /// governed confirmation (`confirm_elements`). Default true.
    #[serde(default = "default_true")]
    #[schema(default = true)]
    pub confirm: bool,
}

/// The result of `accept_proposal_set`: the write fields of the last revision, the accepted
/// proposals and the confirmed elements (both sorted). `events` holds the acceptance event and
/// then one event per confirmation revision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AcceptProposalSetResponse {
    pub version: String,
    pub revision: String,
    pub semantic_hash: String,
    pub events: Vec<DesignEvent>,
    pub accepted: Vec<String>,
    pub confirmed: Vec<String>,
}

/// The governed confirmation of explicitly listed Proposed elements (B1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfirmElementsRequest {
    pub element_ids: Vec<String>,
    /// The rationale of the generic confirmation.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Every pending workflow proposal, sorted by proposal ID then base revision. Not paged: the
/// pending set changes without a revision (a discard or reject commits nothing), so a cursor
/// could not pin it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PendingProposalList {
    pub items: Vec<PendingProposal>,
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
    /// The authoritative workflow class of the proposal (Hotfix 077): the one positive proposal
    /// action endpoint that can confirm it.
    pub kind: ProposalKind,
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
    /// `scenarios.yaml` (Fable #307: every `report --out` artifact is exportable).
    Scenarios,
    /// `gate-report.json`.
    #[serde(rename = "gate_report")]
    GateReport,
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

// ============================================================================ UI pipeline (B2)

/// The derived state of one pipeline step (B2): `queued` a captured job not started yet;
/// `running` acquiring or evaluating; `needs_review` its latest job is ready on the head with
/// pending proposals of that job; `done` committed, or ready with nothing pending; `stale` its
/// latest job is stale, or ready with pending proposals built on another revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PipelineStepStatus {
    NotStarted,
    Queued,
    Running,
    NeedsReview,
    Done,
    Failed,
    Stale,
}

/// The typed failure of a failed step's latest job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PipelineError {
    pub code: String,
    pub message: String,
}

/// One stage recipe of the pipeline, from its latest recipe job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PipelineStep {
    pub recipe: StageName,
    pub status: PipelineStepStatus,
    /// The latest recipe job of this recipe; null when it never ran.
    pub job_id: Option<String>,
    /// The latest job's status-derived progress; null when it never ran.
    #[schema(minimum = 0.0, maximum = 1.0)]
    pub progress: Option<f64>,
    /// Pending workflow proposals produced by any job of this recipe (stale ones included).
    pub pending_proposals: u64,
    pub error: Option<PipelineError>,
    /// When the latest job last changed.
    pub updated_at: Option<String>,
    /// The configuration the recipe needs and that is absent (`run_stage` would be 409
    /// `E_STAGE_CONFIG_MISSING`); empty when it can run.
    pub missing_config: Vec<String>,
}

/// `get_pipeline`: every stage recipe in pipeline order with the Overview counts. `next` is the
/// first step that is not `done` and whose configuration is complete; null when there is none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PipelineResponse {
    pub steps: Vec<PipelineStep>,
    /// Imported sources (SourceArtifact nodes) on the head.
    pub sources: u64,
    /// Accepted Questions whose status is open.
    pub open_questions: u64,
    pub next: Option<StageName>,
}

/// `retry_job` takes no parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RetryJobRequest {}

/// `generate_questions`: with `stakeholder`, also the round of at most `max` (default 1000)
/// of that stakeholder's eligible unassigned questions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateQuestionsRequest {
    #[serde(default)]
    pub stakeholder: Option<String>,
    #[serde(default)]
    #[schema(minimum = 1, maximum = 1000)]
    pub max: Option<u64>,
}

/// The questions of the current gate reports (new, existing and updated) and, when requested
/// and the stakeholder had eligible unassigned questions, its new round.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateQuestionsResponse {
    pub version: String,
    pub revision: String,
    pub semantic_hash: String,
    pub events: Vec<DesignEvent>,
    pub question_ids: Vec<String>,
    pub round: Option<Round>,
}

/// `materialize_findings` takes no parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MaterializeFindingsRequest {}

/// The newly materialized Finding nodes (stale open findings it closed are in `events`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MaterializeFindingsResponse {
    pub version: String,
    pub revision: String,
    pub semantic_hash: String,
    pub events: Vec<DesignEvent>,
    pub finding_ids: Vec<String>,
}

/// One imported source: the `get_source` projection plus its display name, import time and
/// passage (span) count.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceListItem {
    pub id: String,
    pub hash: String,
    pub media_type: String,
    pub source_kind: String,
    pub name: String,
    pub imported_at: String,
    pub spans_count: u64,
}

/// Every imported source, by ID. Not paged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceList {
    pub items: Vec<SourceListItem>,
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
        params(("q" = String, Query, description = "Search text; required and not blank (400 otherwise)"),
               ("scope" = Option<SearchScope>, Query, description = "Search scope")),
        responses((status = 200, description = "Search hits over Accepted elements only: Proposed, Rejected and other non-Accepted elements are never searchable", body = SearchPage)))]
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
        responses((status = 200, description = "The exact create-class split proposal, intake and semantic element diff; nothing is persisted or committed", body = SemanticChangePreviewResponse)))]
    pub fn preview_requirement_split() {}

    #[utoipa::path(post, path = "/api/glossary/{id}/merge/preview", operation_id = "preview_glossary_merge", tag = "glossary",
        params(("id" = String, Path, description = "Term ID")),
        request_body(content = GlossaryMergeRequest),
        responses((status = 200, description = "The exact merge proposal, intake and semantic element diff; nothing is persisted or committed", body = SemanticChangePreviewResponse)))]
    pub fn preview_glossary_merge() {}

    #[utoipa::path(get, path = "/api/proposals", operation_id = "list_proposals", tag = "proposals",
        responses((status = 200, description = "Every pending workflow proposal", body = PendingProposalList)))]
    pub fn list_proposals() {}

    #[utoipa::path(post, path = "/api/proposals/{id}/discard", operation_id = "discard_proposal", tag = "proposals",
        params(("id" = String, Path, description = "Proposal ID")),
        request_body(content = ProposalActionRequest),
        responses((status = 200, description = "The pending answer or intake proposal is abandoned; nothing is committed", body = WriteResult)))]
    pub fn discard_proposal() {}

    #[utoipa::path(get, path = "/api/proposals/{id}", operation_id = "get_proposal", tag = "proposals",
        params(("id" = String, Path, description = "Proposal ID")),
        responses((status = 200, description = "The pending occurrence on the head with its facts, evidence and model change (404 E_NOT_FOUND when unknown, 409 E_PROPOSAL_NOT_PENDING when resolved)", body = PendingProposalDetail)))]
    pub fn get_proposal() {}

    #[utoipa::path(post, path = "/api/proposals/accept-set", operation_id = "accept_proposal_set", tag = "proposals",
        request_body(content = AcceptProposalSetRequest),
        responses((status = 200, description = "Every member accepted atomically in one revision, then (confirm) the Proposed elements they created confirmed; a stale, unknown or non-pending member applies nothing", body = AcceptProposalSetResponse)))]
    pub fn accept_proposal_set() {}

    #[utoipa::path(post, path = "/api/model/confirm", operation_id = "confirm_elements", tag = "proposals",
        request_body(content = ConfirmElementsRequest),
        responses((status = 200, description = "The listed Proposed elements are Accepted: Terms, Calculations and Scenarios through their dedicated confirmations (one revision each), every other element through one generic governed confirmation", body = WriteResult)))]
    pub fn confirm_elements() {}

    // B2 (UI pipeline).

    #[utoipa::path(get, path = "/api/pipeline", operation_id = "get_pipeline", tag = "stages",
        responses((status = 200, description = "Every stage recipe in pipeline order (extract, consolidate, lint, directory, analyse, flows, calc, rules, scenarios, tags, run), derived from its latest recipe job and the pending workflow proposals", body = PipelineResponse)))]
    pub fn get_pipeline() {}

    #[utoipa::path(post, path = "/api/jobs/{id}/retry", operation_id = "retry_job", tag = "stages",
        params(("id" = String, Path, description = "Job ID")),
        request_body(content = RetryJobRequest),
        responses((status = 200, description = "The failed job is queued again as its next attempt under the same ID (the run_stage response); 409 E_STAGE_RETRY_NOT_FAILED or E_STAGE_RETRY_BASE_MOVED otherwise", body = StageRunResponse)))]
    pub fn retry_job() {}

    #[utoipa::path(post, path = "/api/questions/generate", operation_id = "generate_questions", tag = "questions",
        request_body(content = GenerateQuestionsRequest),
        responses((status = 200, description = "Questions materialized from the current gate reports with the configured routing, and the optional stakeholder round", body = GenerateQuestionsResponse)))]
    pub fn generate_questions() {}

    #[utoipa::path(post, path = "/api/findings/materialize", operation_id = "materialize_findings", tag = "findings",
        request_body(content = MaterializeFindingsRequest),
        responses((status = 200, description = "Actionable current gate findings persisted as Finding nodes; stale open ones closed", body = MaterializeFindingsResponse)))]
    pub fn materialize_findings() {}

    #[utoipa::path(get, path = "/api/sources", operation_id = "list_sources", tag = "sources",
        responses((status = 200, description = "Every imported source", body = SourceList)))]
    pub fn list_sources() {}
}

/// The utoipa base contract of the 84 operations.
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
        stubs::preview_glossary_merge, stubs::list_proposals, stubs::discard_proposal,
        stubs::get_proposal, stubs::accept_proposal_set, stubs::confirm_elements,
        stubs::get_pipeline, stubs::retry_job, stubs::generate_questions,
        stubs::materialize_findings, stubs::list_sources,
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
            "operations differ from the frozen 84-operation inventory",
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
                "description": "The current branch-head revision ID, as the strong entity-tag from ETag (\"rev:...\") or bare (rev:...). A weak validator, * or a semantic hash is 412.",
                "schema": {"type": "string"}
            }));
            responses_mut(operation)?.insert(
                "412".to_owned(),
                json!({
                    "description": "Precondition Failed: If-Match does not name the current branch-head revision ID; revision and semantic_hash are the current branch-head values.",
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
                                "description": "The strong entity-tag of the loaded branch-head revision ID (the quoted revision ID).",
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
    // H0.11: the authoritative error-code catalogue of every typed error body.
    let codes: Vec<Value> = crate::error_catalogue::ERROR_CATALOGUE
        .iter()
        .map(|e| json!({"code": e.code, "status": e.status, "meaning": e.meaning}))
        .collect();
    root.get_mut("info")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| invariant("document has no info object"))?
        .insert("x-plumb-error-codes".to_owned(), json!(codes));
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
        "Plumb pilot HTTP API contract (A0.1): a task-oriented compatibility projection over the PSG. Runtime handlers are provided by A0.2, A0.3 and A0.4 (operations 1-72), U2.1 (the two semantic change previews, operations 73-74), H0.11 (proposal discovery and discard, operations 75-76) B1 (the Inbox proposal detail, set acceptance and confirmation, operations 77-79) and B2 (the UI pipeline operations 80-84)."
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
        "A0.1 defines the pilot contract only; A0.2, A0.3 and A0.4 provide the runtime handlers of operations 1-72, U2.1 those of operations 73-74, H0.11 those of operations 75-76 B1 those of operations 77-79 and B2 those of operations 80-84. \
An operation listed here is not operational until its handler task is done.\n\n",
    );
    out.push_str("## Conventions\n\n");
    for line in [
        "The API is a task-oriented compatibility projection over the PSG. Requests and responses are JSON unless stated otherwise; errors carry `code`, `message` and `details`. Every error `code` (including framework failures: unknown route, method, malformed input, body limit and media type) is one of the catalogued codes in `info.x-plumb-error-codes` with its HTTP status and meaning.",
        "Every ordinary `GET` success response carries an `ETag` header: the RFC 9110 strong entity-tag of the loaded branch-head revision ID (the quoted `rev:...`).",
        "Every `POST` (including dry-run intake, stage runs and session creation and turns) requires an `If-Match` header naming the current branch-head revision ID, quoted as in `ETag` or bare; weak validators and `*` never match, and `semantic_hash` is not a concurrency token. A mismatch returns `412 Precondition Failed` with `code`, `message`, `details` and the current branch-head `revision` and `semantic_hash`.",
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
    for owner in ["A0.2", "A0.3", "A0.4", "U2.1", "H0.11", "B1", "B2"] {
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
        "Hotfix 073 appends operations 73 and 74, the U2.1 semantic change previews: the server builds the exact domain Proposal and returns its semantic element diff without committing; `candidate_semantic_hash` is not a prediction of the committed `semantic_hash`.",
        "H0.11 (architecture §33.12): a preview never persists a pending proposal. It returns the previewed `proposal_id` and `built_at`; confirmation is the direct write (split or merge) with the same request plus `preview: {proposal_id, built_at}` and the baseline revision as If-Match, which commits only exactly the previewed proposal (409 `E_PREVIEW_MISMATCH` otherwise).",
        "H0.11 appends operations 75 and 76: `GET /api/proposals` lists every pending workflow proposal (not paged), and `POST /api/proposals/{id}/discard` moves a pending answer or intake proposal to the terminal state `abandoned` without a revision. Accepted, rejected and abandoned proposals are kept as history.",
        "B1 (UI Inbox, UI-SPEC §5.2) appends operations 77 to 79: `GET /api/proposals/{id}` is one pending proposal with its facts, evidence passages (UTF-8 byte highlight offsets) and model change; `POST /api/proposals/accept-set` accepts pending proposals atomically in one revision through the R0.3 set acceptance and, unless `confirm` is false, then confirms the Proposed elements they created; `POST /api/model/confirm` is the governed confirmation of the CLI `confirm`. List items gain `title`, `element_kind`, `created_at` and `stage`.",
        "B2 appends operations 80 to 84 for the UI (UI-SPEC §9): `GET /api/pipeline` derives every stage recipe's state from its latest recipe job and the pending proposals; `POST /api/jobs/{id}/retry` re-queues a failed recipe job through the H0.3 retry service; `POST /api/questions/generate` is the CLI `ask` (optionally with a stakeholder round); `POST /api/findings/materialize` is the CLI `findings materialize`; `GET /api/sources` lists every imported source. `directory` becomes a `run_stage` recipe, `get_me` gains `inference_model` and `get_model_version` gains `created_at` and `created_by`.",
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
