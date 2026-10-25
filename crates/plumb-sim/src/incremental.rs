//! Dirty-set scenario re-execution and S4 execution findings (plan S4.4, Hotfix 053).
//!
//! The finished F0.11 [`DirtySet`] is intersected with each Confirmed Scenario's exact execution
//! and proof dependencies; every selected Scenario reruns once, in Scenario ID order, through the
//! unchanged S4.2 `run` and S4.3 `compare_run` with its one caller-supplied runtime. Fail and
//! caller-designated blocking Undecidable verdicts become deterministic execution-governance
//! findings. Nothing here recomputes impact, mutates the graph, reads a clock or persists runs,
//! artifacts or findings.

use std::collections::{BTreeMap, BTreeSet};

use plumb_core::{Hash, Id};
use plumb_functional::{
    validate_scenario, ScenarioDependency, ScenarioSemantics, ScenarioValue, ScenarioWhen,
    SCENARIO_CONFIRMED,
};
use plumb_patch::DirtySet;
use plumb_psg::{
    ElementStatus, Finding, FindingSeverity, Graph, NodePayload, NodeType, RelationKind,
};
use plumb_validation::{finding_id, finding_key, EvaluationError, GeneratedFinding};
use thiserror::Error;

use crate::compare::{compare_run, ComparisonError, ScenarioComparison};
use crate::execute::run;
use crate::materialize::admit;
use crate::model::{accepted, CalculationEntry, ExecutionModel, ExecutionRuntime, SimulationError};
use crate::trace::ScenarioVerdict;

/// The finding code of a failed Confirmed Scenario.
pub const SCENARIO_FAIL_CODE: &str = "I_SCENARIO_FAIL";
/// The finding code of an undecidable caller-designated blocking Scenario.
pub const SCENARIO_UNDECIDABLE_CODE: &str = "G_SCENARIO_UNDECIDABLE";
/// The semantic condition key of [`SCENARIO_FAIL_CODE`].
pub const SCENARIO_FAIL_CONDITION: &str = "scenario_fail";
/// The semantic condition key of [`SCENARIO_UNDECIDABLE_CODE`].
pub const SCENARIO_UNDECIDABLE_CONDITION: &str = "blocking_scenario_undecidable";
/// The family of S4 execution findings.
pub const SCENARIO_FINDING_FAMILY: &str = "S4";

const OPEN: &str = "Open";

/// The one runtime context of one Scenario in an incremental invocation.
pub struct IncrementalScenarioRuntime<'a> {
    pub scenario_ref: Id,
    pub runtime: ExecutionRuntime<'a>,
}

/// Temporary caller-supplied S4 policy (the pilot bridge before S4.5): never PSG truth and never
/// persisted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IncrementalScenarioPolicy {
    pub blocking_scenario_refs: BTreeSet<Id>,
}

/// The reruns and current execution-finding material of one invocation.
#[derive(Debug, Clone, PartialEq)]
pub struct IncrementalScenarioResult {
    /// Scenario ID order.
    pub selected_scenario_refs: Vec<Id>,
    /// scenario_ref order.
    pub comparisons: Vec<ScenarioComparison>,
    /// Finding ID order.
    pub findings: Vec<GeneratedFinding>,
}

