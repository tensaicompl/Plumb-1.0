//! The complete `NodePayload` semantic enum and its payload types (metamodel §§5-16, 24, 24.1).
//!
//! Binding rules (§24.1): Required fields are `T`, Optional fields `Option<T>` (serialized as
//! `null` when absent); every struct rejects unknown fields; `*_ref` is `Id`, `*_hash` is
//! `Hash`; only the whitelisted fields hold open JSON (`Value`); expressions are strings.

use plumb_core::{Hash, HashKind, Id, Timestamp};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::extensions::ExtensionKey;
use crate::standards::MappingRole;

// ============================================================================ closed vocabularies

/// Declares a closed vocabulary enum whose serialized values are the listed strings.
macro_rules! closed_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, std::hash::Hash, Serialize, Deserialize)]
        pub enum $name {
            $(#[serde(rename = $text)] $variant),+
        }
    };
}

closed_enum!(
    /// `DerivationRecord.kind` (§5.3).
    DerivationKind {
        DeterministicRule => "deterministic_rule",
        Parser => "parser",
        Import => "import",
        HumanEdit => "human_edit",
        HumanResolution => "human_resolution",
        LlmInference => "llm_inference",
        Recovery => "recovery",
        Migration => "migration",
        ExternalSync => "external_sync",
    }
);

closed_enum!(
    /// `Agent.agent_kind` (§5.4).
    AgentKind {
        Human => "human",
        Organization => "organization",
        SoftwareService => "software_service",
        LlmModel => "llm_model",
        CompilerStage => "compiler_stage",
        ExternalSystem => "external_system",
    }
);

closed_enum!(
    /// `Finding.severity` (validation rulebook severities).
    FindingSeverity {
        Blocker => "blocker",
        Error => "error",
        Warn => "warn",
        Info => "info",
    }
);

closed_enum!(
    /// `Question.question_kind` (§6.2); serialized with the metamodel spellings.
    QuestionKind {
        YesNo => "YesNo",
        PickOne => "PickOne",
        PickMany => "PickMany",
        Number => "Number",
        Text => "Text",
        Cardinality => "Cardinality",
        Unit => "Unit",
        Precision => "Precision",
        Rounding => "Rounding",
        FormulaConfirm => "FormulaConfirm",
        RuleCell => "RuleCell",
        Calendar => "Calendar",
        RoleAssignment => "RoleAssignment",
        Permission => "Permission",
        QualityThreshold => "QualityThreshold",
        ArchitectureChoice => "ArchitectureChoice",
        TechnologyChoice => "TechnologyChoice",
        InterfaceChoice => "InterfaceChoice",
        VerificationMethod => "VerificationMethod",
    }
);

closed_enum!(
    /// `Requirement.requirement_kind` (§7.5).
    RequirementKind {
        Functional => "functional",
        Quality => "quality",
        Interface => "interface",
        Data => "data",
        Security => "security",
        Operational => "operational",
        Compliance => "compliance",
        Transition => "transition",
        Constraint => "constraint",
    }
);

closed_enum!(
    /// `Requirement.level` (§7.5).
    RequirementLevel {
        Stakeholder => "stakeholder",
        System => "system",
        Software => "software",
        Subsystem => "subsystem",
        Component => "component",
        Interface => "interface",
    }
);

closed_enum!(
    /// `Requirement.modality` (§7.5).
    Modality {
        Shall => "shall",
        Should => "should",
        May => "may",
        ShallNot => "shall_not",
    }
);

closed_enum!(
    /// `Constraint.constraint_category` (§7.7).
    ConstraintCategory {
        Business => "business",
        Technical => "technical",
        Technology => "technology",
        Security => "security",
        Data => "data",
        Integration => "integration",
        Operational => "operational",
        Legal => "legal",
        Regulatory => "regulatory",
        Organizational => "organizational",
        Legacy => "legacy",
    }
);

closed_enum!(
    /// `Constraint.strength` (§7.7).
    ConstraintStrength {
        Mandatory => "mandatory",
        Preferred => "preferred",
        Prohibited => "prohibited",
    }
);

closed_enum!(
    /// `Concept.concept_kind` (§8.2).
    ConceptKind {
        ObjectType => "object_type",
        FactType => "fact_type",
        ValueType => "value_type",
        Role => "role",
        Other => "other",
    }
);

closed_enum!(
    /// `Actor.actor_kind` (§8.3).
    ActorKind {
        Human => "human",
        System => "system",
        ExternalSystem => "external_system",
        Organization => "organization",
    }
);

closed_enum!(
    /// `Operation.operation_kind` (§9.1).
    OperationKind {
        Command => "command",
        Query => "query",
    }
);

closed_enum!(
    /// `Outcome.outcome_kind` (§9.2).
    OutcomeKind {
        Success => "success",
        BusinessFailure => "business_failure",
        TechnicalFailure => "technical_failure",
        Partial => "partial",
    }
);

