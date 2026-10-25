//! The application `StageRecipeJob` model (A0.4, Hotfix 067): recipes, phases, scope rules, the
//! recipe configuration projection and the job identity. A recipe job is not a compiler
//! `StageJob`; its phases own zero or more child StageJobs whose identity is unchanged.

use plumb_core::{to_canonical_json, Hash, Id, StageId, Timestamp};
use plumb_psg::{ElementStatus, Graph, NodePayload, NodeType};
use plumb_store::{BranchName, LoadedRevision, RevisionId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{echo, ApiError};
use crate::openapi::StageName;
use crate::runtime::ProviderMode;
use crate::selector::Selections;
use crate::stage::scenario_execution::runnable;
use crate::state::{A03Runtime, A04Runtime};

/// The version of the recipe job identity and configuration projections.
pub const RECIPE_VERSION: u32 = 1;

/// One recipe phase; the wire value is the stored `phase`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Phase {
    I0,
    F1,
    F2,
    F4,
    Segmentation,
    Classification,
    Consolidate,
    Calc,
    Rules,
    Analyse,
    Flows,
    Scenarios,
    Tags,
    Execute,
    Verification,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::I0 => "i0",
            Phase::F1 => "f1",
            Phase::F2 => "f2",
            Phase::F4 => "f4",
            Phase::Segmentation => "segmentation",
            Phase::Classification => "classification",
            Phase::Consolidate => "consolidate",
            Phase::Calc => "calc",
            Phase::Rules => "rules",
            Phase::Analyse => "analyse",
            Phase::Flows => "flows",
            Phase::Scenarios => "scenarios",
            Phase::Tags => "tags",
            Phase::Execute => "execute",
            Phase::Verification => "verification",
        }
    }

    /// The compiler stage owning the phase.
    pub fn stage(self) -> StageId {
        match self {
            Phase::I0 => StageId::S0,
            Phase::F1 | Phase::Segmentation | Phase::Classification | Phase::Consolidate => {
                StageId::S1
            }
            Phase::F2
            | Phase::Calc
            | Phase::Rules
            | Phase::Analyse
            | Phase::Flows
            | Phase::Tags => StageId::S2,
            Phase::F4 | Phase::Scenarios | Phase::Execute | Phase::Verification => StageId::S4,
        }
    }

    /// Whether the phase owns a compiler child StageJob.
    pub fn has_child(self) -> bool {
        !matches!(
            self,
            Phase::I0 | Phase::F1 | Phase::F2 | Phase::F4 | Phase::Rules
        )
    }
}

/// Which IDs a recipe scope may name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    /// Only a null scope is accepted.
    Whole,
    /// Baseline EvidenceFragments with `plumb_import:fragment` and non-empty extracted text.
    EvidenceFragments,
    /// Proposed or Accepted Requirements.
    Requirements,
    /// Accepted DecisionTables.
    DecisionTables,
    /// Accepted, Confirmed Scenarios.
    Scenarios,
}

/// One public `StageName` compatibility recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Recipe {
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

impl Recipe {
    pub const ALL: [Recipe; 11] = [
        Recipe::Intake,
        Recipe::Lint,
        Recipe::Extract,
        Recipe::Consolidate,
        Recipe::Calc,
        Recipe::Rules,
        Recipe::Analyse,
        Recipe::Flows,
        Recipe::Scenarios,
        Recipe::Tags,
        Recipe::Run,
    ];

    pub fn from_stage_name(name: StageName) -> Recipe {
        match name {
            StageName::Intake => Recipe::Intake,
            StageName::Lint => Recipe::Lint,
            StageName::Extract => Recipe::Extract,
            StageName::Consolidate => Recipe::Consolidate,
            StageName::Calc => Recipe::Calc,
            StageName::Rules => Recipe::Rules,
            StageName::Analyse => Recipe::Analyse,
            StageName::Flows => Recipe::Flows,
            StageName::Scenarios => Recipe::Scenarios,
            StageName::Tags => Recipe::Tags,
            StageName::Run => Recipe::Run,
        }
    }