/// Why an incremental invocation could not complete. Nothing partial is returned.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IncrementalError {
    #[error("two runtime entries for {scenario_ref}")]
    DuplicateRuntime { scenario_ref: Id },
    #[error("selected scenario {scenario_ref} has no runtime entry")]
    MissingRuntime { scenario_ref: Id },
    #[error("{scenario_ref} is not an Accepted, Confirmed Scenario")]
    InvalidBlockingScenario { scenario_ref: Id },
    #[error(transparent)]
    Simulation(#[from] SimulationError),
    #[error(transparent)]
    Comparison(#[from] ComparisonError),
    #[error(transparent)]
    Finding(#[from] EvaluationError),
    #[error("dependencies of {scenario_ref} cannot be derived: {reason}")]
    Dependency { scenario_ref: Id, reason: String },
}

fn confirmed_scenario(graph: &Graph, id: &Id) -> bool {
    matches!(
        accepted(graph, id, NodeType::Scenario).map(|n| &n.payload),
        Some(NodePayload::Scenario(s)) if s.confirmation_status.as_deref() == Some(SCENARIO_CONFIRMED)
    )
}

/// Accepted targets of Accepted `kind` edges from `from`.
fn targets(graph: &Graph, from: &Id, kind: RelationKind) -> Vec<Id> {
    graph
        .outgoing_edge_ids(from)
        .iter()
        .filter_map(|e| graph.edge(e))
        .filter(|e| e.kind == kind && e.status == ElementStatus::Accepted)
        .filter(|e| {
            graph
                .node(&e.to)
                .is_some_and(|n| n.status == ElementStatus::Accepted)
        })
        .map(|e| e.to.clone())
        .collect()
}

/// Accepted sources of Accepted `kind` edges into `to`.
fn sources(graph: &Graph, to: &Id, kind: RelationKind) -> Vec<Id> {
    graph
        .incoming_edge_ids(to)
        .iter()
        .filter_map(|e| graph.edge(e))
        .filter(|e| e.kind == kind && e.status == ElementStatus::Accepted)
        .filter(|e| {
            graph
                .node(&e.from)
                .is_some_and(|n| n.status == ElementStatus::Accepted)
        })
        .map(|e| e.from.clone())
        .collect()
}

/// The exact execution and proof dependencies of one Scenario.
struct DependencyCollector<'m, 'g> {
    model: &'m ExecutionModel<'g>,
    ids: BTreeSet<Id>,
    /// Calculations whose calendar and closure were already collected.
    calculations: BTreeSet<Id>,
    /// The elements whose Invariants can apply to the run: the anchor, the Operation's read and
    /// written Entities and Attributes and the subjects of reachable Transitions (H0.10).
    footprint: BTreeSet<Id>,
}

impl DependencyCollector<'_, '_> {
    fn graph(&self) -> &Graph {
        self.model.graph
    }

    fn add(&mut self, id: &Id) {
        if self
            .graph()
            .node(id)
            .is_some_and(|n| n.status == ElementStatus::Accepted)
        {
            self.ids.insert(id.clone());
        }
    }

    fn dependency(&mut self, d: &ScenarioDependency) {
        if accepted(self.graph(), &d.owner_ref, NodeType::Calculation).is_some() {
            self.calculation(&d.owner_ref);
        } else {
            self.add(&d.owner_ref);
        }
    }

    fn value(&mut self, v: &ScenarioValue) {
        if let ScenarioValue::SemanticRef(d) = v {
            self.dependency(d);
        }
    }

    /// A Calculation, its Accepted calendar and its S2.6 dependency closure from the model.
    fn calculation(&mut self, id: &Id) {
        if !self.calculations.insert(id.clone()) {
            return;
        }
        self.add(id);
        let graph = self.model.graph;
        if let Some(NodePayload::Calculation(c)) =
            accepted(graph, id, NodeType::Calculation).map(|n| &n.payload)
        {
            if let Some(calendar) = &c.calendar_ref {
                self.add(calendar);
            }
        }
        if let Some(CalculationEntry::Qualified(q)) = self.model.calculations.get(id) {
            for dependency in &q.dependencies {
                self.calculation(dependency);
            }
        }
    }

    fn calculations_used_by(&mut self, owner: &Id) {
        for c in targets(self.model.graph, owner, RelationKind::UsesCalculation) {
            self.calculation(&c);
        }
    }

    /// Transitions triggered by `trigger` and their from and to States.
    fn transitions_via(&mut self, trigger: &Id) {
        let graph = self.model.graph;
        for t in sources(graph, trigger, RelationKind::TransitionsVia) {
            self.transition(&t);
        }
    }

    fn transition(&mut self, id: &Id) {
        let graph = self.model.graph;
        if let Some(NodePayload::Transition(t)) =
            accepted(graph, id, NodeType::Transition).map(|n| &n.payload)
        {
            self.add(id);
            self.add(&t.from_state);
            self.add(&t.to_state);
            self.footprint.insert(t.stateful_ref.clone());
        }
    }

    /// The Accepted read and written Entities and Attributes of an Operation.
    fn operation_footprint(&mut self, id: &Id) {
        let graph = self.model.graph;
        for kind in [RelationKind::Reads, RelationKind::Writes] {
            for target in targets(graph, id, kind) {
                let footprint_type = graph.node(&target).map(|n| n.payload.node_type());
                if matches!(footprint_type, Some(NodeType::Entity | NodeType::Attribute)) {
                    self.add(&target);
                    self.footprint.insert(target);
                }
            }
        }
    }

    /// The Accepted Invariants scoped to the footprint (H0.10, architecture §33.11).
    fn footprint_invariants(&mut self) {
        let graph = self.model.graph;
        for invariant_ref in graph.node_ids_by_type(NodeType::Invariant) {
            if let Some(NodePayload::Invariant(i)) =
                accepted(graph, invariant_ref, NodeType::Invariant).map(|n| &n.payload)
            {
                if self.footprint.contains(&i.scope_ref) {
                    self.ids.insert(invariant_ref.clone());
                }
            }
        }
    }

    fn operation(&mut self, id: &Id) {
        let graph = self.model.graph;
        for governor in targets(graph, id, RelationKind::GovernedBy) {
            self.add(&governor);
            if accepted(graph, &governor, NodeType::Rule).is_some() {
                self.calculations_used_by(&governor);
            }
        }
        self.calculations_used_by(id);
        self.operation_footprint(id);
        for produced in targets(graph, id, RelationKind::Produces) {
            self.add(&produced);
        }
        // Authorization: applicable Permissions, the SecurityRoles granting them and every role
        // inheriting one of those (holders of A inherit B when A inherits_role B).
        let mut roles = BTreeSet::new();
        for permission in sources(graph, id, RelationKind::Permits) {
            self.add(&permission);
            roles.extend(sources(graph, &permission, RelationKind::Grants));
        }
        let mut stack: Vec<Id> = roles.iter().cloned().collect();
        while let Some(role) = stack.pop() {
            for inheritor in sources(graph, &role, RelationKind::InheritsRole) {
                if roles.insert(inheritor.clone()) {
                    stack.push(inheritor);
                }
            }
        }
        for role in &roles {
            self.add(role);
        }
        self.transitions_via(id);
    }

    fn collect(
        &mut self,
        scenario_ref: &Id,
        semantics: &ScenarioSemantics,
        payload_refs: &[Id],
        actor: Option<&Id>,
    ) {
        self.add(scenario_ref);
        for r in payload_refs {
            self.add(r);
        }
        self.footprint.insert(semantics.when.anchor().clone());
        match &semantics.when {
            ScenarioWhen::Operation(id) => {
                self.add(id);
                self.operation(id);
            }
            ScenarioWhen::Event(id) => {
                self.add(id);
                self.transitions_via(id);
            }
            ScenarioWhen::Transition(id) => self.transition(id),
            ScenarioWhen::Rule(id) => {
                self.add(id);
                self.calculations_used_by(id);
            }
            ScenarioWhen::Calculation(id) => self.calculation(id),
            ScenarioWhen::DecisionTable(id) => self.add(id),
        }
        let given = &semantics.given;
        for (attribute, value) in &given.attribute_values {
            self.add(attribute);
            self.value(value);
        }
        for id in given.entities_exist.iter().chain(&given.states_active) {
            self.add(id);
        }
        for value in given.decision_inputs.values() {
            self.value(value);
        }
        for (calculation, entries) in &given.calculation_semantics {
            self.calculation(calculation);
            for value in entries.values() {
                self.value(value);
            }
        }
        let then = &semantics.then;
        for (attribute, value) in &then.attribute_values {
            self.add(attribute);
            self.value(value);
        }
        for id in then
            .states_active
            .iter()
            .chain(&then.outcomes)
            .chain(&then.events)
            .chain(&then.rules_satisfied)
        {
            self.add(id);
        }
        for value in then.decision_outputs.values() {
            self.value(value);
        }
        for (calculation, value) in &then.calculation_results {
            self.calculation(calculation);
            self.value(value);
        }
        if let Some(actor) = actor {
            self.add(actor);
        }
        self.footprint_invariants();
    }
}

/// The exact dependency set of one Confirmed Scenario.
fn scenario_dependencies(
    model: &ExecutionModel<'_>,
    scenario_ref: &Id,
    actor: Option<&Id>,
) -> Result<BTreeSet<Id>, IncrementalError> {
    let Some(NodePayload::Scenario(scenario)) = model.graph.node(scenario_ref).map(|n| &n.payload)
    else {
        return Err(IncrementalError::Dependency {
            scenario_ref: scenario_ref.clone(),
            reason: "not a Scenario".to_owned(),
        });
    };
    let semantics =
        validate_scenario(model.graph, scenario).map_err(|e| IncrementalError::Dependency {
            scenario_ref: scenario_ref.clone(),
            reason: e.to_string(),
        })?;
    let payload_refs: Vec<Id> = scenario
        .requirement_refs
        .iter()
        .flatten()
        .chain(scenario.derived_from_refs.iter().flatten())
        .cloned()
        .collect();
    let mut collector = DependencyCollector {
        model,
        ids: BTreeSet::new(),
        calculations: BTreeSet::new(),
        footprint: BTreeSet::new(),
    };
    collector.collect(scenario_ref, &semantics, &payload_refs, actor);
    Ok(collector.ids)
}

/// The runtime catalog by Scenario; a duplicate entry is an error.
fn runtime_catalog<'r, 'a>(
    runtimes: &'r [IncrementalScenarioRuntime<'a>],
) -> Result<BTreeMap<&'r Id, &'r ExecutionRuntime<'a>>, IncrementalError> {
    let mut catalog = BTreeMap::new();
    for entry in runtimes {
        if catalog
            .insert(&entry.scenario_ref, &entry.runtime)
            .is_some()
        {
            return Err(IncrementalError::DuplicateRuntime {
                scenario_ref: entry.scenario_ref.clone(),
            });
        }
    }
    Ok(catalog)
}

