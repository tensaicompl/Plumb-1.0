//! Gate evaluation and durable GateReport persistence of A0.4 (Hotfix 067): the I0-F4
//! validation contexts, the `ready` chain and the store-v3 report rows.
//!
//! R0.4 (architecture §21 "Gate input assembly"): the F1, F2 and F4 supplemental inputs are
//! assembled per evaluation from the exact loaded revision (graph class), the latest completed
//! stage run's validated inference (durable class) and the explicit configuration (external
//! class only). The validation consumers (`with_f1_inputs`, `with_f2_inputs`,
//! `with_f4_inputs`) and every rule are unchanged.

use std::collections::{BTreeMap, BTreeSet};

use plumb_artifacts::{ArtifactKind, ArtifactStore};
use plumb_compiler::ArtifactInput;
use plumb_core::{to_canonical_json, FixedClock, GateId, Hash, Id, Timestamp};
use plumb_functional::{
    analyze_assumption_expiry, analyze_domain, analyze_lifecycle, analyze_requirement_duplicates,
    analyze_vocabulary, build_domain_request, build_lifecycle_request, build_vocabulary_request,
    compile_verification_obligations, decode_decision_table, normalize_vocabulary_term,
    validate_scenario, DomainAudit, DomainInference, LifecycleAudit, LifecycleInference,
    VerificationApplicabilityInputs, VerificationAudit, VerificationError, VocabularyAudit,
    VocabularyInference, PILOT_VOCABULARY_LANGUAGE,
};
use plumb_import::{
    build_evidence_manifest, evidence_manifest_artifact, SourceArtifactLinks,
    SOURCE_ARTIFACTS_EXTENSION,
};
use plumb_inference::{InferenceArtifact, InferenceRequest};
use plumb_psg::{AgentKind, ElementStatus, Graph, Node, NodePayload, NodeType};
use plumb_store::{GateReportRecord, LoadedRevision, NewGateReport};
use plumb_validation::calculation_analysis::calculation_origin_of;
use plumb_validation::evaluator::{
    F3DecisionArtifactInput, F3ValidationInputs, S3_RESOLUTION_KINDS,
};
use plumb_validation::{
    rules, scenario_specification_hash, EvaluatorRegistry, F1RequirementValidationInput,
    F1ValidationInputs, F1VocabularyDependency, F1VocabularyResolution, F2CalculationScopeOverride,
    F2DecisionTableInput, F2InvariantInput, F2ValidationInputs, F4ApplicabilityInput,
    F4ScenarioRunInput, F4ScenarioTypeCheckInput, F4ScenarioTypeStatus, F4ValidationInputs,
    GateReport, GateResult, GeneratedFinding, ValidationArtifactInput, ValidationContext,
    ValidationPolicy, ValidationRegistry, WAIVER_KIND,
};
use serde::Serialize;
use serde_json::{json, Value};

use crate::error::ApiError;
use crate::stage::acceptance::load_run;
use crate::stage::acquire::derivation_ref;
use crate::stage::recipe::{effective_targets, Phase, Recipe};
use crate::stage::scenario_execution::{current_run, error_name, link_key};
use crate::state::{A03Runtime, A04Runtime, ApiState};

/// 409 `E_GATE_INPUTS_MISSING`: a gate needs configured supplemental inputs that are absent.
pub(crate) fn inputs_missing(gate: GateId) -> ApiError {
    ApiError::conflict(
        "E_GATE_INPUTS_MISSING",
        format!("the {gate} gate needs configured supplemental inputs"),
    )
    .with_details(json!({ "gate": gate.to_string() }))
}

/// 409 `E_GATE_INPUTS_MISSING` naming the current in-scope elements whose required external
/// `field` entry is absent.
fn external_missing(gate: GateId, field: &str, elements: &BTreeSet<Id>) -> ApiError {
    ApiError::conflict(
        "E_GATE_INPUTS_MISSING",
        format!("the {gate} gate needs explicit {field} entries for current elements"),
    )
    .with_details(json!({ "gate": gate.to_string(), "field": field, "elements": elements }))
}

/// 409 `E_GATE_INPUTS_STALE`: external entries naming elements in a forbidden state.
fn external_stale(gate: GateId, field: &str, elements: &BTreeSet<Id>) -> ApiError {
    ApiError::conflict(
        "E_GATE_INPUTS_STALE",
        format!("configured {field} entries of the {gate} gate name elements in a forbidden state"),
    )
    .with_details(json!({ "gate": gate.to_string(), "field": field, "elements": elements }))
}

/// 409 `E_GATE_INPUTS_STALE`: the durable analysis material of `recipe` does not match the
/// current graph; re-running `recipe` is the ordinary product operation that repairs it.
fn durable_stale(gate: GateId, recipe: Recipe, reason: &str, request: &Hash) -> ApiError {
    ApiError::conflict(
        "E_GATE_INPUTS_STALE",
        format!(
            "the {gate} gate needs a current {} run; re-run the {} recipe",
            recipe.as_str(),
            recipe.as_str()
        ),
    )
    .with_details(json!({
        "gate": gate.to_string(),
        "recipe": recipe.as_str(),
        "reason": reason,
        "request": request,
    }))
}

/// 422 `E_STAGE_EVALUATION_FAILED` for a gate that cannot be evaluated.
fn evaluation_failed<E: std::fmt::Debug + std::fmt::Display>(gate: GateId, error: E) -> ApiError {
    ApiError::unprocessable(
        "E_STAGE_EVALUATION_FAILED",
        format!("the {gate} gate cannot be evaluated: {error}"),
    )
    .with_details(json!({ "gate": gate.to_string(), "error": error_name(&error) }))
}

