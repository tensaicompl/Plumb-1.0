//! Grounded calculations over explicit PlumbExpr scopes (plan S2.6; compiler architecture §8).
//!
//! A supplied, already-acquired `calculation_analysis` inference names a calculation by a
//! grounded requirement range, proposes a PlumbExpr expression with grounded evidence and
//! declares its result type, unit, rounding and calendar. Plumb resolves the expression only
//! through an explicit [`CalculationScope`]: every input symbol is an explicit binding of an
//! Accepted Entity, Attribute or Calculation, PSG names are never rewritten into identifiers,
//! and the root symbol `calendar` is reserved for `Calculation.calendar_ref`. The S2.4 parser
//! and S2.5 type checker, evaluator and rounding are reused; nothing here emits validation
//! findings, mutates a graph, calls a provider, reads a clock or persists anything, and
//! `apply_patch` is used solely to dry-validate proposals in memory.

use std::collections::{BTreeMap, BTreeSet};

use jsonschema::{Draft, JSONSchema};
use plumb_core::{to_canonical_json, CoreError, Hash, Id, StageId, Timestamp};
use plumb_expr::{
    check_result, eval, CalendarProvider, DeclaredResult, EvalContext, EvalError,
    PredicateProvider, RefValue, ResultCheckError, Symbol, Value, ValueEnv,
};
use plumb_inference::{
    InferenceArtifact, InferenceError, InferenceRequest, InferenceRequestMaterial, ProviderError,
    ProviderPolicy,
};
use plumb_patch::{
    apply_patch, AcceptancePolicy, PatchSet, Proposal, ProposalMateriality, SemanticPatch,
};
use plumb_psg::{
    AuditMeta, Calculation, DerivationRef, ElementStatus, EvidenceRef, ExtensionKey, Graph, Node,
    NodePayload,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use thiserror::Error;

/// Version of [`CalculationContext`].
pub const CALCULATION_CONTEXT_VERSION: u32 = 1;

/// Version of the calculation inference output.
pub const CALCULATION_OUTPUT_VERSION: u32 = 1;

/// `InferenceRequest.task_kind` of calculation analysis.
pub const CALCULATION_TASK_KIND: &str = "calculation_analysis";

const PROMPT_TEMPLATE: &[u8] = include_bytes!("../../../prompts/s2-calculation.md");
const SCHEMA_SOURCE: &str = include_str!("../../../schemas/inference/s2-calculation.schema.json");
const CALCULATION_STAGE: StageId = StageId::S2;

// ============================================================================ errors and issues

/// Why calculation analysis could not run. Malformed supplied inference and invalid
/// requests are errors; semantic problems of a candidate are [`CalculationIssue`]s, and a
/// candidate whose groundings, calendar or identity break the artifact contract is dropped
/// with an issue while its valid siblings are still analysed (any-domain tolerance).
#[derive(Debug, Error)]
pub enum CalculationError {
    #[error("invalid calculation input: {reason}")]
    InvalidInput { reason: String },
    #[error("invalid calculation scope: {0:?}")]
    Scope(CalculationIssue),
    #[error("inference request: {0}")]
    InferenceConstruction(#[from] InferenceError),
    #[error("inference material: {0}")]
    InferenceMaterial(#[from] ProviderError),
    #[error("invalid calculation inference artifact: {reason}")]
    InvalidInferenceArtifact { reason: String },
    #[error("calculation schema does not compile: {reason}")]
    SchemaCompilation { reason: String },
    #[error("calculation output is schema-invalid: {reason}")]
    SchemaInvalid { reason: String },
    #[error("invalid proposal: {reason}")]
    InvalidProposal { reason: String },
    #[error(transparent)]
    Core(#[from] CoreError),
}

impl From<CalculationAnalysisError> for CalculationError {
    fn from(e: CalculationAnalysisError) -> CalculationError {
        match e {
            CalculationAnalysisError::InvalidInput { reason } => {
                CalculationError::InvalidInput { reason }
            }
        }
    }
}

fn invalid_input(reason: impl Into<String>) -> CalculationError {
    CalculationError::InvalidInput {
        reason: reason.into(),
    }
}

fn invalid_proposal(e: impl ToString) -> CalculationError {
    CalculationError::InvalidProposal {
        reason: e.to_string(),
    }
}

// The pure qualification kernel lives in plumb-validation (Hotfix 044); these are its S2.6
// public names.
use plumb_validation::calculation_analysis::CalculationAnalysisError;
pub use plumb_validation::calculation_analysis::{
    accepted_calendar, calculation_origin_of, qualify_calculation, validate_calculation_bindings,
    validate_calculation_scope, AcceptedCalculationQualification, CalculationBindingExposure,
    CalculationCycleAnalysis, CalculationDraft, CalculationGroundedRange, CalculationIssue,
    CalculationOrigin, CalculationQualification, CalculationScope, CalculationScopeBinding,
    PreparedCalculation, CALCULATION_CALENDAR_SYMBOL, CALCULATION_ORIGIN_EXTENSION,
};

// ============================================================================ default bindings

fn payload_name(node: &Node) -> Option<&str> {
    match &node.payload {
        NodePayload::Entity(e) => Some(&e.name),
        NodePayload::Attribute(a) => Some(&a.name),
        NodePayload::Calculation(c) => Some(&c.name),
        _ => None,
    }
}

/// The exact payload name as a binding symbol, when it already is a valid non-reserved Symbol.
fn default_symbol(graph: &Graph, node_ref: &Id) -> Result<String, CalculationIssue> {
    let node = graph
        .node(node_ref)
        .ok_or_else(|| CalculationIssue::UnknownInputRef {
            node_ref: node_ref.clone(),
        })?;
    let name = payload_name(node).ok_or_else(|| CalculationIssue::InvalidScope {
        reason: format!("{node_ref} has no bindable payload name"),
    })?;
    match Symbol::new(name) {
        Ok(_) if name != CALCULATION_CALENDAR_SYMBOL => Ok(name.to_owned()),
        _ => Err(CalculationIssue::UnbindablePayloadName {
            node_ref: node_ref.clone(),
            name: name.to_owned(),
        }),
    }
}

/// A Root binding named by the exact payload name, only when it is already a valid Symbol.
pub fn default_root_binding(
    graph: &Graph,
    node_ref: &Id,
) -> Result<CalculationScopeBinding, CalculationIssue> {
    Ok(CalculationScopeBinding {
        node_ref: node_ref.clone(),
        symbol: default_symbol(graph, node_ref)?,
        exposure: CalculationBindingExposure::Root,
    })
}

/// A Field binding named by the exact Attribute name, only when it is already a valid Symbol.
pub fn default_field_binding(
    graph: &Graph,
    attribute_ref: &Id,
    owner_ref: &Id,
) -> Result<CalculationScopeBinding, CalculationIssue> {
    Ok(CalculationScopeBinding {
        node_ref: attribute_ref.clone(),
        symbol: default_symbol(graph, attribute_ref)?,
        exposure: CalculationBindingExposure::Field {
            owner_ref: owner_ref.clone(),
        },
    })
}

/// Qualifies an Accepted Calculation. An S2.6 origin is the authoritative replay scope (an
/// override for such a node is rejected); a calculation without that origin needs an explicit
/// override, and without one its scope is unavailable rather than guessed.
pub fn qualify_accepted_calculation(
    graph: &Graph,
    calculation_ref: &Id,
    scope_override: Option<&CalculationScope>,
) -> Result<AcceptedCalculationQualification, CalculationError> {
    Ok(
        plumb_validation::calculation_analysis::qualify_accepted_calculation(
            graph,
            calculation_ref,
            scope_override,
        )?,
    )
}

/// Qualifies every Accepted Calculation (with explicit overrides for legacy ones) and
/// reports the dependency cycles among them; unavailable scopes are never guessed.
pub fn analyze_calculation_cycles(
    graph: &Graph,
    overrides: &BTreeMap<Id, CalculationScope>,
) -> Result<CalculationCycleAnalysis, CalculationError> {
    Ok(plumb_validation::calculation_analysis::analyze_calculation_cycles(graph, overrides)?)
}

// ============================================================================ evaluation

/// Why a prepared calculation could not be evaluated.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CalculationEvalError {
    #[error("evaluation: {0}")]
    Eval(#[from] EvalError),
    #[error("the caller's value environment already binds the reserved root calendar")]
    ReservedBindingConflict,
    #[error("rounding: {reason}")]
    Rounding { reason: String },
    #[error("result scale {scale} exceeds the declared Decimal({declared})")]
    ResultScaleExceeded { scale: u32, declared: u8 },
}

/// The caller's values plus the reserved calendar Ref (`type_ref` and `value_ref` =
/// calendar_ref) when the calculation has a calendar: exactly the environment an evaluation sees.
fn evaluation_values(
    prepared: &PreparedCalculation,
    values: &ValueEnv,
) -> Result<ValueEnv, CalculationEvalError> {
    let mut values = values.clone();
    if let Some(calendar) = &prepared.calendar_ref {
        let reserved = Symbol::new(CALCULATION_CALENDAR_SYMBOL)
            .unwrap_or_else(|_| unreachable!("calendar is a valid symbol"));
        let value = Value::Ref(RefValue {
            type_ref: calendar.clone(),
            value_ref: calendar.clone(),
            fields: BTreeMap::new(),
        });
        values
            .bind(reserved, value)
            .map_err(|_| CalculationEvalError::ReservedBindingConflict)?;
    }
    Ok(values)
}

/// Evaluates a prepared calculation with the S2.5 evaluator, injecting the reserved calendar
/// Ref (`type_ref` and `value_ref` = calendar_ref) and applying only final rounding.
pub fn evaluate_calculation(
    prepared: &PreparedCalculation,
    values: &ValueEnv,
    calendars: &dyn CalendarProvider,
    predicates: &dyn PredicateProvider,
) -> Result<Value, CalculationEvalError> {
    let values = evaluation_values(prepared, values)?;
    let value = eval(
        &prepared.ast,
        &EvalContext {
            types: &prepared.type_env,
            values: &values,
            calendars,
            predicates,
        },
    )?;
    // H0.10: round per the declared rounding, then the declared Decimal(n) scale check (for
    // scalar and Quantity results alike), then the unit check.
    let declared = DeclaredResult {
        ty: prepared.expected_ty.clone(),
        scale: prepared.declared_scale,
    };
    check_result(value, &declared, prepared.rounding.as_ref()).map_err(|e| match e {
        ResultCheckError::Rounding(e) => CalculationEvalError::Rounding {
            reason: e.to_string(),
        },
        ResultCheckError::RoundingNotApplicable => CalculationEvalError::Rounding {
            reason: e.to_string(),
        },
        ResultCheckError::ScaleExceeded { scale, declared } => {
            CalculationEvalError::ResultScaleExceeded { scale, declared }
        }
        ResultCheckError::UnitMismatch { .. } | ResultCheckError::TypeMismatch { .. } => {
            CalculationEvalError::Eval(EvalError::ValueTypeMismatch {
                context: e.to_string(),
            })
        }
    })
}

// ============================================================================ request

/// One target Requirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculationRequirementContext {
    pub requirement_ref: Id,
    pub status: ElementStatus,
    pub statement: String,
}

/// One available PlumbExpr input symbol with its type metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculationInputContext {
    pub node_ref: Id,
    pub symbol: String,
    pub exposure: CalculationBindingExposure,
    pub node_type: String,
    pub value_type: Option<String>,
    pub unit: Option<String>,
    pub enum_values: Option<Vec<String>>,
}

/// One Accepted Calendar inference may select; it is never a symbol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculationCalendarContext {
    pub calendar_ref: Id,
    pub time_zone: String,
    pub region: Option<String>,
}

/// The exact calculation inference context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculationContext {
    pub version: u32,
    pub project_id: Id,
    pub requirements: Vec<CalculationRequirementContext>,
    pub available_inputs: Vec<CalculationInputContext>,
    pub calendars: Vec<CalculationCalendarContext>,
}