    /// The lowercase `StageName` wire value.
    pub fn as_str(self) -> &'static str {
        match self {
            Recipe::Intake => "intake",
            Recipe::Lint => "lint",
            Recipe::Extract => "extract",
            Recipe::Consolidate => "consolidate",
            Recipe::Calc => "calc",
            Recipe::Rules => "rules",
            Recipe::Analyse => "analyse",
            Recipe::Flows => "flows",
            Recipe::Scenarios => "scenarios",
            Recipe::Tags => "tags",
            Recipe::Run => "run",
        }
    }

    pub fn parse(text: &str) -> Option<Recipe> {
        Recipe::ALL.into_iter().find(|r| r.as_str() == text)
    }

    pub fn scope_kind(self) -> ScopeKind {
        match self {
            Recipe::Intake | Recipe::Lint | Recipe::Analyse | Recipe::Scenarios | Recipe::Tags => {
                ScopeKind::Whole
            }
            Recipe::Extract => ScopeKind::EvidenceFragments,
            Recipe::Consolidate | Recipe::Calc | Recipe::Flows => ScopeKind::Requirements,
            Recipe::Rules => ScopeKind::DecisionTables,
            Recipe::Run => ScopeKind::Scenarios,
        }
    }

    /// Whether the recipe has an inference phase (and therefore binds the provider).
    pub fn uses_inference(self) -> bool {
        matches!(
            self,
            Recipe::Extract
                | Recipe::Consolidate
                | Recipe::Calc
                | Recipe::Analyse
                | Recipe::Flows
                | Recipe::Scenarios
                | Recipe::Tags
        )
    }

    /// The configuration values the recipe requires and that are absent.
    pub(crate) fn missing_config(self, a04: &A04Runtime) -> Vec<&'static str> {
        let mut missing = Vec::new();
        match self {
            Recipe::Lint if a04.gates.f1.is_none() => missing.push("gates.f1"),
            Recipe::Calc if a04.calculation_scope.is_none() => missing.push("calculation_scope"),
            Recipe::Flows => {
                if a04.operation_scope.is_none() {
                    missing.push("operation_scope");
                }
                if a04.process_scope.is_none() {
                    missing.push("process_scope");
                }
            }
            Recipe::Scenarios if a04.scenario_derivation.is_none() => {
                missing.push("scenario_derivation")
            }
            Recipe::Run => {
                if a04.verification_applicability.is_none() {
                    missing.push("verification_applicability");
                }
                if a04.gates.f4.is_none() {
                    missing.push("gates.f4");
                }
            }
            _ => {}
        }
        missing
    }

    /// The recipe-relevant configuration values (compiler architecture, Hotfix 067), each
    /// resolved against `graph` (the captured base): a selector form is recorded as its
    /// resolved typed value, exactly the explicit form, plus `selections` naming what was
    /// configured (absent when nothing is a selector form, so explicit inputs are unchanged).
    pub(crate) fn inputs(
        self,
        a03: &A03Runtime,
        a04: &A04Runtime,
        graph: &Graph,
    ) -> Result<Value, ApiError> {
        let mut selections = Selections::default();
        let inputs = match self {
            Recipe::Intake => json!({ "validation_policy": v(&a04.validation_policy)? }),
            Recipe::Lint => json!({
                "validation_policy": v(&a04.validation_policy)?,
                "f1": v(&a04.gates.f1)?,
                "vocabulary_policy": v(&a04.vocabulary_policy)?,
                "duplicate_policy": v(&a04.duplicate_policy)?,
            }),
            Recipe::Extract | Recipe::Rules | Recipe::Tags => json!({}),
            Recipe::Consolidate => json!({
                "vocabulary_policy": v(&a04.vocabulary_policy)?,
                "duplicate_policy": v(&a04.duplicate_policy)?,
            }),
            Recipe::Calc => {
                selections.record("calculation_scope", a04.calculation_scope.as_ref());
                json!({ "calculation_scope": v(&a04.calculation_scope_for(graph)?)? })
            }
            Recipe::Analyse => {
                selections.record_entries("expression_scopes", &a03.expression_scopes);
                selections.record_entries(
                    "calculation_scope_overrides",
                    &a03.calculation_scope_overrides,
                );
                json!({
                    "validation_policy": v(&a04.validation_policy)?,
                    "expression_scopes": v(&a03.expression_scopes_for(graph)?)?,
                    "calculation_scope_overrides":
                        v(&a03.calculation_scope_overrides_for(graph)?)?,
                })
            }
            Recipe::Flows => {
                selections.record("operation_scope", a04.operation_scope.as_ref());
                selections.record("process_scope", a04.process_scope.as_ref());
                selections.record("authorization_draft", a04.authorization_draft.as_ref());
                json!({
                    "operation_scope": v(&a04.operation_scope_for(graph)?)?,
                    "process_scope": v(&a04.process_scope_for(graph)?)?,
                    "authorization_draft": v(&a04.authorization_draft_for(graph)?)?,
                })
            }
            Recipe::Scenarios => {
                selections.record("scenario_derivation", a04.scenario_derivation.as_ref());
                json!({ "scenario_derivation": v(&a04.scenario_derivation_for(graph)?)? })
            }
            Recipe::Run => {
                selections.record(
                    "verification_applicability",
                    a04.verification_applicability.as_ref(),
                );
                selections.record("f4", a04.gates.f4.as_ref());
                selections.record_entries("expression_scopes", &a03.expression_scopes);
                selections.record_entries(
                    "calculation_scope_overrides",
                    &a03.calculation_scope_overrides,
                );
                selections.record_entries("runtime_semantic_values", &a03.runtime_semantic_values);
                json!({
                    "validation_policy": v(&a04.validation_policy)?,
                    "verification_applicability": v(&a04.verification_applicability_for(graph)?)?,
                    "f4": v(&a04.f4_for(graph)?)?,
                    "calendars": v(&a04.calendars)?,
                    "direct_managers": v(&a04.direct_managers)?,
                    "expression_scopes": v(&a03.expression_scopes_for(graph)?)?,
                    "calculation_scope_overrides":
                        v(&a03.calculation_scope_overrides_for(graph)?)?,
                    "runtime_semantic_values": v(&a03.runtime_semantic_values_for(graph)?)?,
                })
            }
        };
        Ok(selections.into_inputs(inputs))
    }
}