/// Decodes a stored report: strict serde, canonical bytes, valid report and matching hash.
pub(crate) fn decode_report(record: &GateReportRecord) -> Result<GateReport, ApiError> {
    let corrupt = || ApiError::workflow_corrupt(&format!("gate report {}", record.revision_id));
    let report: GateReport = serde_json::from_str(&record.report_json).map_err(|_| corrupt())?;
    let canonical = to_canonical_json(&report).map_err(|_| corrupt())?;
    if canonical != record.report_json.as_bytes()
        || report.validate().is_err()
        || report.content_hash().map_err(|_| corrupt())? != record.report_hash
        || report.gate.to_string() != record.gate
    {
        return Err(corrupt());
    }
    Ok(report)
}

/// The evaluator registry with every NOW gate's evaluators.
fn registry(state: &ApiState) -> Result<EvaluatorRegistry, ApiError> {
    let metadata = ValidationRegistry::new(state.profile().clone())
        .map_err(|_| ApiError::internal("validation_registry"))?;
    let mut registry = EvaluatorRegistry::new(metadata);
    for register in [
        rules::register_i0_evaluators,
        rules::register_f1_evaluators,
        rules::register_f2_evaluators,
        rules::register_f3_evaluators,
        rules::register_f4_evaluators,
    ] {
        register(&mut registry).map_err(|_| ApiError::internal("validation_registry"))?;
    }
    Ok(registry)
}

/// The content hash of supplemental inputs.
fn input_hash<T: Serialize>(value: &T) -> Result<Hash, ApiError> {
    let bytes = to_canonical_json(value).map_err(|_| ApiError::internal("gate_inputs"))?;
    Ok(Hash::content_sha256(&bytes))
}

/// The I0 evidence material: the graph's EvidenceManifest artifact plus every stored original
/// and extracted artifact of each baseline SourceArtifact (a missing one stays absent).
fn i0_evidence(
    state: &ApiState,
    loaded: &LoadedRevision,
) -> Result<Vec<ValidationArtifactInput>, ApiError> {
    let graph = &loaded.graph;
    let mut out = Vec::new();
    if let Ok(manifest) = build_evidence_manifest(graph) {
        if let Ok(artifact) = evidence_manifest_artifact(&manifest, loaded.revision.created_at) {
            out.push(ValidationArtifactInput {
                hash: artifact.hash,
                kind: artifact.kind,
                media_type: artifact.media_type,
                bytes: artifact.bytes,
            });
        }
    }
    let key = SOURCE_ARTIFACTS_EXTENSION
        .parse()
        .map_err(|_| ApiError::internal("source_artifacts_extension"))?;
    let mut refs = Vec::new();
    for id in graph.node_ids_by_type(NodeType::SourceArtifact) {
        let Some(node) = graph.node(id) else { continue };
        let NodePayload::SourceArtifact(source) = &node.payload else {
            continue;
        };
        if !plumb_psg::is_baseline(node.status) {
            continue;
        }
        refs.push(source.content_hash.clone());
        if let Some(links) = node
            .extensions
            .get(&key)
            .and_then(|v| serde_json::from_value::<SourceArtifactLinks>(v.clone()).ok())
        {
            refs.push(links.extracted_ref);
        }
    }
    refs.sort();
    refs.dedup();
    for hash in refs {
        let stored = state.with_artifact_store(|store| store.get(&hash))?;
        if let Ok(artifact) = stored {
            if matches!(
                artifact.kind,
                ArtifactKind::SourceOriginal | ArtifactKind::SourceExtracted
            ) && !out.iter().any(|a| a.hash == artifact.hash)
            {
                out.push(ValidationArtifactInput {
                    hash: artifact.hash,
                    kind: artifact.kind,
                    media_type: artifact.media_type,
                    bytes: artifact.bytes,
                });
            }
        }
    }
    Ok(out)
}

/// F3 decision artifacts: exactly the Accepted current S3 and waiver decisions.
fn f3_decision_artifacts(
    state: &ApiState,
    loaded: &LoadedRevision,
) -> Result<Vec<F3DecisionArtifactInput>, ApiError> {
    let graph = &loaded.graph;
    let mut out = Vec::new();
    for id in graph.node_ids_by_type(NodeType::ResolutionDecision) {
        let Some(node) = graph.node(id) else { continue };
        let NodePayload::ResolutionDecision(decision) = &node.payload else {
            continue;
        };
        if node.status != ElementStatus::Accepted {
            continue;
        }
        let kind = decision.answer.get("kind").and_then(Value::as_str);
        if !kind.is_some_and(|k| S3_RESOLUTION_KINDS.contains(&k) || k == WAIVER_KIND) {
            continue;
        }
        let artifact = state
            .with_artifact_store(|store| store.get(&decision.patch_ref))?
            .map_err(|_| {
                ApiError::unprocessable(
                    "E_STAGE_EVALUATION_FAILED",
                    "a decision artifact is missing",
                )
                .with_details(json!({ "gate": "F3", "decision": node.id }))
            })?;
        out.push(F3DecisionArtifactInput {
            decision_ref: node.id.clone(),
            hash: decision.patch_ref.clone(),
            bytes: artifact.bytes,
        });
    }
    Ok(out)
}