closed_enum!(
    /// `ProcessNode.node_kind` (§9.5).
    ProcessNodeKind {
        Start => "start",
        End => "end",
        HumanTask => "human_task",
        ServiceTask => "service_task",
        ExclusiveGateway => "exclusive_gateway",
        ParallelSplit => "parallel_split",
        ParallelJoin => "parallel_join",
        MessageEvent => "message_event",
        TimerEvent => "timer_event",
        ErrorEvent => "error_event",
        Subprocess => "subprocess",
    }
);

closed_enum!(
    /// `Rule.rule_kind` (§9.6).
    RuleKind {
        Constraint => "constraint",
        Derivation => "derivation",
        Permission => "permission",
        Validation => "validation",
        Business => "business",
    }
);

closed_enum!(
    /// `Scenario.scenario_kind` (§9.10).
    ScenarioKind {
        Acceptance => "acceptance",
        Boundary => "boundary",
        Failure => "failure",
        StateTransition => "state_transition",
        RuleRow => "rule_row",
        Quality => "quality",
        Integration => "integration",
        Regression => "regression",
    }
);

closed_enum!(
    /// `SeparationConstraint.constraint_kind` (§10.6).
    SeparationConstraintKind {
        StaticSeparationOfDuty => "static_separation_of_duty",
        DynamicSeparationOfDuty => "dynamic_separation_of_duty",
        MutualExclusion => "mutual_exclusion",
        RequiredCombination => "required_combination",
    }
);

closed_enum!(
    /// `ArchitectureCandidate.status` (§12.3).
    ArchitectureCandidateStatus {
        Exploring => "exploring",
        Candidate => "candidate",
        Accepted => "accepted",
        Rejected => "rejected",
        Superseded => "superseded",
    }
);

closed_enum!(
    /// `TechnologySelection.status` (§12.10).
    TechnologySelectionStatus {
        Candidate => "candidate",
        Selected => "selected",
        Rejected => "rejected",
        Legacy => "legacy",
        Prohibited => "prohibited",
    }
);

closed_enum!(
    /// `ApiContract.contract_kind` (§13.1).
    ApiContractKind {
        Http => "http",
        Rpc => "rpc",
        Other => "other",
    }
);

closed_enum!(
    /// `VerificationObligation.verification_kind` (§15.1).
    VerificationKind {
        Test => "test",
        Scenario => "scenario",
        Analysis => "analysis",
        Inspection => "inspection",
        Review => "review",
        Demonstration => "demonstration",
        FormalCheck => "formal_check",
        ArchitectureCheck => "architecture_check",
        SecurityCheck => "security_check",
    }
);

closed_enum!(
    /// `ScenarioRun.result` (§15.3).
    ScenarioRunResult {
        Pass => "pass",
        Fail => "fail",
        Undecidable => "undecidable",
    }
);

// ============================================================================ evidence / provenance

/// §5.1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceArtifact {
    pub source_kind: String,
    pub display_name: String,
    pub content_hash: Hash,
    pub media_type: String,
    pub external_uri: Option<String>,
    pub external_version: Option<String>,
    pub producer: Option<String>,
    pub created_at_source: Option<Timestamp>,
    pub language: Option<String>,
    pub classification: Option<String>,
}

/// Why an [`EvidenceLocator`] is invalid.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EvidenceLocatorError {
    /// A `PageRegion` coordinate is NaN or infinite.
    #[error("page-region {0} must be finite")]
    NonFiniteCoordinate(&'static str),
    /// A `PageRegion` width or height is negative.
    #[error("page-region {0} must be >= 0")]
    NegativeExtent(&'static str),
}

/// Where an evidence fragment sits inside its source (§5.2). Structural only, except that
/// `PageRegion` values must be finite and its width/height non-negative.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    deny_unknown_fields,
    try_from = "EvidenceLocatorFields"
)]
pub enum EvidenceLocator {
    TextRange {
        start: u64,
        end: u64,
    },
    PageRegion {
        page: u64,
        x: Option<f64>,
        y: Option<f64>,
        width: Option<f64>,
        height: Option<f64>,
    },
    TableCell {
        table: u64,
        row: u64,
        column: u64,
    },
    XmlPath {
        xpath: String,
    },
    JsonPointer {
        pointer: String,
    },
    ConversationTurn {
        turn_id: Id,
    },
    ExternalObject {
        object_id: String,
        field: Option<String>,
    },
}

impl EvidenceLocator {
    /// Checks `PageRegion` values: finite coordinates and non-negative width/height.
    pub fn validate(&self) -> Result<(), EvidenceLocatorError> {
        if let EvidenceLocator::PageRegion {
            x,
            y,
            width,
            height,
            ..
        } = self
        {
            for (name, value) in [("x", x), ("y", y), ("width", width), ("height", height)] {
                if value.is_some_and(|v| !v.is_finite()) {
                    return Err(EvidenceLocatorError::NonFiniteCoordinate(name));
                }
            }
            for (name, value) in [("width", width), ("height", height)] {
                if value.is_some_and(|v| v < 0.0) {
                    return Err(EvidenceLocatorError::NegativeExtent(name));
                }
            }
        }
        Ok(())
    }
}