impl CalculationContext {
    /// Generic SHA-256 of the RFC 8785 canonical JSON of the context.
    pub fn content_hash(&self) -> Result<Hash, CalculationError> {
        Ok(Hash::content_sha256(&to_canonical_json(self)?))
    }

    fn requirement(&self, id: &Id) -> Option<&CalculationRequirementContext> {
        self.requirements
            .binary_search_by(|r| r.requirement_ref.cmp(id))
            .ok()
            .map(|i| &self.requirements[i])
    }

    fn input_refs(&self) -> Vec<Id> {
        let refs: BTreeSet<&Id> = self
            .requirements
            .iter()
            .map(|r| &r.requirement_ref)
            .chain(self.available_inputs.iter().map(|i| &i.node_ref))
            .chain(self.calendars.iter().map(|c| &c.calendar_ref))
            .collect();
        refs.into_iter().cloned().collect()
    }
}

/// A calculation `InferenceRequest` with its exact context and validated scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculationRequest {
    pub request: InferenceRequest,
    pub context: CalculationContext,
    pub scope: CalculationScope,
}

fn context_of(
    graph: &Graph,
    target_requirement_refs: &[Id],
    scope: &CalculationScope,
) -> Result<(CalculationContext, CalculationScope), CalculationError> {
    let validated = validate_calculation_scope(graph, scope).map_err(CalculationError::Scope)?;
    let mut targets = BTreeSet::new();
    let mut requirements = Vec::new();
    for id in target_requirement_refs {
        if !targets.insert(id) {
            return Err(invalid_input(format!("duplicate target requirement {id}")));
        }
    }
    if targets.is_empty() {
        return Err(invalid_input("no target requirement"));
    }
    for id in targets {
        let node = graph
            .node(id)
            .ok_or_else(|| invalid_input(format!("requirement {id} does not exist")))?;
        let NodePayload::Requirement(r) = &node.payload else {
            return Err(invalid_input(format!("{id} is not a Requirement")));
        };
        if !matches!(
            node.status,
            ElementStatus::Proposed | ElementStatus::Accepted
        ) {
            return Err(invalid_input(format!(
                "requirement {id} is not Proposed or Accepted"
            )));
        }
        requirements.push(CalculationRequirementContext {
            requirement_ref: id.clone(),
            status: node.status,
            statement: r.statement.clone(),
        });
    }
    let mut available_inputs = Vec::new();
    for binding in &validated.bindings {
        let node = graph
            .node(&binding.node_ref)
            .ok_or_else(|| invalid_input(format!("{} does not exist", binding.node_ref)))?;
        let (value_type, unit, enum_values) = match &node.payload {
            NodePayload::Attribute(a) => (
                Some(a.value_type.clone()),
                a.unit.clone(),
                a.enum_values.clone(),
            ),
            NodePayload::Calculation(c) => (Some(c.result_type.clone()), c.unit.clone(), None),
            _ => (None, None, None),
        };
        available_inputs.push(CalculationInputContext {
            node_ref: binding.node_ref.clone(),
            symbol: binding.symbol.clone(),
            exposure: binding.exposure.clone(),
            node_type: node.payload.node_type().as_str().to_owned(),
            value_type,
            unit,
            enum_values,
        });
    }
    let mut calendar_refs = scope.calendar_refs.clone();
    calendar_refs.sort();
    let mut calendars = Vec::new();
    for calendar_ref in &calendar_refs {
        let Some(NodePayload::Calendar(c)) = graph.node(calendar_ref).map(|n| &n.payload) else {
            return Err(invalid_input(format!("{calendar_ref} is not a Calendar")));
        };
        calendars.push(CalculationCalendarContext {
            calendar_ref: calendar_ref.clone(),
            time_zone: c.time_zone.clone(),
            region: c.region.clone(),
        });
    }
    let context = CalculationContext {
        version: CALCULATION_CONTEXT_VERSION,
        project_id: graph.project_id().clone(),
        requirements,
        available_inputs,
        calendars,
    };
    let canonical = CalculationScope {
        bindings: validated.bindings,
        calendar_refs,
    };
    Ok((context, canonical))
}