// ============================================================================ R0.4 assembly

/// The Accepted nodes of `node_type`, sorted by ID.
fn accepted_nodes(graph: &Graph, node_type: NodeType) -> Vec<&Node> {
    let mut nodes: Vec<&Node> = graph
        .node_ids_by_type(node_type)
        .iter()
        .filter_map(|id| graph.node(id))
        .filter(|n| n.status == ElementStatus::Accepted)
        .collect();
    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    nodes
}

/// The validated inference of `request` produced by the latest completed `phase` child of a
/// `recipe` job on the current branch (latest by input revision version, then job update time).
/// The artifact is usable exactly when its request hash equals the request the current graph
/// produces; otherwise the durable material is stale. No inference is executed here.
fn durable_inference(
    state: &ApiState,
    gate: GateId,
    recipe: Recipe,
    phase: Phase,
    request: &InferenceRequest,
) -> Result<(InferenceArtifact, ArtifactInput), ApiError> {
    let branch = state.branch().clone();
    let jobs = state
        .with_stage_store(|store| store.recipe_jobs())?
        .map_err(crate::stage::store_error)?;
    let mut latest: Option<(u64, Timestamp, Hash)> = None;
    for job in jobs
        .iter()
        .filter(|j| j.recipe == recipe.as_str() && j.branch == branch)
    {
        let steps = state
            .with_stage_store(|store| store.steps(&job.recipe_job_id))?
            .map_err(crate::stage::store_error)?;
        for step in steps.iter().filter(|s| s.phase == phase.as_str()) {
            let (Some(child), Some(run_ref)) = (&step.stage_job_id, &step.compile_run_ref) else {
                continue;
            };
            let completed = state
                .with_stage_store(|store| store.stage_job(child))?
                .map_err(crate::stage::store_error)?
                .is_some_and(|r| {
                    matches!(
                        r.status,
                        plumb_store::StageStatus::Ready | plumb_store::StageStatus::Committed
                    )
                });
            if !completed {
                continue;
            }
            let key = (
                step.input_revision_id.version(),
                job.updated_at,
                run_ref.clone(),
            );
            if latest.as_ref().is_none_or(|l| key > *l) {
                latest = Some(key);
            }
        }
    }
    let Some((_, _, run_ref)) = latest else {
        return Err(durable_stale(gate, recipe, "no_completed_run", &request.id));
    };
    let run = load_run(state, &run_ref)?;
    for hash in &run.inference_artifacts {
        let stored = state
            .with_artifact_store(|store| store.get(hash))?
            .map_err(|_| ApiError::workflow_corrupt(&format!("stage run artifact {hash}")))?;
        let artifact: InferenceArtifact = serde_json::from_slice(&stored.bytes)
            .map_err(|_| ApiError::workflow_corrupt(&format!("stage run artifact {hash}")))?;
        if artifact.request_hash == request.id {
            let input = ArtifactInput::from(&stored);
            return Ok((artifact, input));
        }
    }
    Err(durable_stale(
        gate,
        recipe,
        "inference_request_changed",
        &request.id,
    ))
}

/// Accepted English Terms and Accepted Concepts by normalized key, exactly as S1.5 indexes
/// them (Term name and aliases; Concept name).
fn accepted_vocabulary(
    graph: &Graph,
) -> (
    BTreeMap<String, BTreeSet<Id>>,
    BTreeMap<String, BTreeSet<Id>>,
) {
    let mut terms: BTreeMap<String, BTreeSet<Id>> = BTreeMap::new();
    for node in accepted_nodes(graph, NodeType::Term) {
        let NodePayload::Term(term) = &node.payload else {
            continue;
        };
        if term.language != PILOT_VOCABULARY_LANGUAGE {
            continue;
        }
        for name in std::iter::once(&term.term).chain(term.aliases.iter().flatten()) {
            if let Some(key) = normalize_vocabulary_term(name) {
                terms.entry(key).or_default().insert(node.id.clone());
            }
        }
    }
    let mut concepts: BTreeMap<String, BTreeSet<Id>> = BTreeMap::new();
    for node in accepted_nodes(graph, NodeType::Concept) {
        let NodePayload::Concept(concept) = &node.payload else {
            continue;
        };
        if let Some(key) = normalize_vocabulary_term(&concept.name) {
            concepts.entry(key).or_default().insert(node.id.clone());
        }
    }
    (terms, concepts)
}

/// The governed external vocabulary identifiers by (requirement, normalized key): Accepted
/// ResolutionDecisions whose answer is exactly {kind: external_vocabulary_identifier,
/// requirement_ref, normalized_key, identifier}, with a clean rationale and an Accepted human
/// decider (plan S1.6 governed-decision check). The first such decision by ID is used.
fn external_identifiers(graph: &Graph) -> BTreeMap<(Id, String), (Id, String)> {
    let clean = |t: &str| !t.is_empty() && t.trim() == t && !t.chars().any(char::is_control);
    let mut out = BTreeMap::new();
    for node in accepted_nodes(graph, NodeType::ResolutionDecision) {
        let NodePayload::ResolutionDecision(d) = &node.payload else {
            continue;
        };
        let Some(answer) = d.answer.as_object() else {
            continue;
        };
        if answer.get("kind").and_then(Value::as_str) != Some("external_vocabulary_identifier")
            || answer.len() != 4
        {
            continue;
        }
        let (Some(requirement), Some(key), Some(identifier)) = (
            answer
                .get("requirement_ref")
                .and_then(Value::as_str)
                .and_then(|r| r.parse::<Id>().ok()),
            answer.get("normalized_key").and_then(Value::as_str),
            answer.get("identifier").and_then(Value::as_str),
        ) else {
            continue;
        };
        let governed = d.rationale.as_deref().is_some_and(clean)
            && clean(identifier)
            && graph.node(&d.decided_by).is_some_and(|a| {
                a.status == ElementStatus::Accepted
                    && matches!(&a.payload, NodePayload::Agent(agent) if agent.agent_kind == AgentKind::Human)
            });
        if governed {
            out.entry((requirement, key.to_owned()))
                .or_insert((node.id.clone(), identifier.to_owned()));
        }
    }
    out
}