/// The JSON value of one typed configuration value.
fn v<T: Serialize>(value: &T) -> Result<Value, ApiError> {
    serde_json::to_value(value).map_err(|e| ApiError::internal_cause("stage_config", &e))
}

/// The provider object of a recipe configuration: `null` for recipes without inference.
pub(crate) fn provider_projection(
    recipe: Recipe,
    a03: &A03Runtime,
    mode: &ProviderMode,
) -> Result<Value, ApiError> {
    if !recipe.uses_inference() {
        return Ok(Value::Null);
    }
    Ok(json!({
        "mode": mode.as_str(),
        "policy": serde_json::to_value(&a03.provider_policy)
            .map_err(|e| ApiError::internal_cause("stage_config", &e))?,
        "replay_fixtures_hash": mode.fixtures_hash(),
    }))
}

/// The recipe configuration projection `{version, recipe, provider, stage_actor_ref, inputs}`.
pub(crate) fn recipe_config(
    recipe: Recipe,
    a03: &A03Runtime,
    a04: &A04Runtime,
    graph: &Graph,
) -> Result<Value, ApiError> {
    Ok(json!({
        "version": RECIPE_VERSION,
        "recipe": recipe.as_str(),
        "provider": provider_projection(recipe, a03, &a04.provider_mode)?,
        "stage_actor_ref": a04.stage_actor_ref,
        "inputs": recipe.inputs(a03, a04, graph)?,
    }))
}

/// Generic SHA-256 of the RFC 8785 JSON of `value`.
pub fn canonical_hash(value: &Value) -> Result<Hash, ApiError> {
    let bytes = to_canonical_json(value).map_err(|e| ApiError::internal_cause("stage_hash", &e))?;
    Ok(Hash::content_sha256(&bytes))
}

/// The immutable capture of a recipe job: exactly the identity fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeCapture {
    pub version: u32,
    pub branch: BranchName,
    pub initial_base_revision: RevisionId,
    pub initial_base_semantic_hash: Hash,
    pub recipe: String,
    pub scope: Option<Vec<Id>>,
    pub profile_hash: Hash,
    pub config_hash: Hash,
}

impl RecipeCapture {
    /// Generic SHA-256 of the RFC 8785 JSON of the capture.
    pub fn id(&self) -> Result<Hash, ApiError> {
        let bytes =
            to_canonical_json(self).map_err(|e| ApiError::internal_cause("stage_job_id", &e))?;
        Ok(Hash::content_sha256(&bytes))
    }

    /// The capture's recipe.
    pub fn recipe(&self) -> Result<Recipe, ApiError> {
        Recipe::parse(&self.recipe).ok_or_else(|| ApiError::workflow_corrupt("stage recipe"))
    }
}

/// Parses request scope items: exact Plumb IDs, sorted, duplicates rejected (400). `None` stays
/// `None`.
pub fn parse_scope(scope: Option<&[String]>) -> Result<Option<Vec<Id>>, ApiError> {
    let Some(items) = scope else {
        return Ok(None);
    };
    let mut ids = Vec::with_capacity(items.len());
    for text in items {
        let id: Id = text
            .parse()
            .map_err(|_| ApiError::invalid(format!("scope item {} is not an ID", echo(text))))?;
        if id.as_str() != text {
            return Err(ApiError::invalid(format!(
                "scope item {} is not a canonical ID",
                echo(text)
            )));
        }
        ids.push(id);
    }
    ids.sort();
    if ids.windows(2).any(|w| w[0] == w[1]) {
        return Err(ApiError::invalid("scope items must be unique"));
    }
    Ok(Some(ids))
}