fn context_evidence(graph: &Graph, context: &CalculationContext) -> Vec<Id> {
    let refs: BTreeSet<Id> = context
        .input_refs()
        .iter()
        .filter_map(|id| graph.node(id))
        .flat_map(|n| n.evidence.iter().map(|e| e.as_id().clone()))
        .collect();
    refs.into_iter().collect()
}

/// Builds the S2 calculation request over explicit target Requirements and an explicit scope.
pub fn build_calculation_request(
    graph: &Graph,
    target_requirement_refs: &[Id],
    scope: &CalculationScope,
    provider_policy: ProviderPolicy,
) -> Result<CalculationRequest, CalculationError> {
    let (context, scope) = context_of(graph, target_requirement_refs, scope)?;
    let request = InferenceRequest::new(
        CALCULATION_STAGE,
        CALCULATION_TASK_KIND.to_owned(),
        context.input_refs(),
        context_evidence(graph, &context),
        context.content_hash()?,
        Hash::content_sha256(PROMPT_TEMPLATE),
        Hash::content_sha256(SCHEMA_SOURCE.as_bytes()),
        provider_policy,
    )?;
    Ok(CalculationRequest {
        request,
        context,
        scope,
    })
}

impl CalculationRequest {
    /// The exact acquisition material of this request (Hotfix 066): the RFC 8785 canonical
    /// context and the committed prompt and schema bytes, bound to the request. Pure: no files,
    /// environment, clock or network.
    pub fn inference_material(&self) -> Result<InferenceRequestMaterial, CalculationError> {
        Ok(InferenceRequestMaterial::new(
            &self.request,
            to_canonical_json(&self.context)?,
            PROMPT_TEMPLATE.to_vec(),
            SCHEMA_SOURCE.as_bytes().to_vec(),
        )?)
    }

    /// The request is bound to its context, the committed prompt and schema.
    pub fn validate(&self) -> Result<(), CalculationError> {
        let r = &self.request;
        r.validate()
            .map_err(|e| invalid_input(format!("invalid inference request: {e}")))?;
        let ok = r.stage == CALCULATION_STAGE
            && r.task_kind == CALCULATION_TASK_KIND
            && r.input_refs == self.context.input_refs()
            && r.context_hash == self.context.content_hash()?
            && r.prompt_template_hash == Hash::content_sha256(PROMPT_TEMPLATE)
            && r.schema_hash == Hash::content_sha256(SCHEMA_SOURCE.as_bytes())
            && self.context.version == CALCULATION_CONTEXT_VERSION;
        if ok {
            Ok(())
        } else {
            Err(invalid_input(
                "request does not match its context, prompt or schema",
            ))
        }
    }
}

// ============================================================================ analysis

/// An already-acquired calculation artifact and the caller's provenance ref for it.
#[derive(Debug, Clone)]
pub struct CalculationInference<'a> {
    pub artifact: &'a InferenceArtifact,
    pub derivation_ref: DerivationRef,
}

/// Who owns the Proposed Calculation nodes and when, supplied by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalculationAudit {
    pub created_by: Id,
    pub created_at: Timestamp,
}