/// F1 inputs (R0.4): Accepted Requirements and their lint inputs from the graph, vocabulary
/// dependencies and TERMS_RESOLVED findings from S1.5 over the latest consolidate artifact,
/// NO_DUPLICATE_ACCEPTED findings from S1.3 with the configured policy, and the configured
/// lint policy. NO_CONTRADICTION keeps reading Accepted Open Finding nodes.
fn f1_inputs(
    state: &ApiState,
    loaded: &LoadedRevision,
    a03: &A03Runtime,
    a04: &A04Runtime,
) -> Result<F1ValidationInputs, ApiError> {
    let gate = GateId::F1;
    let config = a04.gates.f1.as_ref().ok_or_else(|| inputs_missing(gate))?;
    let template = config
        .inputs_template()
        .map_err(|e| evaluation_failed(gate, e))?;
    let graph = &loaded.graph;
    let accepted = accepted_nodes(graph, NodeType::Requirement);
    let mut dependencies: BTreeMap<Id, Vec<F1VocabularyDependency>> = BTreeMap::new();
    let mut findings: Vec<GeneratedFinding> = Vec::new();
    // Every S1.5 dependency and finding is grounded in an Accepted Requirement statement.
    if !accepted.is_empty() {
        let targets = effective_targets(Recipe::Consolidate, graph, None);
        let request = build_vocabulary_request(
            graph,
            &targets,
            &a04.vocabulary_policy,
            a03.provider_policy.clone(),
        )
        .map_err(|e| evaluation_failed(gate, e))?;
        let (artifact, input) = durable_inference(
            state,
            gate,
            Recipe::Consolidate,
            Phase::Consolidate,
            &request.request,
        )?;
        let derivation = derivation_ref(
            &request.request,
            &artifact,
            &input,
            loaded.revision.created_at,
        )
        .map_err(|e| evaluation_failed(gate, e))?;
        let analysis = analyze_vocabulary(
            graph,
            &request,
            &a04.vocabulary_policy,
            Some(VocabularyInference {
                artifact: &artifact,
                derivation_ref: derivation,
            }),
            &VocabularyAudit {
                created_by: a04.stage_actor_ref.clone(),
                created_at: loaded.revision.created_at,
            },
        )
        .map_err(|e| evaluation_failed(gate, e))?;
        findings.extend(analysis.findings);
        let accepted_ids: BTreeSet<&Id> = accepted.iter().map(|n| &n.id).collect();
        let (terms, concepts) = accepted_vocabulary(graph);
        let externals = external_identifiers(graph);
        let single = |map: &BTreeMap<String, BTreeSet<Id>>, key: &str| -> Option<Id> {
            match map.get(key) {
                Some(ids) if ids.len() == 1 => ids.iter().next().cloned(),
                _ => None,
            }
        };
        for candidate in &analysis.candidates {
            for mention in &candidate.mentions {
                if !accepted_ids.contains(&mention.requirement_ref) {
                    continue;
                }
                let key = &mention.normalized_key;
                let resolution = if let Some(term_ref) = single(&terms, key) {
                    F1VocabularyResolution::AcceptedTerm { term_ref }
                } else if let (None, Some(concept_ref)) = (terms.get(key), single(&concepts, key)) {
                    F1VocabularyResolution::AcceptedConcept { concept_ref }
                } else if let Some((decision_ref, identifier)) =
                    externals.get(&(mention.requirement_ref.clone(), key.clone()))
                {
                    F1VocabularyResolution::ExternalIdentifier {
                        decision_ref: decision_ref.clone(),
                        identifier: identifier.clone(),
                    }
                } else {
                    F1VocabularyResolution::Unresolved
                };
                dependencies
                    .entry(mention.requirement_ref.clone())
                    .or_default()
                    .push(F1VocabularyDependency {
                        normalized_key: key.clone(),
                        range: mention.range,
                        resolution,
                    });
            }
        }
    }
    findings.extend(
        analyze_requirement_duplicates(graph, &a04.duplicate_policy)
            .map_err(|e| evaluation_failed(gate, e))?
            .findings,
    );
    findings.sort_by(|a, b| a.id.cmp(&b.id));
    findings.dedup_by(|a, b| a.id == b.id);
    let mut requirements = Vec::with_capacity(accepted.len());
    for node in &accepted {
        let NodePayload::Requirement(requirement) = &node.payload else {
            continue;
        };
        let evidence: BTreeSet<&Id> = node.evidence.iter().map(|e| e.as_id()).collect();
        let input: F1RequirementValidationInput = serde_json::from_value(json!({
            "requirement_ref": node.id,
            "vocabulary_dependencies": dependencies.remove(&node.id).unwrap_or_default(),
            "lint_input": {
                "requirement_ref": node.id,
                "statement": requirement.statement,
                "evidence_refs": evidence,
                "source_anchor": null,
                "term_context": null,
            },
        }))
        .map_err(|e| evaluation_failed(gate, e))?;
        requirements.push(input);
    }
    Ok(F1ValidationInputs::new(
        requirements,
        findings,
        template.lint_policy,
    ))
}