/// Unvalidated wire form of [`EvidenceLocator`]; deserialization goes through `TryFrom`.
#[derive(Deserialize)]
#[serde(tag = "kind", content = "data", deny_unknown_fields)]
enum EvidenceLocatorFields {
    TextRange {
        start: u64,
        end: u64,
    },
    PageRegion {
        page: u64,
        x: Option<f64>,
        y: Option<f64>,
        width: Option<f64>,
        height: Option<f64>,
    },
    TableCell {
        table: u64,
        row: u64,
        column: u64,
    },
    XmlPath {
        xpath: String,
    },
    JsonPointer {
        pointer: String,
    },
    ConversationTurn {
        turn_id: Id,
    },
    ExternalObject {
        object_id: String,
        field: Option<String>,
    },
}

impl TryFrom<EvidenceLocatorFields> for EvidenceLocator {
    type Error = EvidenceLocatorError;

    fn try_from(f: EvidenceLocatorFields) -> Result<Self, Self::Error> {
        let locator = match f {
            EvidenceLocatorFields::TextRange { start, end } => {
                EvidenceLocator::TextRange { start, end }
            }
            EvidenceLocatorFields::PageRegion {
                page,
                x,
                y,
                width,
                height,
            } => EvidenceLocator::PageRegion {
                page,
                x,
                y,
                width,
                height,
            },
            EvidenceLocatorFields::TableCell { table, row, column } => {
                EvidenceLocator::TableCell { table, row, column }
            }
            EvidenceLocatorFields::XmlPath { xpath } => EvidenceLocator::XmlPath { xpath },
            EvidenceLocatorFields::JsonPointer { pointer } => {
                EvidenceLocator::JsonPointer { pointer }
            }
            EvidenceLocatorFields::ConversationTurn { turn_id } => {
                EvidenceLocator::ConversationTurn { turn_id }
            }
            EvidenceLocatorFields::ExternalObject { object_id, field } => {
                EvidenceLocator::ExternalObject { object_id, field }
            }
        };
        locator.validate()?;
        Ok(locator)
    }
}

/// §5.2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceFragment {
    pub source_ref: Id,
    pub locator: EvidenceLocator,
    pub content_hash: Hash,
    pub extracted_text: Option<String>,
    pub speaker: Option<String>,
    pub source_timestamp: Option<Timestamp>,
}