/// What happened to one inferred candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalculationDisposition {
    Proposed { proposal_ref: Id },
    Ineligible,
    IdempotentReplay,
    ExistingEquivalent,
    ExistingConflict,
}

/// One inferred candidate with its deterministic identity and qualification.
#[derive(Debug, Clone)]
pub struct CalculationCandidateAnalysis {
    pub calculation_ref: Id,
    pub name: String,
    pub qualification: CalculationQualification,
    pub disposition: CalculationDisposition,
}

/// Everything one analysis produced; nothing has been applied or persisted.
#[derive(Debug, Clone)]
pub struct CalculationAnalysisResult {
    pub candidates: Vec<CalculationCandidateAnalysis>,
    pub proposals: Vec<Proposal>,
    pub issues: Vec<CalculationIssue>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CalculationOutput {
    version: u32,
    calculations: Vec<OutputCalculation>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputCalculation {
    name_range: CalculationGroundedRange,
    expression: String,
    expression_evidence: Vec<CalculationGroundedRange>,
    result_type: String,
    unit: Option<String>,
    rounding: Option<String>,
    calendar_ref: Option<Id>,
}

fn compile_schema() -> Result<JSONSchema, CalculationError> {
    let schema: JsonValue =
        serde_json::from_str(SCHEMA_SOURCE).map_err(|e| CalculationError::SchemaCompilation {
            reason: format!("schema is not JSON: {e}"),
        })?;
    JSONSchema::options()
        .with_draft(Draft::Draft202012)
        .compile(&schema)
        .map_err(|e| CalculationError::SchemaCompilation {
            reason: e.to_string(),
        })
}

fn grounding_issue(g: &CalculationGroundedRange, reason: &str) -> CalculationIssue {
    CalculationIssue::InvalidGrounding {
        reason: format!("{} {}..{}: {reason}", g.requirement_ref, g.start, g.end),
    }
}

fn grounded<'c>(
    context: &'c CalculationContext,
    g: &CalculationGroundedRange,
) -> Result<&'c str, CalculationIssue> {
    let requirement = context
        .requirement(&g.requirement_ref)
        .ok_or_else(|| grounding_issue(g, "not a target requirement"))?;
    let text = &requirement.statement;
    let (s, e) = match (usize::try_from(g.start), usize::try_from(g.end)) {
        (Ok(s), Ok(e)) if s < e && e <= text.len() => (s, e),
        _ => return Err(grounding_issue(g, "not a non-empty range of the statement")),
    };
    if !text.is_char_boundary(s) || !text.is_char_boundary(e) {
        return Err(grounding_issue(g, "not on UTF-8 character boundaries"));
    }
    Ok(&text[s..e])
}

/// The grounded name slice: exactly the persisted range, without surrounding whitespace or
/// control characters (H0.10, Fable #109; the state and domain producers' rule). Nothing is
/// trimmed, so identity always uses the persisted range.
fn grounded_name<'c>(
    context: &'c CalculationContext,
    g: &CalculationGroundedRange,
) -> Result<&'c str, CalculationIssue> {
    let slice = grounded(context, g)?;
    if slice.trim() != slice || slice.chars().any(char::is_control) {
        return Err(grounding_issue(
            g,
            "slice has surrounding whitespace or control characters",
        ));
    }
    Ok(slice)
}

/// The per-candidate artifact contract (any-domain tolerance): a grounded name, grounded
/// expression evidence (an exact duplicate range is ignored with an issue) and an allowed
/// calendar. A violation drops only this candidate, reported as the returned issue.
fn validated_candidate(
    context: &CalculationContext,
    scope: &CalculationScope,
    candidate: &OutputCalculation,
    issues: &mut Vec<CalculationIssue>,
) -> Result<(String, Vec<CalculationGroundedRange>), CalculationIssue> {
    let name = grounded_name(context, &candidate.name_range)?.to_owned();
    let mut evidence = candidate.expression_evidence.clone();
    for range in &evidence {
        grounded(context, range)?;
    }
    evidence.sort();
    let before = evidence.len();
    evidence.dedup();
    if evidence.len() != before {
        issues.push(CalculationIssue::InvalidGrounding {
            reason: format!("calculation {name:?}: duplicate expression evidence range ignored"),
        });
    }
    if let Some(calendar) = &candidate.calendar_ref {
        if !scope.calendar_refs.contains(calendar) {
            return Err(CalculationIssue::CalendarUnresolved {
                calendar_ref: calendar.clone(),
            });
        }
    }
    Ok((name, evidence))
}

/// `calculation:<16 hex of SHA-256(RFC 8785 {project_id, name, node_type: calculation})>`.
fn calculation_id(project_id: &Id, name: &str) -> Result<Id, CoreError> {
    let body = json!({"project_id": project_id, "name": name, "node_type": "calculation"});
    let digest = Hash::content_sha256(&to_canonical_json(&body)?);
    let hex = &digest.as_str()["sha256:".len()..];
    format!("calculation:{}", &hex[..16]).parse()
}