/// The disposition of an external entry for `node`: `Some(true)` usable, `Some(false)` a
/// forbidden state (stale), `None` no longer in scope (ignored: absent or still Proposed).
fn external_target(node: Option<&Node>, usable: impl Fn(&Node) -> bool) -> Option<bool> {
    let node = node?;
    if usable(node) {
        return Some(true);
    }
    if node.status == ElementStatus::Proposed {
        return None;
    }
    Some(false)
}

/// F2 inputs (R0.4): S2.1/S2.2 findings over the latest analyse artifacts, decision-table
/// specifications decoded from the PSG, and the external runtime calculation scope overrides
/// (origin-less Accepted Calculations only) and expression scopes (Accepted Invariants only).
fn f2_inputs(
    state: &ApiState,
    loaded: &LoadedRevision,
    a03: &A03Runtime,
    a04: &A04Runtime,
) -> Result<F2ValidationInputs, ApiError> {
    let gate = GateId::F2;
    let graph = &loaded.graph;
    let mut findings = Vec::new();
    // Every S2.1/S2.2 finding is grounded in an Accepted Requirement statement.
    if !accepted_nodes(graph, NodeType::Requirement).is_empty() {
        let audit_at = loaded.revision.created_at;
        let actor = a04.stage_actor_ref.clone();
        let domain = build_domain_request(graph, a03.provider_policy.clone())
            .map_err(|e| evaluation_failed(gate, e))?;
        let (artifact, input) = durable_inference(
            state,
            gate,
            Recipe::Analyse,
            Phase::Analyse,
            &domain.request,
        )?;
        let derivation = derivation_ref(&domain.request, &artifact, &input, audit_at)
            .map_err(|e| evaluation_failed(gate, e))?;
        findings.extend(
            analyze_domain(
                graph,
                &domain,
                Some(DomainInference {
                    artifact: &artifact,
                    derivation_ref: derivation,
                }),
                &DomainAudit {
                    created_by: actor.clone(),
                    created_at: audit_at,
                },
            )
            .map_err(|e| evaluation_failed(gate, e))?
            .findings,
        );
        let lifecycle = build_lifecycle_request(graph, a03.provider_policy.clone())
            .map_err(|e| evaluation_failed(gate, e))?;
        let (artifact, input) = durable_inference(
            state,
            gate,
            Recipe::Analyse,
            Phase::Analyse,
            &lifecycle.request,
        )?;
        let derivation = derivation_ref(&lifecycle.request, &artifact, &input, audit_at)
            .map_err(|e| evaluation_failed(gate, e))?;
        findings.extend(
            analyze_lifecycle(
                graph,
                &lifecycle,
                Some(LifecycleInference {
                    artifact: &artifact,
                    derivation_ref: derivation,
                }),
                &LifecycleAudit {
                    created_by: actor,
                    created_at: audit_at,
                },
            )
            .map_err(|e| evaluation_failed(gate, e))?
            .findings,
        );
    }
    // Calculation scope overrides: required exactly for origin-less Accepted Calculations.
    let mut stale = BTreeSet::new();
    let mut overrides = Vec::new();
    for o in &a03.calculation_scope_overrides {
        let usable = |n: &Node| {
            n.status == ElementStatus::Accepted
                && matches!(n.payload, NodePayload::Calculation(_))
                && calculation_origin_of(n).is_none()
        };
        match external_target(graph.node(&o.calculation_ref), usable) {
            Some(true) => overrides.push(F2CalculationScopeOverride {
                calculation_ref: o.calculation_ref.clone(),
                scope: o.scope.clone(),
            }),
            Some(false) => {
                stale.insert(o.calculation_ref.clone());
            }
            None => {}
        }
    }
    if !stale.is_empty() {
        return Err(external_stale(gate, "calculation_scope_overrides", &stale));
    }
    let covered: BTreeSet<&Id> = overrides.iter().map(|o| &o.calculation_ref).collect();
    let missing: BTreeSet<Id> = accepted_nodes(graph, NodeType::Calculation)
        .into_iter()
        .filter(|n| calculation_origin_of(n).is_none() && !covered.contains(&n.id))
        .map(|n| n.id.clone())
        .collect();
    if !missing.is_empty() {
        return Err(external_missing(
            gate,
            "calculation_scope_overrides",
            &missing,
        ));
    }
    // Decision tables: decoded from the PSG; an undecodable table fails the gate explicitly.
    let mut tables = Vec::new();
    for node in accepted_nodes(graph, NodeType::DecisionTable) {
        let NodePayload::DecisionTable(table) = &node.payload else {
            continue;
        };
        let spec = decode_decision_table(table).map_err(|e| {
            ApiError::unprocessable(
                "E_STAGE_EVALUATION_FAILED",
                format!(
                    "the {gate} gate cannot be evaluated: decision table {} cannot be decoded",
                    node.id
                ),
            )
            .with_details(json!({
                "gate": gate.to_string(),
                "decision_table": node.id,
                "error": e.to_string(),
            }))
        })?;
        tables.push(F2DecisionTableInput {
            decision_table_ref: node.id.clone(),
            spec,
        });
    }
    // Invariant bindings: the external expression scopes whose owner is an Accepted Invariant.
    let mut invariants = Vec::new();
    for scope in &a03.expression_scopes {
        let node = graph.node(&scope.owner_ref);
        if !node.is_some_and(|n| matches!(n.payload, NodePayload::Invariant(_))) {
            continue; // an Operation, Rule or Transition scope belongs to execution
        }
        match external_target(node, |n| n.status == ElementStatus::Accepted) {
            Some(true) => invariants.push(F2InvariantInput {
                invariant_ref: scope.owner_ref.clone(),
                bindings: scope.bindings.clone(),
            }),
            Some(false) => {
                stale.insert(scope.owner_ref.clone());
            }
            None => {}
        }
    }
    if !stale.is_empty() {
        return Err(external_stale(gate, "expression_scopes", &stale));
    }
    let covered: BTreeSet<&Id> = invariants.iter().map(|i| &i.invariant_ref).collect();
    let missing: BTreeSet<Id> = accepted_nodes(graph, NodeType::Invariant)
        .into_iter()
        .filter(|n| !covered.contains(&n.id))
        .map(|n| n.id.clone())
        .collect();
    if !missing.is_empty() {
        return Err(external_missing(gate, "expression_scopes", &missing));
    }
    Ok(F2ValidationInputs::new(
        findings, overrides, tables, invariants,
    ))
}