fn select(
    model: &ExecutionModel<'_>,
    dirty: &DirtySet,
    catalog: &BTreeMap<&Id, &ExecutionRuntime<'_>>,
) -> Result<Vec<Id>, IncrementalError> {
    let mut selected = Vec::new();
    if dirty.node_ids.is_empty() {
        return Ok(selected);
    }
    let graph = model.graph;
    for scenario_ref in graph.node_ids_by_type(NodeType::Scenario) {
        if !confirmed_scenario(graph, scenario_ref) {
            continue;
        }
        let actor = catalog
            .get(scenario_ref)
            .and_then(|runtime| runtime.actor_ref.as_ref());
        let dependencies = scenario_dependencies(model, scenario_ref, actor)?;
        if dirty.node_ids.contains(scenario_ref)
            || dependencies.iter().any(|id| dirty.node_ids.contains(id))
        {
            selected.push(scenario_ref.clone());
        }
    }
    Ok(selected)
}

/// The Confirmed Scenarios whose definition or exact dependencies are in the dirty set, in
/// Scenario ID order. No runtime or provider is called.
pub fn select_dirty_scenarios(
    model: &ExecutionModel<'_>,
    dirty: &DirtySet,
    runtimes: &[IncrementalScenarioRuntime<'_>],
) -> Result<Vec<Id>, IncrementalError> {
    let catalog = runtime_catalog(runtimes)?;
    select(model, dirty, &catalog)
}

fn scenario_finding(
    code: &str,
    condition: &str,
    message: String,
    resolution: &str,
    scenario_ref: &Id,
) -> Result<GeneratedFinding, IncrementalError> {
    let key = finding_key(code, std::slice::from_ref(scenario_ref), condition)?;
    let id = finding_id(&key)?;
    Ok(GeneratedFinding {
        key,
        id,
        semantic_condition_key: condition.to_owned(),
        payload: Finding {
            code: code.to_owned(),
            family: SCENARIO_FINDING_FAMILY.to_owned(),
            severity: FindingSeverity::Blocker,
            message,
            status: OPEN.to_owned(),
            affected_refs: vec![scenario_ref.clone()],
            standard_rule_ref: None,
            suggested_resolution: Some(resolution.to_owned()),
            waiver_ref: None,
        },
    })
}

/// The current S4 execution finding of one comparison, if any.
pub fn scenario_execution_finding(
    comparison: &ScenarioComparison,
    policy: &IncrementalScenarioPolicy,
) -> Result<Option<GeneratedFinding>, IncrementalError> {
    let scenario_ref = &comparison.scenario_ref;
    match comparison.result {
        ScenarioVerdict::Pass => Ok(None),
        ScenarioVerdict::Fail => scenario_finding(
            SCENARIO_FAIL_CODE,
            SCENARIO_FAIL_CONDITION,
            format!("Confirmed scenario {scenario_ref} failed against its expected semantics."),
            "Review the scenario trace and resolve the model or confirmed expectation mismatch.",
            scenario_ref,
        )
        .map(Some),
        ScenarioVerdict::Undecidable if policy.blocking_scenario_refs.contains(scenario_ref) => {
            scenario_finding(
                SCENARIO_UNDECIDABLE_CODE,
                SCENARIO_UNDECIDABLE_CONDITION,
                format!(
                    "Blocking confirmed scenario {scenario_ref} is undecidable because required semantics are missing."
                ),
                "Resolve the missing semantics and re-execute the scenario.",
                scenario_ref,
            )
            .map(Some)
        }
        ScenarioVerdict::Undecidable => Ok(None),
    }
}

/// Whether a recorded ScenarioRun is current for the model (H0.10, architecture §33.11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenarioRunCurrency {
    Current,
    /// The Scenario specification changed since the run.
    StaleSpecification,
    /// The run was computed for an older model baseline, so possibly for an older state of
    /// the Scenario's dependencies: never reported current.
    StaleDependencies,
}