/// 422 `E_STAGE_SCOPE_INVALID`.
fn scope_invalid(recipe: Recipe, id: Option<&Id>, reason: &str) -> ApiError {
    ApiError::unprocessable(
        "E_STAGE_SCOPE_INVALID",
        format!("the scope is not valid for {}: {reason}", recipe.as_str()),
    )
    .with_details(json!({ "recipe": recipe.as_str(), "id": id }))
}

/// Whether `id` is eligible for `kind` in `graph`.
fn eligible(graph: &Graph, kind: ScopeKind, id: &Id) -> bool {
    let Some(node) = graph.node(id) else {
        return false;
    };
    match kind {
        ScopeKind::Whole => false,
        ScopeKind::EvidenceFragments => is_segmentable(node),
        ScopeKind::Requirements => {
            matches!(node.payload, NodePayload::Requirement(_))
                && matches!(
                    node.status,
                    ElementStatus::Proposed | ElementStatus::Accepted
                )
        }
        ScopeKind::DecisionTables => {
            matches!(node.payload, NodePayload::DecisionTable(_))
                && node.status == ElementStatus::Accepted
        }
        ScopeKind::Scenarios => runnable(node),
    }
}

/// A baseline EvidenceFragment carrying `plumb_import:fragment` and non-empty extracted text.
pub fn is_segmentable(node: &plumb_psg::Node) -> bool {
    let NodePayload::EvidenceFragment(fragment) = &node.payload else {
        return false;
    };
    plumb_psg::is_baseline(node.status)
        && fragment
            .extracted_text
            .as_deref()
            .is_some_and(|t| !t.is_empty())
        && node
            .extensions
            .keys()
            .any(|k| k.as_str() == plumb_import::FRAGMENT_EXTENSION)
}

/// Validates a parsed scope against the recipe and graph: absent IDs are 404, ineligible IDs and
/// any non-null scope of a whole-scope recipe are 422.
pub fn validate_scope(recipe: Recipe, graph: &Graph, scope: Option<&[Id]>) -> Result<(), ApiError> {
    let Some(ids) = scope else {
        return Ok(());
    };
    let kind = recipe.scope_kind();
    if kind == ScopeKind::Whole {
        return Err(scope_invalid(
            recipe,
            None,
            "only the whole scope is supported",
        ));
    }
    for id in ids {
        if graph.node(id).is_none() && graph.edge(id).is_none() {
            return Err(ApiError::not_found(format!("scope element {id} not found")));
        }
        if !eligible(graph, kind, id) {
            return Err(scope_invalid(
                recipe,
                Some(id),
                "the element is not eligible",
            ));
        }
    }
    Ok(())
}

/// The effective target set of a recipe in `graph`: the explicit scope, or every eligible ID
/// sorted. Whole-scope recipes have no target list.
pub fn effective_targets(recipe: Recipe, graph: &Graph, scope: Option<&[Id]>) -> Vec<Id> {
    if let Some(ids) = scope {
        return ids.to_vec();
    }
    let kind = recipe.scope_kind();
    let node_type = match kind {
        ScopeKind::Whole => return Vec::new(),
        ScopeKind::EvidenceFragments => NodeType::EvidenceFragment,
        ScopeKind::Requirements => NodeType::Requirement,
        ScopeKind::DecisionTables => NodeType::DecisionTable,
        ScopeKind::Scenarios => NodeType::Scenario,
    };
    let mut ids: Vec<Id> = graph
        .node_ids_by_type(node_type)
        .iter()
        .filter(|id| eligible(graph, kind, id))
        .cloned()
        .collect();
    ids.sort();
    ids
}

/// Builds the capture of `recipe` over `loaded` with `scope` and its configuration.
pub fn capture(
    branch: &BranchName,
    loaded: &LoadedRevision,
    recipe: Recipe,
    scope: Option<Vec<Id>>,
    config: &Value,
) -> Result<RecipeCapture, ApiError> {
    Ok(RecipeCapture {
        version: RECIPE_VERSION,
        branch: branch.clone(),
        initial_base_revision: loaded.revision.id.clone(),
        initial_base_semantic_hash: loaded.revision.semantic_hash.clone(),
        recipe: recipe.as_str().to_owned(),
        scope,
        profile_hash: loaded.revision.profile_hash.clone(),
        config_hash: canonical_hash(config)?,
    })
}

/// The child `CompileContext.config` of one phase (Hotfix 067).
pub fn child_config(
    recipe: Recipe,
    phase: Phase,
    stage_actor_ref: &Id,
    created_at: Timestamp,
    provider: &Value,
    inputs: Value,
) -> Value {
    json!({
        "version": RECIPE_VERSION,
        "recipe": recipe.as_str(),
        "phase": phase.as_str(),
        "audit": { "actor_ref": stage_actor_ref, "created_at": created_at },
        "provider": provider,
        "inputs": inputs,
    })
}