/// Why a [`DerivationRecord`] is structurally invalid.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DerivationRecordError {
    /// `kind` is `llm_inference` but this LLM-specific field is absent.
    #[error("llm_inference derivation is missing required field {0}")]
    MissingLlmField(&'static str),
    /// `kind` is not `llm_inference` but this LLM-only field is present.
    #[error("{kind:?} derivation must not set LLM-only field {field}")]
    UnexpectedLlmField {
        kind: DerivationKind,
        field: &'static str,
    },
    /// An `llm_inference` content hash field is not a generic `sha256:` hash.
    #[error("llm_inference field {field} must be a generic sha256: hash, got {kind:?}")]
    NonGenericLlmHash { field: &'static str, kind: HashKind },
}

/// Provenance of an element (§5.3). LLM-specific fields are present exactly when
/// `kind == llm_inference`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "DerivationRecordFields")]
pub struct DerivationRecord {
    pub id: Id,
    pub kind: DerivationKind,
    pub stage: String,
    pub input_refs: Vec<String>,
    pub output_refs: Vec<String>,
    pub created_at: Timestamp,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub prompt_template_hash: Option<Hash>,
    pub schema_hash: Option<Hash>,
    pub context_hash: Option<Hash>,
    pub parameters: Option<Value>,
    pub raw_response_hash: Option<Hash>,
    pub validated_output_hash: Option<Hash>,
}

impl DerivationRecord {
    /// Checks the LLM-field presence rule for `kind`.
    pub fn validate(&self) -> Result<(), DerivationRecordError> {
        let llm_fields: [(&'static str, bool); 8] = [
            ("provider", self.provider.is_some()),
            ("model", self.model.is_some()),
            ("prompt_template_hash", self.prompt_template_hash.is_some()),
            ("schema_hash", self.schema_hash.is_some()),
            ("context_hash", self.context_hash.is_some()),
            ("parameters", self.parameters.is_some()),
            ("raw_response_hash", self.raw_response_hash.is_some()),
            (
                "validated_output_hash",
                self.validated_output_hash.is_some(),
            ),
        ];
        let is_llm = self.kind == DerivationKind::LlmInference;
        for (field, present) in llm_fields {
            match (is_llm, present) {
                (true, false) => return Err(DerivationRecordError::MissingLlmField(field)),
                (false, true) => {
                    return Err(DerivationRecordError::UnexpectedLlmField {
                        kind: self.kind,
                        field,
                    })
                }
                _ => {}
            }
        }
        if is_llm {
            let llm_hashes: [(&'static str, &Option<Hash>); 5] = [
                ("prompt_template_hash", &self.prompt_template_hash),
                ("schema_hash", &self.schema_hash),
                ("context_hash", &self.context_hash),
                ("raw_response_hash", &self.raw_response_hash),
                ("validated_output_hash", &self.validated_output_hash),
            ];
            for (field, hash) in llm_hashes {
                if let Some(kind) = hash.as_ref().map(Hash::kind) {
                    if kind != HashKind::Generic {
                        return Err(DerivationRecordError::NonGenericLlmHash { field, kind });
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DerivationRecordFields {
    id: Id,
    kind: DerivationKind,
    stage: String,
    input_refs: Vec<String>,
    output_refs: Vec<String>,
    created_at: Timestamp,
    provider: Option<String>,
    model: Option<String>,
    prompt_template_hash: Option<Hash>,
    schema_hash: Option<Hash>,
    context_hash: Option<Hash>,
    parameters: Option<Value>,
    raw_response_hash: Option<Hash>,
    validated_output_hash: Option<Hash>,
}

impl TryFrom<DerivationRecordFields> for DerivationRecord {
    type Error = DerivationRecordError;

    fn try_from(f: DerivationRecordFields) -> Result<Self, Self::Error> {
        let record = DerivationRecord {
            id: f.id,
            kind: f.kind,
            stage: f.stage,
            input_refs: f.input_refs,
            output_refs: f.output_refs,
            created_at: f.created_at,
            provider: f.provider,
            model: f.model,
            prompt_template_hash: f.prompt_template_hash,
            schema_hash: f.schema_hash,
            context_hash: f.context_hash,
            parameters: f.parameters,
            raw_response_hash: f.raw_response_hash,
            validated_output_hash: f.validated_output_hash,
        };
        record.validate()?;
        Ok(record)
    }
}

/// §5.4. The node ID identifies the actual actor/service/model/stage instance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    pub agent_kind: AgentKind,
}

// ============================================================================ governance

/// §6.1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub code: String,
    pub family: String,
    pub severity: FindingSeverity,
    pub message: String,
    pub status: String,
    pub affected_refs: Vec<Id>,
    pub standard_rule_ref: Option<String>,
    pub suggested_resolution: Option<String>,
    pub waiver_ref: Option<Id>,
}

/// §6.2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Question {
    pub finding_ref: Id,
    pub question_kind: QuestionKind,
    pub prompt: String,
    pub status: String,
    pub answer_schema: Option<Value>,
    pub stakeholder_ref: Option<Id>,
    pub priority: Option<String>,
    pub round_ref: Option<Id>,
    pub context_refs: Option<Vec<Id>>,
}

/// Why a [`ResolutionDecision`] is structurally invalid.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ResolutionDecisionError {
    /// Neither `question_ref` nor `proposal_ref` is present.
    #[error("resolution decision must reference a question or a proposal")]
    MissingQuestionOrProposal,
}

/// §6.3. At least one of `question_ref` and `proposal_ref` is present.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ResolutionDecisionFields")]
pub struct ResolutionDecision {
    pub question_ref: Option<Id>,
    pub proposal_ref: Option<Id>,
    pub answer: Value,
    pub decided_by: Id,
    pub decided_at: Timestamp,
    pub patch_ref: Hash,
    pub rationale: Option<String>,
    pub supersedes: Option<Id>,
}

impl ResolutionDecision {
    /// Checks that the decision references a question or a proposal.
    pub fn validate(&self) -> Result<(), ResolutionDecisionError> {
        if self.question_ref.is_none() && self.proposal_ref.is_none() {
            Err(ResolutionDecisionError::MissingQuestionOrProposal)
        } else {
            Ok(())
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResolutionDecisionFields {
    question_ref: Option<Id>,
    proposal_ref: Option<Id>,
    answer: Value,
    decided_by: Id,
    decided_at: Timestamp,
    patch_ref: Hash,
    rationale: Option<String>,
    supersedes: Option<Id>,
}

impl TryFrom<ResolutionDecisionFields> for ResolutionDecision {
    type Error = ResolutionDecisionError;

    fn try_from(f: ResolutionDecisionFields) -> Result<Self, Self::Error> {
        let decision = ResolutionDecision {
            question_ref: f.question_ref,
            proposal_ref: f.proposal_ref,
            answer: f.answer,
            decided_by: f.decided_by,
            decided_at: f.decided_at,
            patch_ref: f.patch_ref,
            rationale: f.rationale,
            supersedes: f.supersedes,
        };
        decision.validate()?;
        Ok(decision)
    }
}

/// §6.4.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assumption {
    pub statement: String,
    pub owner_ref: Id,
    pub status: String,
    pub finding_ref: Option<Id>,
    pub default_value: Option<Value>,
    pub expires_at: Option<Timestamp>,
    pub risk_ref: Option<Id>,
}

// ============================================================================ intent / requirements

/// §7.1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stakeholder {
    pub name: String,
    pub stakeholder_kind: String,
    pub organization: Option<String>,
    pub responsibilities: Option<Vec<String>>,
    pub contact_ref: Option<String>,
}

/// §7.2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Concern {
    pub name: String,
    pub description: String,
}

/// §7.3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Goal {
    pub statement: String,
    pub success_measures: Option<Vec<String>>,
    pub priority: Option<String>,
}

/// §7.4.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Need {
    pub statement: String,
    pub stakeholder_refs: Vec<Id>,
    pub goal_refs: Option<Vec<Id>>,
    pub context: Option<String>,
}

/// §7.5.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    pub statement: String,
    pub requirement_kind: RequirementKind,
    pub level: RequirementLevel,
    pub modality: Modality,
    pub title: Option<String>,
    pub rationale: Option<String>,
    pub priority: Option<String>,
    pub source_identifier: Option<String>,
    pub verification_method: Option<String>,
    pub owner_refs: Option<Vec<Id>>,
    pub stakeholder_refs: Option<Vec<Id>>,
}

/// §7.6.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceCriterion {
    pub statement: String,
    pub criterion_kind: String,
    pub verification_method: Option<String>,
    pub measure_ref: Option<Id>,
    pub scenario_ref: Option<Id>,
}

/// §7.7.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Constraint {
    pub statement: String,
    pub constraint_category: ConstraintCategory,
    pub strength: ConstraintStrength,
}