/// The currency of a run recorded with `specification_hash` over the model baseline
/// `model_semantic_hash` (both from its ScenarioTrace artifact). The trace envelope binds the
/// whole baseline, so a run of any older dependency state is stale, never current.
pub fn scenario_run_currency(
    model: &ExecutionModel<'_>,
    scenario_ref: &Id,
    specification_hash: &Hash,
    model_semantic_hash: &Hash,
) -> Result<ScenarioRunCurrency, IncrementalError> {
    let current = admit(model, scenario_ref)?;
    Ok(if &current.specification_hash != specification_hash {
        ScenarioRunCurrency::StaleSpecification
    } else if model.semantic_hash() != model_semantic_hash {
        ScenarioRunCurrency::StaleDependencies
    } else {
        ScenarioRunCurrency::Current
    })
}

/// Reruns the selected Scenarios through the unchanged run and compare_run and returns their
/// comparisons and current execution-finding material. Nothing is persisted or reconciled.
pub fn rerun_dirty_scenarios(
    model: &ExecutionModel<'_>,
    dirty: &DirtySet,
    runtimes: &[IncrementalScenarioRuntime<'_>],
    policy: &IncrementalScenarioPolicy,
) -> Result<IncrementalScenarioResult, IncrementalError> {
    for scenario_ref in &policy.blocking_scenario_refs {
        if !confirmed_scenario(model.graph, scenario_ref) {
            return Err(IncrementalError::InvalidBlockingScenario {
                scenario_ref: scenario_ref.clone(),
            });
        }
    }
    let catalog = runtime_catalog(runtimes)?;
    let selected = select(model, dirty, &catalog)?;
    for scenario_ref in &selected {
        if !catalog.contains_key(scenario_ref) {
            return Err(IncrementalError::MissingRuntime {
                scenario_ref: scenario_ref.clone(),
            });
        }
    }
    let mut comparisons = Vec::new();
    let mut findings = Vec::new();
    for scenario_ref in &selected {
        let runtime = catalog[scenario_ref];
        let execution = run(model, scenario_ref, runtime)?;
        let comparison = compare_run(model, &execution, runtime)?;
        findings.extend(scenario_execution_finding(&comparison, policy)?);
        comparisons.push(comparison);
    }
    comparisons.sort_by(|a, b| a.scenario_ref.cmp(&b.scenario_ref));
    findings.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(IncrementalScenarioResult {
        selected_scenario_refs: selected,
        comparisons,
        findings,
    })
}