/// An Accepted, Confirmed Scenario (the F4 in-scope requirement).
fn is_confirmed_scenario(node: &Node) -> bool {
    node.status == ElementStatus::Accepted
        && matches!(&node.payload, NodePayload::Scenario(s) if s.confirmation_status.as_deref() == Some("Confirmed"))
}

/// The F4 applicability (R0.4): sources, executable sources and blocking Scenarios from the
/// S4.5 verification applicability input and the graph (the S4.5 snapshot), in-scope Scenarios
/// and required kinds per executable source from the explicit F4 configuration.
fn f4_applicability(
    loaded: &LoadedRevision,
    a04: &A04Runtime,
) -> Result<(F4ApplicabilityInput, Vec<Id>), ApiError> {
    let gate = GateId::F4;
    let config = a04.gates.f4.as_ref().ok_or_else(|| inputs_missing(gate))?;
    let verification = a04
        .verification_applicability
        .as_ref()
        .ok_or_else(|| inputs_missing(gate))?;
    let graph = &loaded.graph;
    let mut stale = BTreeSet::new();
    let mut sources = Vec::new();
    for source in &verification.sources {
        let usable = |n: &Node| {
            n.status == ElementStatus::Accepted
                && matches!(
                    n.payload,
                    NodePayload::Requirement(_) | NodePayload::Operation(_)
                )
        };
        match external_target(graph.node(&source.source_ref), usable) {
            Some(true) => sources.push(source.clone()),
            Some(false) => {
                stale.insert(source.source_ref.clone());
            }
            None => {}
        }
    }
    if !stale.is_empty() {
        return Err(external_stale(gate, "verification_applicability", &stale));
    }
    let snapshot = compile_verification_obligations(
        graph,
        &VerificationApplicabilityInputs { sources },
        &VerificationAudit {
            created_by: a04.stage_actor_ref.clone(),
            created_at: loaded.revision.created_at,
        },
    )
    .map_err(|e| match &e {
        VerificationError::InvalidSource { source_ref, .. }
        | VerificationError::InvalidApplicability { source_ref, .. } => external_stale(
            gate,
            "verification_applicability",
            &BTreeSet::from([source_ref.clone()]),
        ),
        _ => evaluation_failed(gate, e),
    })?
    .applicability;
    let mut in_scope = Vec::new();
    for scenario in &config.in_scope_scenario_refs {
        match external_target(graph.node(scenario), is_confirmed_scenario) {
            Some(true) => in_scope.push(scenario.clone()),
            Some(false) => {
                stale.insert(scenario.clone());
            }
            None => {}
        }
    }
    if !stale.is_empty() {
        return Err(external_stale(gate, "in_scope_scenario_refs", &stale));
    }
    let in_scope_set: BTreeSet<&Id> = in_scope.iter().collect();
    let unscoped: BTreeSet<Id> = snapshot
        .blocking_scenario_refs
        .iter()
        .filter(|s| !in_scope_set.contains(s))
        .cloned()
        .collect();
    if !unscoped.is_empty() {
        return Err(external_missing(gate, "in_scope_scenario_refs", &unscoped));
    }
    let mut coverage = Vec::new();
    let mut missing = BTreeSet::new();
    for source in &snapshot.executable_source_refs {
        match config
            .required_scenario_kinds
            .iter()
            .find(|c| &c.source_ref == source)
        {
            Some(entry) => coverage.push(entry.clone()),
            None => {
                missing.insert(source.clone());
            }
        }
    }
    if !missing.is_empty() {
        return Err(external_missing(gate, "required_scenario_kinds", &missing));
    }
    let mut visible = Vec::new();
    for assumption in &config.visible_assumption_refs {
        let active = |n: &Node| {
            n.status == ElementStatus::Accepted
                && matches!(&n.payload, NodePayload::Assumption(a) if a.status == "Accepted")
        };
        match external_target(graph.node(assumption), active) {
            Some(true) => visible.push(assumption.clone()),
            Some(false) => {
                stale.insert(assumption.clone());
            }
            None => {}
        }
    }
    if !stale.is_empty() {
        return Err(external_stale(gate, "visible_assumption_refs", &stale));
    }
    Ok((
        F4ApplicabilityInput {
            source_refs: snapshot.source_refs.into_iter().collect(),
            executable_source_refs: snapshot.executable_source_refs.into_iter().collect(),
            in_scope_scenario_refs: in_scope,
            blocking_scenario_refs: snapshot.blocking_scenario_refs.into_iter().collect(),
            executable_coverage: coverage,
        },
        visible,
    ))
}