/// Analyzes a supplied calculation inference against its request. Absent inference yields
/// only InferenceUnavailable; malformed inference is an error.
pub fn analyze_calculations(
    graph: &Graph,
    request: &CalculationRequest,
    inference: Option<CalculationInference<'_>>,
    audit: &CalculationAudit,
) -> Result<CalculationAnalysisResult, CalculationError> {
    request.validate()?;
    let targets: Vec<Id> = request
        .context
        .requirements
        .iter()
        .map(|r| r.requirement_ref.clone())
        .collect();
    let (context, scope) = context_of(graph, &targets, &request.scope)?;
    if context != request.context || scope != request.scope {
        return Err(invalid_input(
            "graph does not match the calculation request context",
        ));
    }
    if request.request.evidence_refs != context_evidence(graph, &context) {
        return Err(invalid_input(
            "evidence_refs differ from the context evidence",
        ));
    }
    let Some(inference) = inference else {
        return Ok(CalculationAnalysisResult {
            candidates: Vec::new(),
            proposals: Vec::new(),
            issues: vec![CalculationIssue::InferenceUnavailable],
        });
    };
    inference
        .artifact
        .validate_for(&request.request)
        .map_err(|e| CalculationError::InvalidInferenceArtifact {
            reason: e.to_string(),
        })?;
    let output = inference.artifact.validated_output.as_value();
    if let Err(errors) = compile_schema()?.validate(output) {
        let mut messages: Vec<String> = errors
            .map(|e| format!("{e} at {}", e.instance_path))
            .collect();
        messages.sort();
        return Err(CalculationError::SchemaInvalid {
            reason: messages.join("; "),
        });
    }
    let decoded: CalculationOutput =
        serde_json::from_value(output.clone()).map_err(|e| CalculationError::SchemaInvalid {
            reason: e.to_string(),
        })?;
    if decoded.version != CALCULATION_OUTPUT_VERSION {
        return Err(CalculationError::SchemaInvalid {
            reason: format!("output version {} is not 1", decoded.version),
        });
    }
    let base = graph.semantic_hash()?;
    let extension: ExtensionKey = CALCULATION_ORIGIN_EXTENSION
        .parse()
        .map_err(invalid_proposal)?;
    let mut candidates = Vec::new();
    let mut proposals = Vec::new();
    let mut issues = Vec::new();
    let mut seen = BTreeSet::new();
    for candidate in decoded.calculations {
        // Grounding and calendar selection are part of the artifact contract; a candidate
        // that breaks it is dropped with an issue and its siblings continue.
        let (name, evidence) = match validated_candidate(&context, &scope, &candidate, &mut issues)
        {
            Ok(valid) => valid,
            Err(issue) => {
                issues.push(issue);
                continue;
            }
        };
        let calculation_ref = calculation_id(graph.project_id(), &name)?;
        // The first candidate of an identity is analysed; a later one is dropped.
        if !seen.insert(calculation_ref.clone()) {
            issues.push(CalculationIssue::InvalidName {
                reason: format!("duplicate calculation candidate {calculation_ref} dropped"),
            });
            continue;
        }
        let draft = CalculationDraft {
            expression: &candidate.expression,
            result_type: &candidate.result_type,
            unit: candidate.unit.as_deref(),
            rounding: candidate.rounding.as_deref(),
            calendar_ref: candidate.calendar_ref.as_ref(),
        };
        let mut qualification = qualify_calculation(
            graph,
            &draft,
            validate_calculation_bindings(graph, &scope.bindings),
        );
        let mut disposition = CalculationDisposition::Ineligible;
        if qualification.is_qualified() {
            let payload = NodePayload::Calculation(Calculation {
                name: name.clone(),
                expression: qualification
                    .canonical_expression
                    .clone()
                    .unwrap_or_default(),
                result_type: candidate.result_type.clone(),
                unit: candidate.unit.clone(),
                rounding: candidate.rounding.clone(),
                calendar_ref: candidate.calendar_ref.clone(),
                examples: None,
            });
            let origin = serde_json::to_value(CalculationOrigin {
                name_range: candidate.name_range.clone(),
                expression_evidence: evidence.clone(),
                used_bindings: qualification.used_bindings.clone(),
            })
            .map_err(invalid_proposal)?;
            match graph.node(&calculation_ref) {
                Some(existing) => {
                    let same_payload = existing.payload == payload;
                    let own_output = same_owned_calculation(&existing.payload, &payload);
                    let same_origin = existing
                        .extensions
                        .iter()
                        .find(|(k, _)| k.as_str() == CALCULATION_ORIGIN_EXTENSION)
                        .is_some_and(|(_, v)| *v == origin);
                    disposition = match existing.status {
                        ElementStatus::Proposed if same_payload && same_origin => {
                            CalculationDisposition::IdempotentReplay
                        }
                        ElementStatus::Accepted if same_payload || own_output => {
                            CalculationDisposition::ExistingEquivalent
                        }
                        _ => {
                            qualification.issues.push(
                                CalculationIssue::ExistingCalculationConflict {
                                    node_ref: calculation_ref.clone(),
                                },
                            );
                            CalculationDisposition::ExistingConflict
                        }
                    };
                }
                None => {
                    let mut groundings = vec![&candidate.name_range];
                    groundings.extend(evidence.iter());
                    let node_evidence: Vec<EvidenceRef> = groundings
                        .iter()
                        .filter_map(|g| graph.node(&g.requirement_ref))
                        .flat_map(|n| n.evidence.iter().cloned())
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect();
                    let node = Node {
                        id: calculation_ref.clone(),
                        revision: 1,
                        status: ElementStatus::Proposed,
                        payload,
                        evidence: node_evidence.clone(),
                        derivations: Vec::new(),
                        standards: Vec::new(),
                        tags: BTreeSet::new(),
                        extensions: BTreeMap::from([(extension.clone(), origin)]),
                        audit: AuditMeta::new(
                            audit.created_by.clone(),
                            audit.created_at,
                            None,
                            None,
                        )
                        .map_err(invalid_proposal)?,
                    };
                    node.validate().map_err(invalid_proposal)?;
                    let proposal = Proposal::new(
                        CALCULATION_STAGE,
                        PatchSet {
                            base_semantic_hash: base.clone(),
                            patch: SemanticPatch::AddNode { node },
                        },
                        node_evidence,
                        vec![inference.derivation_ref.clone()],
                        ProposalMateriality::Semantic,
                        AcceptancePolicy::HumanConfirm,
                        None,
                    )
                    .map_err(invalid_proposal)?;
                    // In-memory dry validation; the candidate graph is discarded.
                    apply_patch(graph, &proposal.patch_set)
                        .map_err(|e| invalid_proposal(format!("proposal does not apply: {e}")))?;
                    disposition = CalculationDisposition::Proposed {
                        proposal_ref: proposal.id.clone(),
                    };
                    proposals.push(proposal);
                }
            }
        }
        candidates.push(CalculationCandidateAnalysis {
            calculation_ref,
            name,
            qualification,
            disposition,
        });
    }
    candidates.sort_by(|a, b| a.calculation_ref.cmp(&b.calculation_ref));
    proposals.sort_by(|a, b| a.id.cmp(&b.id));
    issues.sort();
    issues.dedup();
    Ok(CalculationAnalysisResult {
        candidates,
        proposals,
        issues,
    })
}

// ============================================================================ examples (Hotfix 065)

/// An exact, non-recursive calculation value: the wire form of the API `CalculationValue`.
/// Decimal, duration and quantity magnitudes are exact decimal strings, never binary floats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExactValue {
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