// ============================================================================ vocabulary / domain

/// §8.1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Term {
    pub term: String,
    pub language: String,
    pub definition_ref: Option<Id>,
    pub aliases: Option<Vec<String>>,
    pub status: Option<String>,
}

/// §8.2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Concept {
    pub name: String,
    pub definition: String,
    pub concept_kind: ConceptKind,
}

/// §8.3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Actor {
    pub name: String,
    pub actor_kind: ActorKind,
}

/// §8.4.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BusinessRole {
    pub name: String,
}

/// §8.5.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entity {
    pub name: String,
    pub description: Option<String>,
    pub aggregate_root: Option<bool>,
}

/// §8.6.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attribute {
    pub name: String,
    pub value_type: String,
    pub nullable: bool,
    pub unit: Option<String>,
    pub precision: Option<u32>,
    pub enum_values: Option<Vec<String>>,
    pub data_classification: Option<String>,
}

/// §8.7.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainRelationship {
    pub from_entity: Id,
    pub to_entity: Id,
    pub relationship_kind: String,
    pub cardinality_from: String,
    pub cardinality_to: String,
    pub name: Option<String>,
    pub snapshot_semantics: Option<bool>,
    pub ownership: Option<String>,
}

/// §8.8. The owner is the `has_state` relation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub name: String,
}

/// §8.9. `from_state`/`to_state` are State IDs; the trigger is the `transitions_via` relation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    pub stateful_ref: Id,
    pub from_state: Id,
    pub to_state: Id,
    pub guard_expr: Option<String>,
    pub effect_refs: Option<Vec<Id>>,
}

/// §8.10.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Invariant {
    pub scope_ref: Id,
    pub expression: String,
}

// ============================================================================ functional behaviour

/// §9.1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub name: String,
    pub operation_kind: OperationKind,
    pub input_schema_ref: Option<Id>,
    pub output_schema_ref: Option<Id>,
    pub preconditions: Option<Vec<String>>,
    pub postconditions: Option<Vec<String>>,
    pub idempotency: Option<String>,
    pub transaction_semantics: Option<String>,
}

/// §9.2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub name: String,
    pub outcome_kind: OutcomeKind,
}

/// §9.3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub name: String,
    pub payload_schema_ref: Option<Id>,
    pub semantic_type: Option<String>,
}

/// §9.4.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Process {
    pub name: String,
    pub description: Option<String>,
    pub process_kind: Option<String>,
}

/// §9.5.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessNode {
    pub process_ref: Id,
    pub node_kind: ProcessNodeKind,
    pub operation_ref: Option<Id>,
    pub condition_expr: Option<String>,
    pub message_ref: Option<Id>,
    pub timer_expr: Option<String>,
}

/// §9.6.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub name: String,
    pub rule_kind: RuleKind,
    pub condition_expr: Option<String>,
    pub action_expr: Option<String>,
}

/// §9.7.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionTable {
    pub name: String,
    pub hit_policy: String,
    pub inputs: Vec<Value>,
    pub outputs: Vec<Value>,
    pub rows: Vec<Value>,
}