/// F4 inputs: the assembled applicability and visible assumptions, one type check per in-scope
/// Scenario and the current runs of in-scope Scenarios with their linked traces.
fn f4_inputs(
    state: &ApiState,
    loaded: &LoadedRevision,
    a04: &A04Runtime,
) -> Result<F4ValidationInputs, ApiError> {
    let (applicability, visible_assumption_refs) = f4_applicability(loaded, a04)?;
    let graph = &loaded.graph;
    let mut checks = Vec::new();
    let mut runs = Vec::new();
    for scenario_ref in &applicability.in_scope_scenario_refs {
        let Some(NodePayload::Scenario(scenario)) = graph.node(scenario_ref).map(|n| &n.payload)
        else {
            continue; // validation rejects the stale applicability itself
        };
        let specification_hash = scenario_specification_hash(scenario_ref, scenario)
            .map_err(|e| evaluation_failed(GateId::F4, e))?;
        let status = match validate_scenario(graph, scenario) {
            Ok(_) => F4ScenarioTypeStatus::Valid,
            Err(error) => F4ScenarioTypeStatus::Invalid {
                issues: vec![error.to_string()],
            },
        };
        checks.push(F4ScenarioTypeCheckInput {
            scenario_ref: scenario_ref.clone(),
            specification_hash,
            status,
        });
        let (_, current) = current_run(graph, scenario_ref)?;
        if let Some((node, run)) = current {
            let link = node
                .extensions
                .get(&link_key()?)
                .and_then(Value::as_str)
                .and_then(|t| t.parse::<Hash>().ok())
                .ok_or_else(|| {
                    evaluation_failed(GateId::F4, "a scenario run trace link is invalid")
                })?;
            let artifact = state
                .with_artifact_store(|store| store.get(&link))?
                .map_err(|e| evaluation_failed(GateId::F4, e))?;
            runs.push(F4ScenarioRunInput {
                run: run.clone(),
                trace_artifact_hash: link,
                trace_artifact_bytes: artifact.bytes,
            });
        }
    }
    Ok(F4ValidationInputs::new(
        applicability,
        checks,
        runs,
        visible_assumption_refs,
    ))
}

/// One evaluated report with its provenance.
pub(crate) struct Evaluated {
    pub(crate) report: GateReport,
    pub(crate) supplemental_input_hash: Option<Hash>,
    pub(crate) evaluation_at: Option<Timestamp>,
}

/// Evaluates `gate` over `loaded` with the configured or derived material. F3 needs the
/// captured `evaluation_at`.
pub(crate) fn evaluate_gate(
    state: &ApiState,
    loaded: &LoadedRevision,
    gate: GateId,
    policy: &ValidationPolicy,
    evaluation_at: Option<Timestamp>,
) -> Result<Evaluated, ApiError> {
    let a03 = state.runtime("stage_gates")?;
    let a04 = state.stage_runtime("stage_gates")?;
    let registry = registry(state)?;
    let graph = &loaded.graph;
    let evidence = if gate == GateId::I0 {
        i0_evidence(state, loaded)?
    } else {
        Vec::new()
    };
    let ctx = ValidationContext::new(
        graph,
        registry.metadata(),
        policy.clone(),
        loaded.revision.evidence_hash.clone(),
        evidence,
        Vec::new(),
    )
    .map_err(|e| evaluation_failed(gate, e))?;
    let (ctx, supplemental) = match gate {
        GateId::I0 => (ctx, None),
        GateId::F1 => {
            let inputs = f1_inputs(state, loaded, a03, a04)?;
            let hash = input_hash(&inputs)?;
            (
                ctx.with_f1_inputs(graph, inputs)
                    .map_err(|e| evaluation_failed(gate, e))?,
                Some(hash),
            )
        }
        GateId::F2 => {
            let inputs = f2_inputs(state, loaded, a03, a04)?;
            let hash = input_hash(&inputs)?;
            (
                ctx.with_f2_inputs(graph, inputs)
                    .map_err(|e| evaluation_failed(gate, e))?,
                Some(hash),
            )
        }
        GateId::F3 => {
            let at = evaluation_at.ok_or_else(|| inputs_missing(gate))?;
            let findings = analyze_assumption_expiry(graph, &FixedClock::new(at))
                .map_err(|e| evaluation_failed(gate, e))?
                .findings;
            let inputs = F3ValidationInputs::new(f3_decision_artifacts(state, loaded)?, findings);
            let hash = input_hash(&inputs)?;
            (
                ctx.with_f3_inputs(graph, inputs)
                    .map_err(|e| evaluation_failed(gate, e))?,
                Some(hash),
            )
        }
        GateId::F4 => {
            let inputs = f4_inputs(state, loaded, a04)?;
            let hash = input_hash(&inputs)?;
            (
                ctx.with_f4_inputs(graph, inputs)
                    .map_err(|e| evaluation_failed(gate, e))?,
                Some(hash),
            )
        }
        other => return Err(evaluation_failed(other, "not a NOW gate")),
    };
    let report = registry
        .evaluate_gate(gate, graph, &ctx)
        .map_err(|e| evaluation_failed(gate, e))?;
    Ok(Evaluated {
        report,
        supplemental_input_hash: supplemental,
        evaluation_at: (gate == GateId::F3).then_some(evaluation_at).flatten(),
    })
}