/// Why a calculation example cannot be produced.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CalculationExampleError {
    #[error(transparent)]
    Core(#[from] CoreError),
    /// The target does not exist or is not a Calculation.
    #[error("{0} is not a Calculation")]
    NotACalculation(Id),
    /// The Calculation is not Accepted.
    #[error("calculation {0} is not Accepted")]
    NotAccepted(Id),
    /// No replayable S2.6 scope exists; none is guessed.
    #[error("calculation {0} has no replayable scope")]
    ScopeUnavailable(Id),
    /// An input is malformed or is not a root input of the qualified calculation.
    #[error("invalid input {name}: {reason}")]
    InvalidInput { name: String, reason: String },
    /// The calculation does not qualify, an input has the wrong type or is missing, or the
    /// evaluation fails.
    #[error("calculation evaluation: {reason}")]
    Evaluation { reason: String },
    /// The result is not representable as an exact value (a reference or a list).
    #[error("the result is not representable: {reason}")]
    Unrepresentable { reason: String },
}

/// One evaluated example.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalculationExample {
    /// The persisted expression.
    pub expr: String,
    /// The S2.5 example renderer output.
    pub rendered_example: String,
    pub value: ExactValue,
}

fn exact_decimal(name: &str, text: &str) -> Result<Decimal, CalculationExampleError> {
    let invalid = || CalculationExampleError::InvalidInput {
        name: name.to_owned(),
        reason: format!("{text:?} is not an exact decimal"),
    };
    let value: Decimal = text.parse().map_err(|_| invalid())?;
    if value.to_string() == text {
        Ok(value)
    } else {
        Err(invalid())
    }
}

const DATE_FORMAT: &[time::format_description::FormatItem<'static>] =
    time::macros::format_description!("[year]-[month]-[day]");

/// Converts an exact input value to the PlumbExpr value it denotes; every string form must be
/// the exact canonical spelling of its value.
pub fn exact_to_expr(name: &str, value: &ExactValue) -> Result<Value, CalculationExampleError> {
    let invalid = |reason: String| CalculationExampleError::InvalidInput {
        name: name.to_owned(),
        reason,
    };
    let unit = |text: &str| {
        plumb_expr::Unit::new(text).map_err(|_| invalid(format!("{text:?} is not a unit")))
    };
    Ok(match value {
        ExactValue::Integer { value } => Value::Int(*value),
        ExactValue::Decimal { value } => Value::Decimal(exact_decimal(name, value)?),
        ExactValue::Boolean { value } => Value::Bool(*value),
        ExactValue::String { value } => Value::String(value.clone()),
        ExactValue::Date { value } => {
            let date = time::Date::parse(value, DATE_FORMAT)
                .map_err(|_| invalid(format!("{value:?} is not a date")))?;
            let canonical = date
                .format(DATE_FORMAT)
                .map_err(|_| invalid(format!("{value:?} is not a date")))?;
            if &canonical != value {
                return Err(invalid(format!("{value:?} is not a canonical date")));
            }
            Value::Date(date)
        }
        ExactValue::DateTime { value } => {
            let at: Timestamp = value
                .parse()
                .map_err(|_| invalid(format!("{value:?} is not a timestamp")))?;
            if &at.to_string() != value {
                return Err(invalid(format!("{value:?} is not a canonical timestamp")));
            }
            Value::DateTime(at)
        }
        ExactValue::Duration { value, unit: u } => Value::Duration {
            value: exact_decimal(name, value)?,
            unit: unit(u)?,
        },
        ExactValue::Quantity { value, unit: u } => Value::Quantity {
            value: exact_decimal(name, value)?,
            unit: unit(u)?,
        },
        ExactValue::Enum { enum_ref, member } => Value::Enum {
            enum_ref: enum_ref
                .parse()
                .map_err(|_| invalid(format!("{enum_ref:?} is not an ID")))?,
            member: Symbol::new(member)
                .map_err(|_| invalid(format!("{member:?} is not a member symbol")))?,
        },
    })
}

/// Converts a PlumbExpr value to its exact form; references and lists are unrepresentable.
pub fn expr_to_exact(value: &Value) -> Result<ExactValue, CalculationExampleError> {
    let unrepresentable = |kind: &str| CalculationExampleError::Unrepresentable {
        reason: format!("a {kind} value has no exact calculation form"),
    };
    Ok(match value {
        Value::Int(i) => ExactValue::Integer { value: *i },
        Value::Decimal(d) => ExactValue::Decimal {
            value: d.to_string(),
        },
        Value::Bool(b) => ExactValue::Boolean { value: *b },
        Value::String(s) => ExactValue::String { value: s.clone() },
        Value::Date(d) => ExactValue::Date {
            value: d.format(DATE_FORMAT).map_err(|_| unrepresentable("date"))?,
        },
        Value::DateTime(t) => ExactValue::DateTime {
            value: t.to_string(),
        },
        Value::Duration { value, unit } => ExactValue::Duration {
            value: value.to_string(),
            unit: unit.as_str().to_owned(),
        },
        Value::Quantity { value, unit } => ExactValue::Quantity {
            value: value.to_string(),
            unit: unit.as_str().to_owned(),
        },
        Value::Enum { enum_ref, member } => ExactValue::Enum {
            enum_ref: enum_ref.to_string(),
            member: member.as_str().to_owned(),
        },
        Value::Ref(_) => return Err(unrepresentable("reference")),
        Value::List(_) => return Err(unrepresentable("list")),
    })
}

/// Evaluates an example of the Accepted Calculation `calculation_ref` (Hotfix 065): qualified
/// only through its replayable S2.6 scope (none is guessed), with `inputs` bound to exact root
/// input symbols of that scope, evaluated with the injected providers and rendered by the S2.5
/// example renderer. No input is ignored.
pub fn calculation_example(
    graph: &Graph,
    calculation_ref: &Id,
    inputs: &BTreeMap<String, ExactValue>,
    calendars: &dyn CalendarProvider,
    predicates: &dyn PredicateProvider,
) -> Result<CalculationExample, CalculationExampleError> {
    let node = graph
        .node(calculation_ref)
        .ok_or_else(|| CalculationExampleError::NotACalculation(calculation_ref.clone()))?;
    let NodePayload::Calculation(calculation) = &node.payload else {
        return Err(CalculationExampleError::NotACalculation(
            calculation_ref.clone(),
        ));
    };
    if node.status != ElementStatus::Accepted {
        return Err(CalculationExampleError::NotAccepted(
            calculation_ref.clone(),
        ));
    }
    let evaluation = |reason: String| CalculationExampleError::Evaluation { reason };
    let qualified = qualify_accepted_calculation(graph, calculation_ref, None)
        .map_err(|e| evaluation(e.to_string()))?;
    let qualification = qualified.qualification;
    if qualification
        .issues
        .iter()
        .any(|issue| matches!(issue, CalculationIssue::ScopeUnavailable { .. }))
    {
        return Err(CalculationExampleError::ScopeUnavailable(
            calculation_ref.clone(),
        ));
    }
    let prepared = match (&qualification.prepared, qualification.issues.is_empty()) {
        (Some(prepared), true) => prepared,
        _ => {
            return Err(evaluation(format!(
                "calculation does not qualify: {:?}",
                qualification.issues
            )))
        }
    };
    let roots: BTreeSet<&str> = qualification
        .used_bindings
        .iter()
        .filter(|b| matches!(b.exposure, CalculationBindingExposure::Root))
        .map(|b| b.symbol.as_str())
        .collect();
    let mut env = ValueEnv::new();
    for (name, value) in inputs {
        if !roots.contains(name.as_str()) {
            return Err(CalculationExampleError::InvalidInput {
                name: name.clone(),
                reason: "not a root input of the calculation".to_owned(),
            });
        }
        let symbol = Symbol::new(name).map_err(|_| CalculationExampleError::InvalidInput {
            name: name.clone(),
            reason: "not a symbol".to_owned(),
        })?;
        env.bind(symbol, exact_to_expr(name, value)?).map_err(|e| {
            CalculationExampleError::InvalidInput {
                name: name.clone(),
                reason: e.to_string(),
            }
        })?;
    }
    let result = evaluate_calculation(prepared, &env, calendars, predicates)
        .map_err(|e| evaluation(e.to_string()))?;
    let value = expr_to_exact(&result)?;
    // Rendered against exactly the environment the evaluation saw.
    let rendered_env = evaluation_values(prepared, &env).map_err(|e| evaluation(e.to_string()))?;
    let rendered_example =
        plumb_expr::render_example(&prepared.ast, &prepared.type_env, &rendered_env, &result);
    Ok(CalculationExample {
        expr: calculation.expression.clone(),
        rendered_example,
        value,
    })
}

// ============================================================================ governed answers (Hotfix 083)

/// The prefix of the governed fractional-input semantic key.
pub const CALCULATION_FRACTION_KEY_PREFIX: &str = "calculation_fraction:";

/// The unresolved identifier a `working_days` inclusive-end argument carries before its governed
/// answer (Hotfix 083).
pub const UNRESOLVED_INCLUSIVE_END: &str = "inclusive_end";

const WORKING_DAYS_FUNCTION: &str = "working_days";

/// Whether an existing Calculation is this producer's output after legitimate enrichment
/// (architecture 33.5, Fable #105): `examples` are enrichable, and the expression may differ
/// only by the governed inclusive-end rewrite of the candidate's unresolved argument.
fn same_owned_calculation(existing: &NodePayload, candidate: &NodePayload) -> bool {
    let (NodePayload::Calculation(e), NodePayload::Calculation(c)) = (existing, candidate) else {
        return false;
    };
    let expression = e.expression == c.expression
        || [true, false].iter().any(|answer| {
            rewrite_working_days_inclusive_end(&c.expression, None, *answer).as_deref()
                == Ok(e.expression.as_str())
        });
    expression
        && e.name == c.name
        && e.result_type == c.result_type
        && e.unit == c.unit
        && e.rounding == c.rounding
        && e.calendar_ref == c.calendar_ref
}

/// `calculation_fraction:<64 hex of SHA-256(RFC 8785 {attribute_ref, calculation_ref})>`: the
/// governed `Calculation.examples` key of one fractional input, a function of the two exact IDs
/// only (Hotfix 083). Scenario boundaries reuse it as their `CalculationSemantic` key.
pub fn calculation_fraction_semantic_key(
    calculation_ref: &Id,
    attribute_ref: &Id,
) -> Result<String, CoreError> {
    let body = json!({
        "attribute_ref": attribute_ref.to_string(),
        "calculation_ref": calculation_ref.to_string(),
    });
    let digest = Hash::content_sha256(&to_canonical_json(&body)?);
    let hex = &digest.as_str()["sha256:".len()..];
    Ok(format!("{CALCULATION_FRACTION_KEY_PREFIX}{hex}"))
}

/// Why the inclusive-end argument of a calculation expression cannot be rewritten.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InclusiveEndRewriteError {
    #[error("the expression does not parse: {0}")]
    Parse(String),
    #[error("expected exactly one working_days call, found {0}")]
    WorkingDaysCalls(usize),
    #[error("the working_days call has no inclusive-end argument")]
    MissingInclusiveArgument,
    /// The argument is not the expected current material (unresolved identifier or the
    /// previously governed literal); `found` is its canonical text.
    #[error("the inclusive-end argument is {found}, not the expected current value")]
    UnexpectedInclusiveArgument { found: String },
}