/// §9.8.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Calculation {
    pub name: String,
    pub expression: String,
    pub result_type: String,
    pub unit: Option<String>,
    pub rounding: Option<String>,
    pub calendar_ref: Option<Id>,
    pub examples: Option<Vec<Value>>,
}

/// §9.9.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Calendar {
    pub time_zone: String,
    pub week_pattern: Value,
    pub region: Option<String>,
    pub holiday_source: Option<String>,
}

/// §9.10.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub name: String,
    pub scenario_kind: ScenarioKind,
    pub given: Vec<Value>,
    pub when: Value,
    pub then: Vec<Value>,
    pub requirement_refs: Option<Vec<Id>>,
    pub derived_from_refs: Option<Vec<Id>>,
    pub confirmation_status: Option<String>,
}

// ============================================================================ authorization

/// §10.1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Principal {
    pub name: String,
    pub principal_kind: String,
}

/// §10.2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityRole {
    pub name: String,
    pub description: Option<String>,
}

/// §10.3. Operation, scope and conditions are the `permits`, `scoped_to` and
/// `conditioned_by` relations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Permission {
    pub name: String,
}

/// §10.4.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceScope {
    pub resource_ref: Id,
    pub scope_kind: String,
}

/// §10.5.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyCondition {
    pub expression: String,
}

/// §10.6.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeparationConstraint {
    pub constraint_kind: SeparationConstraintKind,
    pub role_refs: Vec<Id>,
}

// ============================================================================ quality

/// §11.1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityCharacteristic {
    pub name: String,
    pub scheme: String,
}

/// §11.2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measure {
    pub name: String,
    pub measure_type: String,
    pub unit: String,
    pub aggregation: Option<String>,
    pub sampling_window: Option<String>,
    pub measurement_method: Option<String>,
}

/// §11.3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityScenario {
    pub stimulus: String,
    pub response: String,
    pub threshold: Value,
    pub source_ref: Option<Id>,
    pub environment_condition: Option<String>,
    pub affected_refs: Option<Vec<Id>>,
    pub priority: Option<String>,
}

// ============================================================================ architecture

/// §12.1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemOfInterest {
    pub name: String,
}

/// §12.2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureDescription {
    pub system_of_interest_ref: Id,
    pub purpose: Option<String>,
    pub scope: Option<String>,
    pub architecture_candidate_ref: Option<Id>,
}

/// §12.3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureCandidate {
    pub name: String,
    pub status: ArchitectureCandidateStatus,
}

/// §12.4.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Viewpoint {
    pub name: String,
    pub concern_refs: Vec<Id>,
    pub stakeholder_refs: Vec<Id>,
    pub model_kind_refs: Vec<Id>,
    pub purpose: Option<String>,
    pub conventions: Option<String>,
}

/// §12.5. Layout/style live in separately hashed metadata referenced by
/// `layout_ref`/`style_ref`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    pub name: String,
    pub viewpoint_ref: Id,
    pub architecture_description_ref: Id,
    pub root_refs: Option<Vec<Id>>,
    pub filter: Option<Value>,
    pub projection_rules: Vec<String>,
    pub layout_ref: Option<Hash>,
    pub style_ref: Option<Hash>,
}

/// §12.6.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelKind {
    pub name: String,
    pub semantic_scope: String,
}

/// §12.7. Shared payload of the ten architecture element variants; the variant is the type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureElement {
    pub name: String,
    pub description: Option<String>,
    pub responsibilities: Option<Vec<String>>,
    pub owner_ref: Option<Id>,
}

/// §12.8.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureDecision {
    pub question: String,
    pub status: String,
    pub alternatives: Vec<String>,
    pub selected_option: String,
    pub rationale: String,
    pub positive_consequences: Option<Vec<String>>,
    pub negative_consequences: Option<Vec<String>>,
    pub affected_refs: Option<Vec<Id>>,
    pub supersedes: Option<Id>,
}

/// §12.9.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Technology {
    pub name: String,
    pub technology_kind: String,
    pub vendor: Option<String>,
    pub version_scheme: Option<String>,
}

/// §12.10.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TechnologySelection {
    pub technology_ref: Id,
    pub status: TechnologySelectionStatus,
    pub version_range: Option<String>,
    pub alternatives: Option<Vec<Id>>,
    pub rationale: Option<String>,
    pub architecture_decision_ref: Option<Id>,
}

// ============================================================================ technical contracts

/// §13.1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiContract {
    pub name: String,
    pub contract_kind: ApiContractKind,
    pub version: Option<String>,
    pub base_uri: Option<String>,
    pub external_spec_ref: Option<String>,
}

/// §13.2. Whether `method`/`path` are mandatory for HTTP is graph-semantic validation;
/// schemas are `schema_for` relations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApiOperation {
    pub api_contract_ref: Id,
    pub operation_id: String,
    pub method: Option<String>,
    pub path: Option<String>,
    pub security_refs: Option<Vec<Id>>,
}

/// §13.3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventContract {
    pub name: String,
    pub external_spec_ref: Option<String>,
}