/// Persists an evaluated report for `loaded`'s revision (upsert per revision and gate).
pub(crate) fn persist_report(
    state: &ApiState,
    loaded: &LoadedRevision,
    evaluated: &Evaluated,
    recipe_job_id: Option<&Hash>,
    created_at: Timestamp,
) -> Result<GateReportRecord, ApiError> {
    let report = &evaluated.report;
    let record = NewGateReport {
        revision_id: loaded.revision.id.clone(),
        gate: report.gate.to_string(),
        report_hash: report
            .content_hash()
            .map_err(|_| ApiError::internal("gate_report"))?,
        report_json: serde_json::to_value(report).map_err(|_| ApiError::internal("gate_report"))?,
        supplemental_input_hash: evaluated.supplemental_input_hash.clone(),
        evaluation_at: evaluated.evaluation_at,
        recipe_job_id: recipe_job_id.cloned(),
        created_at,
    };
    state
        .with_stage_store(|store| store.upsert_gate_report(&record))?
        .map_err(crate::stage::store_error)
}

/// The result of one gate of `ready`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReadyGate {
    pub gate: String,
    pub result: String,
    pub report_hash: Hash,
    /// The rules of the report that did not pass, in report order (what to fix next).
    pub failed_rules: Vec<ReadyRule>,
}

/// One non-passing rule of a `ready` gate report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReadyRule {
    pub rule_id: String,
    pub severity: Value,
    pub state: Value,
    pub targets: Vec<Id>,
}

impl ReadyRule {
    /// The rules of `report` whose state is neither PASS nor NOT_APPLICABLE.
    pub(crate) fn failed(report: &GateReport) -> Vec<ReadyRule> {
        report
            .rules
            .iter()
            .filter_map(|rule| {
                let state = serde_json::to_value(rule.state).ok()?;
                if matches!(state.as_str(), Some("PASS") | Some("NOT_APPLICABLE")) {
                    return None;
                }
                Some(ReadyRule {
                    rule_id: rule.rule_id.clone(),
                    severity: serde_json::to_value(rule.effective_severity).ok()?,
                    state,
                    targets: rule.targets.clone(),
                })
            })
            .collect()
    }
}

/// The `ready` outcome: one loaded head, the captured `evaluation_at` and every gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReadyOutcome {
    pub revision: String,
    pub semantic_hash: String,
    pub evaluation_at: Timestamp,
    pub gates: Vec<ReadyGate>,
    /// Product-level readiness preconditions that do not hold (sorted, unique; R0.4). They are
    /// not validation rules and never change a gate report.
    pub preconditions: Vec<String>,
}

/// The product precondition of a project with zero Accepted Requirements (R0.4).
pub const NO_ACCEPTED_REQUIREMENTS: &str = "no_accepted_requirements";

impl ReadyOutcome {
    /// Whether every gate passed and every product precondition holds.
    pub fn passed(&self) -> bool {
        self.gates.iter().all(|g| g.result == "pass") && self.preconditions.is_empty()
    }
}

impl ApiState {
    /// `ready`: evaluates I0, F1, F2, F3 and F4 on one loaded head and persists every report.
    /// The external F1 and F4 configuration is required before anything is evaluated;
    /// `evaluation_at` is read once from the injected clock for F3. A head with zero Accepted
    /// Requirements adds the `no_accepted_requirements` product precondition (R0.4).
    pub fn ready(&self) -> Result<ReadyOutcome, ApiError> {
        let a04 = self.stage_runtime("ready")?;
        for (gate, present) in [
            (GateId::F1, a04.gates.f1.is_some()),
            (GateId::F4, a04.gates.f4.is_some()),
        ] {
            if !present {
                return Err(inputs_missing(gate));
            }
        }
        let loaded = self.load_head()?;
        let evaluation_at = self.now();
        let mut gates = Vec::new();
        for gate in [GateId::I0, GateId::F1, GateId::F2, GateId::F3, GateId::F4] {
            let evaluated = evaluate_gate(
                self,
                &loaded,
                gate,
                &a04.validation_policy,
                Some(evaluation_at),
            )?;
            let record = persist_report(self, &loaded, &evaluated, None, evaluation_at)?;
            gates.push(ReadyGate {
                gate: gate.to_string(),
                result: result_wire(evaluated.report.result),
                report_hash: record.report_hash,
                failed_rules: ReadyRule::failed(&evaluated.report),
            });
        }
        let mut preconditions = Vec::new();
        if accepted_nodes(&loaded.graph, NodeType::Requirement).is_empty() {
            preconditions.push(NO_ACCEPTED_REQUIREMENTS.to_owned());
        }
        preconditions.sort();
        preconditions.dedup();
        Ok(ReadyOutcome {
            revision: loaded.revision.id.to_string(),
            semantic_hash: loaded.revision.semantic_hash.to_string(),
            evaluation_at,
            gates,
            preconditions,
        })
    }
}

/// The wire string of a gate result.
pub(crate) fn result_wire(result: GateResult) -> String {
    match result {
        GateResult::Pass => "pass".to_owned(),
        GateResult::Fail => "fail".to_owned(),
    }
}