fn working_days_call_count(ast: &plumb_expr::Ast) -> usize {
    use plumb_expr::Ast;
    match ast {
        Ast::Call { function, args } => {
            usize::from(function.as_str() == WORKING_DAYS_FUNCTION)
                + args.iter().map(working_days_call_count).sum::<usize>()
        }
        Ast::List(items) => items.iter().map(working_days_call_count).sum(),
        Ast::Unary { expr, .. } => working_days_call_count(expr),
        Ast::Binary { left, right, .. } => {
            working_days_call_count(left) + working_days_call_count(right)
        }
        Ast::Conditional {
            condition,
            then_expr,
            else_expr,
        } => {
            working_days_call_count(condition)
                + working_days_call_count(then_expr)
                + working_days_call_count(else_expr)
        }
        Ast::Literal(_) | Ast::Identifier(_) => 0,
    }
}

/// The arguments of the (single) `working_days` call, mutably.
fn working_days_args_mut(ast: &mut plumb_expr::Ast) -> Option<&mut Vec<plumb_expr::Ast>> {
    use plumb_expr::Ast;
    match ast {
        Ast::Call { function, args } => {
            if function.as_str() == WORKING_DAYS_FUNCTION {
                return Some(args);
            }
            args.iter_mut().find_map(working_days_args_mut)
        }
        Ast::List(items) => items.iter_mut().find_map(working_days_args_mut),
        Ast::Unary { expr, .. } => working_days_args_mut(expr),
        Ast::Binary { left, right, .. } => {
            working_days_args_mut(left).or_else(|| working_days_args_mut(right))
        }
        Ast::Conditional {
            condition,
            then_expr,
            else_expr,
        } => working_days_args_mut(condition)
            .or_else(|| working_days_args_mut(then_expr))
            .or_else(|| working_days_args_mut(else_expr)),
        Ast::Literal(_) | Ast::Identifier(_) => None,
    }
}

