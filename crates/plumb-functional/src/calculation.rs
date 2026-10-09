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
    eval, round_decimal, CalendarProvider, EvalContext, EvalError, PredicateProvider, RefValue,
    Symbol, Ty, Value, ValueEnv,
};
use plumb_inference::{InferenceArtifact, InferenceError, InferenceRequest, ProviderPolicy};
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
/// requests are errors; semantic problems of a candidate are [`CalculationIssue`]s.
#[derive(Debug, Error)]
pub enum CalculationError {
    #[error("invalid calculation input: {reason}")]
    InvalidInput { reason: String },
    #[error("invalid calculation scope: {0:?}")]
    Scope(CalculationIssue),
    #[error("inference request: {0}")]
    InferenceConstruction(#[from] InferenceError),
    #[error("invalid calculation inference artifact: {reason}")]
    InvalidInferenceArtifact { reason: String },
    #[error("calculation schema does not compile: {reason}")]
    SchemaCompilation { reason: String },
    #[error("calculation output is schema-invalid: {reason}")]
    SchemaInvalid { reason: String },
    #[error("invalid grounding {requirement_ref} {start}..{end}: {reason}")]
    InvalidGrounding {
        requirement_ref: Id,
        start: u64,
        end: u64,
        reason: String,
    },
    #[error("calendar {calendar_ref} is not an allowed calendar of the request")]
    CalendarNotAllowed { calendar_ref: Id },
    #[error("duplicate calculation candidate {calculation_ref}")]
    DuplicateCandidate { calculation_ref: Id },
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

/// Evaluates a prepared calculation with the S2.5 evaluator, injecting the reserved calendar
/// Ref (`type_ref` and `value_ref` = calendar_ref) and applying only final rounding.
pub fn evaluate_calculation(
    prepared: &PreparedCalculation,
    values: &ValueEnv,
    calendars: &dyn CalendarProvider,
    predicates: &dyn PredicateProvider,
) -> Result<Value, CalculationEvalError> {
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
    let value = eval(
        &prepared.ast,
        &EvalContext {
            types: &prepared.type_env,
            values: &values,
            calendars,
            predicates,
        },
    )?;
    let value = match (value, &prepared.expected_ty) {
        (Value::Int(i), Ty::Decimal(_)) => Value::Decimal(Decimal::from(i)),
        (v, _) => v,
    };
    let rounded = match (&prepared.rounding, value) {
        (None, v) => v,
        (Some(spec), Value::Decimal(d)) => {
            Value::Decimal(
                round_decimal(d, spec).map_err(|e| CalculationEvalError::Rounding {
                    reason: e.to_string(),
                })?,
            )
        }
        (Some(spec), Value::Quantity { value, unit }) => Value::Quantity {
            value: round_decimal(value, spec).map_err(|e| CalculationEvalError::Rounding {
                reason: e.to_string(),
            })?,
            unit,
        },
        (Some(_), _) => {
            return Err(CalculationEvalError::Rounding {
                reason: "rounding applies only to Decimal and Quantity results".into(),
            })
        }
    };
    if let (Value::Decimal(d), Ty::Decimal(declared)) = (&rounded, &prepared.expected_ty) {
        if d.scale() > u32::from(*declared) {
            return Err(CalculationEvalError::ResultScaleExceeded {
                scale: d.scale(),
                declared: *declared,
            });
        }
    }
    Ok(rounded)
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

fn grounded<'c>(
    context: &'c CalculationContext,
    g: &CalculationGroundedRange,
) -> Result<&'c str, CalculationError> {
    let invalid = |reason: &str| CalculationError::InvalidGrounding {
        requirement_ref: g.requirement_ref.clone(),
        start: g.start,
        end: g.end,
        reason: reason.to_owned(),
    };
    let requirement = context
        .requirement(&g.requirement_ref)
        .ok_or_else(|| invalid("not a target requirement"))?;
    let text = &requirement.statement;
    let (s, e) = match (usize::try_from(g.start), usize::try_from(g.end)) {
        (Ok(s), Ok(e)) if s < e && e <= text.len() => (s, e),
        _ => return Err(invalid("not a non-empty range of the statement")),
    };
    if !text.is_char_boundary(s) || !text.is_char_boundary(e) {
        return Err(invalid("not on UTF-8 character boundaries"));
    }
    Ok(&text[s..e])
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
    let mut seen = BTreeSet::new();
    for candidate in decoded.calculations {
        // Grounding and calendar selection are part of the artifact contract.
        let raw_name = grounded(&context, &candidate.name_range)?;
        let mut evidence = candidate.expression_evidence.clone();
        for range in &evidence {
            grounded(&context, range)?;
        }
        evidence.sort();
        if evidence.windows(2).any(|p| p[0] == p[1]) {
            return Err(CalculationError::InvalidGrounding {
                requirement_ref: evidence[0].requirement_ref.clone(),
                start: evidence[0].start,
                end: evidence[0].end,
                reason: "duplicate expression evidence range".into(),
            });
        }
        if let Some(calendar) = &candidate.calendar_ref {
            if !scope.calendar_refs.contains(calendar) {
                return Err(CalculationError::CalendarNotAllowed {
                    calendar_ref: calendar.clone(),
                });
            }
        }
        let name = raw_name.trim().to_owned();
        let calculation_ref = calculation_id(graph.project_id(), &name)?;
        if !seen.insert(calculation_ref.clone()) {
            return Err(CalculationError::DuplicateCandidate { calculation_ref });
        }
        let name_issue = if name.is_empty() {
            Some("the grounded name is empty")
        } else if name.chars().any(char::is_control) {
            Some("the grounded name contains control characters")
        } else {
            None
        };
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
        if let Some(reason) = name_issue {
            qualification.issues.push(CalculationIssue::InvalidName {
                reason: reason.to_owned(),
            });
            qualification.prepared = None;
        }
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
                    let same_origin = existing
                        .extensions
                        .iter()
                        .find(|(k, _)| k.as_str() == CALCULATION_ORIGIN_EXTENSION)
                        .is_some_and(|(_, v)| *v == origin);
                    disposition = match existing.status {
                        ElementStatus::Proposed if same_payload && same_origin => {
                            CalculationDisposition::IdempotentReplay
                        }
                        ElementStatus::Accepted if same_payload => {
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
    Ok(CalculationAnalysisResult {
        candidates,
        proposals,
        issues: Vec::new(),
    })
}
