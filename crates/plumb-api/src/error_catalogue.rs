//! The authoritative catalogue of PLUMB API error codes (architecture §33.12, H0.11): every `code`
//! of a typed `{code, message, details}` error body, with its HTTP status and meaning. The
//! OpenAPI contract documents exactly this list (`info.x-plumb-error-codes`); a test proves that
//! every code literal in this crate is catalogued and every catalogued code is used.
//!
//! Codes that only appear inside a recipe job's `error` (never as an HTTP response) carry the
//! status of the response that would report the same failure synchronously.

/// One catalogued error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogueEntry {
    pub code: &'static str,
    pub status: u16,
    pub meaning: &'static str,
}

const fn entry(code: &'static str, status: u16, meaning: &'static str) -> CatalogueEntry {
    CatalogueEntry {
        code,
        status,
        meaning,
    }
}

/// Every PLUMB API error code, sorted by code.
pub const ERROR_CATALOGUE: &[CatalogueEntry] = &[
    entry(
        "E_A03_RUNTIME_UNAVAILABLE",
        503,
        "The operation needs runtime material that is not configured.",
    ),
    entry(
        "E_ASSUMPTION_NOT_ACTIVE",
        409,
        "The assumption is not in an active lifecycle state.",
    ),
    entry(
        "E_ATTRIBUTE_ALREADY_CLASSIFIED",
        409,
        "The attribute already has the requested classification.",
    ),
    entry(
        "E_AUTO_PROPOSAL_REQUIRES_REASON",
        409,
        "An automatically accepted stage proposal would need a human reason.",
    ),
    entry(
        "E_BRANCH_NOT_FOUND",
        404,
        "The configured branch has no head revision.",
    ),
    entry(
        "E_CALCULATION_EVALUATION",
        422,
        "The calculation cannot be evaluated for the given inputs.",
    ),
    entry(
        "E_CALCULATION_SCOPE_UNAVAILABLE",
        409,
        "The calculation's input scope is not available on the current head.",
    ),
    entry(
        "E_CALCULATION_VALUE_UNREPRESENTABLE",
        422,
        "The calculation result cannot be represented on the wire.",
    ),
    entry(
        "E_CURSOR_BASELINE_CHANGED",
        409,
        "The cursor was issued at another branch-head revision or semantic hash.",
    ),
    entry(
        "E_CURSOR_INVALID",
        400,
        "The cursor is malformed or not canonical.",
    ),
    entry(
        "E_CURSOR_REQUEST_MISMATCH",
        400,
        "The cursor belongs to another request.",
    ),
    entry(
        "E_DATABASE_BUSY",
        503,
        "The database is locked by another writer; retry later.",
    ),
    entry(
        "E_DATABASE_NOT_INITIALIZED",
        409,
        "The database does not exist or is not an initialized PLUMB store.",
    ),
    entry(
        "E_DATABASE_READ_ONLY",
        409,
        "The database was opened read-only and cannot be written.",
    ),
    entry(
        "E_DECISION_TABLE_ENCODING_INVALID",
        409,
        "A stored decision table encoding is invalid.",
    ),
    entry(
        "E_DECISION_TABLE_INVALID",
        409,
        "The decision table is not valid for this operation.",
    ),
    entry(
        "E_DIFF_NO_PARENT",
        422,
        "The revision has no parent to diff against.",
    ),
    entry(
        "E_EXPRESSION_LIMIT",
        422,
        "An expression exceeds a resource limit.",
    ),
    entry(
        "E_EXTERNAL_VALIDATOR_UNAVAILABLE",
        409,
        "A required external validator is not available.",
    ),
    entry(
        "E_FINDING_MATERIAL_MISMATCH",
        409,
        "The finding's material no longer matches the current report.",
    ),
    entry(
        "E_FINDING_NOT_CURRENT",
        409,
        "The finding is not in the current gate report.",
    ),
    entry(
        "E_FINDING_NOT_FOR_REQUIREMENT",
        409,
        "The finding does not affect this requirement.",
    ),
    entry(
        "E_FORBIDDEN_HOST",
        403,
        "The request Host is missing or is not a loopback address.",
    ),
    entry(
        "E_FORBIDDEN_ORIGIN",
        403,
        "The request Origin is not a loopback origin.",
    ),
    entry(
        "E_GATE_INPUTS_MISSING",
        409,
        "Configured gate inputs are missing.",
    ),
    entry(
        "E_GATE_INPUTS_STALE",
        409,
        "Gate inputs are stale for the current head.",
    ),
    entry(
        "E_GATE_REPORT_INCOHERENT",
        409,
        "A persisted gate report contradicts the current head.",
    ),
    entry(
        "E_GATE_REPORT_UNAVAILABLE",
        409,
        "No current gate report exists and none can be evaluated.",
    ),
    entry(
        "E_ID_COLLISION",
        409,
        "A derived stable ID already names another element.",
    ),
    entry(
        "E_INFERENCE_FAILED",
        502,
        "The inference provider or its output inspection failed.",
    ),
    entry(
        "E_INFERENCE_REQUIRED",
        409,
        "The operation requires inference and the null provider is configured.",
    ),
    entry(
        "E_INFERENCE_UNAVAILABLE",
        409,
        "A session needs an inference provider and the null provider is configured.",
    ),
    entry(
        "E_INTAKE_REASON_REQUIRED",
        409,
        "Intake requires a non-blank reason for this change.",
    ),
    entry(
        "E_INTERNAL",
        500,
        "Unexpected internal failure; details name the failing subsystem only.",
    ),
    entry(
        "E_INVALID_REQUEST",
        400,
        "Malformed path, query, header, body or multipart input.",
    ),
    entry(
        "E_JOB_INTERRUPTED",
        409,
        "A recipe job was interrupted by a process restart.",
    ),
    entry(
        "E_MERMAID_INVALID",
        400,
        "The Mermaid process text is malformed.",
    ),
    entry(
        "E_MERMAID_UNREPRESENTABLE",
        409,
        "The process cannot be represented as linear Mermaid.",
    ),
    entry(
        "E_METHOD_NOT_ALLOWED",
        405,
        "The route exists but does not accept this HTTP method.",
    ),
    entry("E_NOT_FOUND", 404, "The addressed element does not exist."),
    entry(
        "E_NO_CHANGE",
        422,
        "The requested change would not change the model.",
    ),
    entry(
        "E_NO_CONFIRMABLE_DERIVED_SCENARIOS",
        409,
        "The decision table has no confirmable derived scenarios.",
    ),
    entry(
        "E_NO_QUESTIONS_FOR_ROUND",
        409,
        "No open questions are available for the round.",
    ),
    entry(
        "E_NO_RUNNABLE_SCENARIOS",
        409,
        "No scenario in the scope can be run.",
    ),
    entry(
        "E_PATCH_REJECTED",
        422,
        "The proposed change cannot be applied to the current graph.",
    ),
    entry(
        "E_PAYLOAD_TOO_LARGE",
        413,
        "The request body exceeds the route's size limit.",
    ),
    entry(
        "E_PRECONDITION_FAILED",
        412,
        "If-Match does not name the current branch-head revision.",
    ),
    entry(
        "E_PROCESS_IMPORT_CONFLICT",
        409,
        "The imported process conflicts with existing elements.",
    ),
    entry(
        "E_PROCESS_MUTATION_INVALID",
        422,
        "The process step change is invalid.",
    ),
    entry("E_PROCESS_NOT_LINEAR", 409, "The process is not linear."),
    entry(
        "E_PROCESS_STEP_HAS_RELATIONS",
        409,
        "The process step still has relations.",
    ),
    entry(
        "E_PROPOSAL_ACTION_AMBIGUOUS",
        422,
        "The proposal's action cannot be classified unambiguously.",
    ),
    entry(
        "E_PROPOSAL_ACTION_MISMATCH",
        409,
        "The requested action is not the proposal's classified action.",
    ),
    entry(
        "E_PROPOSAL_BASELINE_CHANGED",
        409,
        "The proposal's baseline revision is no longer the head.",
    ),
    entry(
        "E_PROPOSAL_EVIDENCE_MISMATCH",
        400,
        "The intake evidence does not match the proposal.",
    ),
    entry(
        "E_PROPOSAL_NOT_DISCARDABLE",
        409,
        "Only a pending answer or intake proposal can be discarded.",
    ),
    entry(
        "E_PROPOSAL_NOT_PENDING",
        409,
        "The proposal is not pending.",
    ),
    entry(
        "E_PROPOSAL_RESOLUTION_INVALID",
        409,
        "The resolution revision does not descend from the proposal's base.",
    ),
    // B1 Inbox.
    entry(
        "E_PROPOSAL_SET_CONFIRMATION_FAILED",
        409,
        "The proposal set was accepted, but confirming the elements it created failed.",
    ),
    entry(
        "E_PROPOSAL_SET_CONFLICT",
        409,
        "The proposals of the set are not compatible on their shared base.",
    ),
    entry(
        "E_PREVIEW_MISMATCH",
        409,
        "The submitted change is not the reviewed preview's proposal.",
    ),
    entry(
        "E_QUESTION_GENERATION_INVALID",
        422,
        "Question generation produced invalid output.",
    ),
    entry(
        "E_QUESTION_GENERATION_ISSUES",
        409,
        "Question generation reported issues.",
    ),
    entry(
        "E_QUESTION_PROJECTION_INVALID",
        409,
        "A persisted question cannot be projected.",
    ),
    entry(
        "E_ROUND_COMPOSITION_FAILED",
        409,
        "The question round cannot be composed.",
    ),
    entry(
        "E_ROUND_INCONSISTENT",
        409,
        "A persisted round contradicts its questions.",
    ),
    entry(
        "E_ROUND_STAKEHOLDER_INVALID",
        409,
        "The round stakeholder is not valid.",
    ),
    entry("E_ROUTE_NOT_FOUND", 404, "No API route has this path."),
    entry(
        "E_RULE_NOT_DECISION_TABLE",
        409,
        "The rule is not a decision table.",
    ),
    entry(
        "E_RUNTIME_CONFIG_INVALID",
        500,
        "The runtime configuration is invalid; reported before any store opens.",
    ),
    entry(
        "E_SCENARIO_NOT_CONFIRMABLE",
        409,
        "The scenario cannot be confirmed in its lifecycle state.",
    ),
    entry(
        "E_SCENARIO_NOT_RUNNABLE",
        409,
        "The scenario cannot be run in its lifecycle state.",
    ),
    entry(
        "E_SCENARIO_PROJECTION_INVALID",
        409,
        "A persisted scenario cannot be projected.",
    ),
    entry(
        "E_SCENARIO_RUN_COLLISION",
        422,
        "The stable scenario run ID holds another element.",
    ),
    entry(
        "E_SCENARIO_RUN_TRACE_ARTIFACT_INVALID",
        409,
        "A scenario run trace artifact is invalid.",
    ),
    entry(
        "E_SCENARIO_RUN_TRACE_ARTIFACT_MISSING",
        409,
        "A scenario run trace artifact is missing.",
    ),
    entry(
        "E_SCENARIO_RUN_TRACE_LINK_INVALID",
        409,
        "A scenario run trace link is invalid.",
    ),
    entry(
        "E_SCENARIO_RUN_TRACE_MISMATCH",
        409,
        "A scenario run trace does not match its run.",
    ),
    entry(
        "E_SCENARIO_STATE_INVALID",
        409,
        "The scenario's lifecycle state does not allow the operation.",
    ),
    entry(
        "E_SEMANTIC_INVARIANT",
        422,
        "A domain service rejected the change or the target is in the wrong lifecycle state.",
    ),
    entry(
        "E_SESSION_NOT_OWNED",
        403,
        "The session belongs to another user.",
    ),
    entry(
        "E_SESSION_TURN_IN_FLIGHT",
        409,
        "A turn of the session is still in flight; nothing was written.",
    ),
    entry(
        "E_SOURCE_CONFLICT",
        409,
        "A different source with this identity already exists.",
    ),
    entry(
        "E_SOURCE_EMPTY",
        400,
        "The uploaded file yields no evidence fragments; nothing was written.",
    ),
    entry("E_SOURCE_EXISTS", 409, "This exact source already exists."),
    entry(
        "E_STAGE_CHILD_FAILED",
        409,
        "A child stage of the recipe failed.",
    ),
    entry(
        "E_STAGE_COMMIT_FAILED",
        409,
        "The stage output could not be committed.",
    ),
    entry(
        "E_STAGE_CONFIG_MISSING",
        409,
        "The recipe needs configuration that is absent.",
    ),
    entry(
        "E_STAGE_CONFIG_UNRESOLVED",
        422,
        "A configured selector cannot be resolved against the graph the stage runs on.",
    ),
    entry(
        "E_STAGE_EVALUATION_FAILED",
        422,
        "A phase or gate cannot be evaluated; details.error names the typed error.",
    ),
    entry(
        "E_STAGE_MIXED_OUTPUT",
        422,
        "A stage produced incompatible output kinds.",
    ),
    entry(
        "E_STAGE_PLAN_FAILED",
        422,
        "A phase cannot be planned; details.error names the typed error.",
    ),
    entry(
        "E_STAGE_PROPOSAL_CONFLICT",
        409,
        "Stage proposals conflict with each other.",
    ),
    entry(
        "E_STAGE_RETRY_BASE_MOVED",
        409,
        "The failed job cannot be retried because the branch head moved.",
    ),
    entry(
        "E_STAGE_RETRY_NOT_FAILED",
        409,
        "Only a failed job can be retried.",
    ),
    entry("E_STAGE_SCOPE_INVALID", 422, "The recipe scope is invalid."),
    entry(
        "E_STAGE_WORKER_UNAVAILABLE",
        503,
        "The stage worker of this process is not running or is shutting down.",
    ),
    entry(
        "E_STORE_CORRUPT",
        500,
        "Persisted store or artifact content failed an integrity check.",
    ),
    entry(
        "E_STORE_VERSION_UNSUPPORTED",
        409,
        "A stored revision uses an unsupported PSG schema version.",
    ),
    entry(
        "E_TERM_IDENTITY_CHANGE",
        422,
        "The edit would change the term's identity.",
    ),
    entry("E_TERM_MERGE_CONFLICT", 409, "The terms cannot be merged."),
    entry(
        "E_UNREPRESENTABLE_ASSUMPTION_DEFAULT",
        422,
        "The assumption default cannot be represented on the wire.",
    ),
    entry(
        "E_UNSUPPORTED_MEDIA_TYPE",
        415,
        "The request or source media type is not supported.",
    ),
    entry(
        "E_WORKFLOW_CORRUPT",
        500,
        "Persisted workflow state is impossible or corrupt.",
    ),
];

/// The catalogue entry of `code`.
pub fn lookup(code: &str) -> Option<&'static CatalogueEntry> {
    ERROR_CATALOGUE.iter().find(|entry| entry.code == code)
}