/// Replaces only the third argument of the single `working_days` call of `expression` with the
/// boolean literal `value` and returns the canonical unparse (Hotfix 083). The current argument
/// must be exactly `current`: `None` for the unresolved identifier [`UNRESOLVED_INCLUSIVE_END`]
/// (an initial answer), `Some(b)` for the previously governed literal `b` (a supersession).
/// Nothing else in the expression changes; there is no textual replacement.
pub fn rewrite_working_days_inclusive_end(
    expression: &str,
    current: Option<bool>,
    value: bool,
) -> Result<String, InclusiveEndRewriteError> {
    use plumb_expr::{Ast, Identifier, Literal};
    let mut ast = plumb_expr::parse(expression)
        .map_err(|e| InclusiveEndRewriteError::Parse(e.to_string()))?;
    let calls = working_days_call_count(&ast);
    if calls != 1 {
        return Err(InclusiveEndRewriteError::WorkingDaysCalls(calls));
    }
    let args =
        working_days_args_mut(&mut ast).ok_or(InclusiveEndRewriteError::WorkingDaysCalls(0))?;
    let argument = args
        .get_mut(2)
        .ok_or(InclusiveEndRewriteError::MissingInclusiveArgument)?;
    let expected = match current {
        None => Ast::Identifier(
            UNRESOLVED_INCLUSIVE_END
                .parse::<Identifier>()
                .map_err(|e| InclusiveEndRewriteError::Parse(e.to_string()))?,
        ),
        Some(old) => Ast::Literal(Literal::Bool(old)),
    };
    if *argument != expected {
        return Err(InclusiveEndRewriteError::UnexpectedInclusiveArgument {
            found: plumb_expr::unparse(argument),
        });
    }
    *argument = Ast::Literal(Literal::Bool(value));
    Ok(plumb_expr::unparse(&ast))
}

#[cfg(test)]
mod governed_answer_tests {
    use super::*;

    fn rid(s: &str) -> Id {
        s.parse().unwrap()
    }

    #[test]
    fn fraction_semantic_key_depends_only_on_the_two_ids() {
        let key = calculation_fraction_semantic_key(&rid("calc:days"), &rid("attr:part")).unwrap();
        // Independently: RFC 8785 of the two sorted fields, full SHA-256 hex.
        let canonical = br#"{"attribute_ref":"attr:part","calculation_ref":"calc:days"}"#;
        let hex = &Hash::content_sha256(canonical).as_str()["sha256:".len()..].to_owned();
        assert_eq!(key, format!("calculation_fraction:{hex}"));
        assert_eq!(hex.len(), 64);
        assert_eq!(
            calculation_fraction_semantic_key(&rid("calc:days"), &rid("attr:part")).unwrap(),
            key
        );
        assert_ne!(
            calculation_fraction_semantic_key(&rid("attr:part"), &rid("calc:days")).unwrap(),
            key
        );
        assert_ne!(
            calculation_fraction_semantic_key(&rid("calc:days"), &rid("attr:other")).unwrap(),
            key
        );
    }

    #[test]
    fn inclusive_end_rewrite_is_ast_local() {
        // Similar identifiers elsewhere must survive: only the third working_days argument moves.
        let expression = "if inclusive_end_note then working_days([begin, finish], calendar, inclusive_end) + inclusive_endx else working_days_total";
        let result = rewrite_working_days_inclusive_end(expression, None, true);
        // That expression has one working_days call (working_days_total is an identifier).
        let rewritten = result.unwrap();
        assert_eq!(
            plumb_expr::parse(&rewritten).unwrap(),
            plumb_expr::parse(
                "if inclusive_end_note then working_days([begin, finish], calendar, true) + inclusive_endx else working_days_total"
            )
            .unwrap()
        );
        assert!(rewritten.contains("inclusive_end_note") && rewritten.contains("inclusive_endx"));
        // A supersession from the governed literal.
        let back = rewrite_working_days_inclusive_end(&rewritten, Some(true), false).unwrap();
        assert_eq!(
            plumb_expr::parse(&back).unwrap(),
            plumb_expr::parse(
                "if inclusive_end_note then working_days([begin, finish], calendar, false) + inclusive_endx else working_days_total"
            )
            .unwrap()
        );
    }

    #[test]
    fn inclusive_end_rewrite_rejects_anything_else() {
        let rewrite =
            |e: &str, current: Option<bool>| rewrite_working_days_inclusive_end(e, current, true);
        assert_eq!(
            rewrite("1 + 1", None),
            Err(InclusiveEndRewriteError::WorkingDaysCalls(0))
        );
        assert_eq!(
            rewrite(
                "working_days([a, b], calendar, inclusive_end) + working_days([c, d], calendar, inclusive_end)",
                None
            ),
            Err(InclusiveEndRewriteError::WorkingDaysCalls(2))
        );
        assert_eq!(
            rewrite("working_days([a, b], calendar)", None),
            Err(InclusiveEndRewriteError::MissingInclusiveArgument)
        );
        assert!(matches!(
            rewrite("working_days([a, b], calendar, true)", None),
            Err(InclusiveEndRewriteError::UnexpectedInclusiveArgument { .. })
        ));
        assert!(matches!(
            rewrite("working_days([a, b], calendar, flag)", None),
            Err(InclusiveEndRewriteError::UnexpectedInclusiveArgument { .. })
        ));
        assert!(matches!(
            rewrite("working_days([a, b], calendar, not inclusive_end)", None),
            Err(InclusiveEndRewriteError::UnexpectedInclusiveArgument { .. })
        ));
        // A supersession needs the exact previously governed literal.
        assert!(matches!(
            rewrite("working_days([a, b], calendar, inclusive_end)", Some(false)),
            Err(InclusiveEndRewriteError::UnexpectedInclusiveArgument { .. })
        ));
        assert!(matches!(
            rewrite("working_days([a, b], calendar, true)", Some(false)),
            Err(InclusiveEndRewriteError::UnexpectedInclusiveArgument { .. })
        ));
        assert!(matches!(
            rewrite("working_days(", None),
            Err(InclusiveEndRewriteError::Parse(_))
        ));
    }
}