/// §13.4.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Channel {
    pub name: String,
    pub address: String,
    pub protocol: Option<String>,
}

/// §13.5.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub name: String,
    pub correlation_ref: Option<Id>,
}

/// §13.6.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataSchema {
    pub name: String,
    pub schema_kind: String,
    pub external_ref: Option<String>,
    pub inline_schema: Option<Value>,
}

/// §13.7.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TechnicalWorkflow {
    pub name: String,
}

// ============================================================================ delivery

/// §14.1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capability {
    pub name: String,
    pub goal_refs: Option<Vec<Id>>,
    pub requirement_refs: Option<Vec<Id>>,
}

/// §14.2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImplementationSlice {
    pub name: String,
    pub intent: String,
    pub sequence_hint: Option<String>,
    pub size_hint: Option<String>,
}

/// §14.3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkPackage {
    pub name: String,
    pub owner_ref: Option<Id>,
    pub target_release_ref: Option<Id>,
}

/// §14.4.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskContract {
    pub name: String,
    pub intent: String,
    pub must: Vec<String>,
    pub must_not: Vec<String>,
    pub allowed_change_refs: Vec<Id>,
    pub forbidden_change_refs: Vec<Id>,
    pub completion_criteria: Vec<String>,
    pub implementation_slice_ref: Option<Id>,
    pub decision_refs: Option<Vec<Id>>,
    pub verification_refs: Option<Vec<Id>>,
}

/// §14.5.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Migration {
    pub name: String,
    pub migration_kind: String,
    pub source_ref: Id,
    pub target_ref: Id,
}

/// §14.6. `target_date` is a calendar date string.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub name: String,
    pub version: String,
    pub target_date: Option<String>,
}

// ============================================================================ verification

/// §15.1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationObligation {
    pub name: String,
    pub verification_kind: VerificationKind,
    pub method: Option<String>,
    pub acceptance_condition: Option<String>,
    pub required_evidence_kind: Option<String>,
}

/// §15.2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestCase {
    pub name: String,
    pub steps: Vec<Value>,
    pub expected: Vec<Value>,
    pub automation_ref: Option<String>,
}

/// §15.3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioRun {
    pub scenario_ref: Id,
    pub specification_hash: Hash,
    pub result: ScenarioRunResult,
    pub trace: Vec<Value>,
}

/// §15.4.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestExecution {
    pub test_case_ref: Id,
    pub result: String,
    pub started_at: Timestamp,
    pub finished_at: Timestamp,
}

/// §15.5.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestReceipt {
    pub verification_obligation_ref: Id,
    pub specification_hash: Hash,
    pub code_revision: String,
    pub test_revision: String,
    pub environment_ref: Id,
    pub result: String,
    pub executed_at: Timestamp,
    pub artifact_hashes: Option<Vec<Hash>>,
    pub logs_ref: Option<Hash>,
    pub runner_identity: Option<String>,
}

/// §15.6. `repository_ref` is a plain string.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodeBinding {
    pub repository_ref: String,
    pub code_locator: String,
    pub code_revision: String,
}

/// §15.7.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureCheck {
    pub rule_code: String,
    pub architecture_hash: Hash,
    pub result: String,
    pub affected_refs: Vec<Id>,
}

/// §15.8.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoverageRecord {
    pub coverage_kind: String,
    pub source_refs: Vec<Id>,
    pub target_refs: Vec<Id>,
    pub status: String,
}

// ============================================================================ standards profile

/// One standard referenced by a [`StandardsProfile`] (§16.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandardsProfileStandard {
    pub standard_id: String,
    pub version: String,
    pub role: MappingRole,
    pub required: bool,
}

/// §16.1. A node carrying this payload must have `Node.id == id`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StandardsProfile {
    pub id: Id,
    pub name: String,
    pub version: String,
    pub standards: Vec<StandardsProfileStandard>,
    pub validation_packs: Vec<String>,
}

// ============================================================================ extension

/// The only generic semantic escape hatch. Must not carry core semantics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionPayload {
    pub extension_type: ExtensionKey,
    pub data: Value,
}

// ============================================================================ payload validation

/// Why a `NodePayload` fails its programmatic validation (the same rules deserialization applies).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NodePayloadError {
    #[error("invalid evidence locator: {0}")]
    InvalidEvidenceLocator(#[from] EvidenceLocatorError),
    #[error("invalid derivation record: {0}")]
    InvalidDerivationRecord(#[from] DerivationRecordError),
    #[error("invalid resolution decision: {0}")]
    InvalidResolutionDecision(#[from] ResolutionDecisionError),
}

// ============================================================================ NodePayload / NodeType

/// Declares `NodePayload` and `NodeType` from one variant list so the two stay one-to-one.
macro_rules! node_payloads {
    ($($variant:ident($payload:ty)),+ $(,)?) => {
        /// The semantic content of a PSG node (metamodel §24). Serialized as
        /// `{"type": "<Variant>", "data": {...}}`.
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        #[serde(tag = "type", content = "data", deny_unknown_fields)]
        pub enum NodePayload {
            $($variant($payload)),+
        }

        /// Closed discriminator with exactly one variant per [`NodePayload`] variant.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, std::hash::Hash)]
        pub enum NodeType {
            $($variant),+
        }

        impl NodeType {
            /// Every node type, in metamodel §24 order.
            pub const ALL: &'static [NodeType] = &[$(NodeType::$variant),+];

            /// The exact `NodePayload` `type` tag for this node type.
            pub fn as_str(self) -> &'static str {
                match self {
                    $(NodeType::$variant => stringify!($variant)),+
                }
            }
        }

        impl NodePayload {
            /// Validates typed sub-structures exactly as deserialization does:
            /// `EvidenceFragment.locator`, `DerivationRecord` and `ResolutionDecision`.
            pub fn validate(&self) -> Result<(), NodePayloadError> {
                match self {
                    NodePayload::EvidenceFragment(fragment) => fragment.locator.validate()?,
                    NodePayload::DerivationRecord(record) => record.validate()?,
                    NodePayload::ResolutionDecision(decision) => decision.validate()?,
                    _ => {}
                }
                Ok(())
            }

            /// The discriminator of this payload.
            pub fn node_type(&self) -> NodeType {
                match self {
                    $(NodePayload::$variant(_) => NodeType::$variant),+
                }
            }
        }
    };
}

node_payloads! {
    // Evidence / governance
    SourceArtifact(SourceArtifact),
    EvidenceFragment(EvidenceFragment),
    DerivationRecord(DerivationRecord),
    Agent(Agent),
    Finding(Finding),
    Question(Question),
    ResolutionDecision(ResolutionDecision),
    Assumption(Assumption),

    // Intent / requirements
    Stakeholder(Stakeholder),
    Concern(Concern),
    Goal(Goal),
    Need(Need),
    Requirement(Requirement),
    AcceptanceCriterion(AcceptanceCriterion),
    Constraint(Constraint),

    // Vocabulary / domain
    Term(Term),
    Concept(Concept),
    Actor(Actor),
    BusinessRole(BusinessRole),
    Entity(Entity),
    Attribute(Attribute),
    DomainRelationship(DomainRelationship),
    State(State),
    Transition(Transition),
    Invariant(Invariant),

    // Functional
    Operation(Operation),
    Outcome(Outcome),
    Event(Event),
    Process(Process),
    ProcessNode(ProcessNode),
    Rule(Rule),
    DecisionTable(DecisionTable),
    Calculation(Calculation),
    Calendar(Calendar),
    Scenario(Scenario),

    // Authorization
    Principal(Principal),
    SecurityRole(SecurityRole),
    Permission(Permission),
    ResourceScope(ResourceScope),
    PolicyCondition(PolicyCondition),
    SeparationConstraint(SeparationConstraint),

    // Quality
    QualityCharacteristic(QualityCharacteristic),
    Measure(Measure),
    QualityScenario(QualityScenario),

    // Architecture
    SystemOfInterest(SystemOfInterest),
    ArchitectureDescription(ArchitectureDescription),
    ArchitectureCandidate(ArchitectureCandidate),
    Viewpoint(Viewpoint),
    View(View),
    ModelKind(ModelKind),
    SoftwareSystem(ArchitectureElement),
    Container(ArchitectureElement),
    Component(ArchitectureElement),
    Module(ArchitectureElement),
    Interface(ArchitectureElement),
    DataStore(ArchitectureElement),
    ExternalSystem(ArchitectureElement),
    DeploymentNode(ArchitectureElement),
    RuntimeEnvironment(ArchitectureElement),
    NetworkZone(ArchitectureElement),
    ArchitectureDecision(ArchitectureDecision),
    Technology(Technology),
    TechnologySelection(TechnologySelection),

    // Technical contracts
    ApiContract(ApiContract),
    ApiOperation(ApiOperation),
    EventContract(EventContract),
    Channel(Channel),
    Message(Message),
    DataSchema(DataSchema),
    TechnicalWorkflow(TechnicalWorkflow),

    // Delivery
    Capability(Capability),
    ImplementationSlice(ImplementationSlice),
    WorkPackage(WorkPackage),
    TaskContract(TaskContract),
    Migration(Migration),
    Release(Release),

    // Verification
    VerificationObligation(VerificationObligation),
    TestCase(TestCase),
    ScenarioRun(ScenarioRun),
    TestExecution(TestExecution),
    TestReceipt(TestReceipt),
    CodeBinding(CodeBinding),
    ArchitectureCheck(ArchitectureCheck),
    CoverageRecord(CoverageRecord),

    // Standards
    StandardsProfile(StandardsProfile),

    // Namespaced extension
    Extension(ExtensionPayload),
}
