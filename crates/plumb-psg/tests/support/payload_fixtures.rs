//! The golden `NodePayload` fixtures shared by the PSG test crates: one fixed, hand-authored
//! fixture per variant, in metamodel §24 order.

#![allow(dead_code)]

use serde_json::{json, Value};

pub const H1: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
pub const H2: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
pub const H3: &str = "sha256:3333333333333333333333333333333333333333333333333333333333333333";
pub const H4: &str = "sha256:4444444444444444444444444444444444444444444444444444444444444444";
pub const H5: &str = "sha256:5555555555555555555555555555555555555555555555555555555555555555";
pub const T1: &str = "2026-09-29T08:00:00.000000000Z";
pub const T2: &str = "2026-09-29T09:30:00.250000000Z";
pub const T3: &str = "2026-12-31T23:59:59.999999999Z";

/// A hand-authored golden fixture: variant tag, complete `data` object, required fields.
pub struct Fixture {
    pub tag: &'static str,
    pub data: Value,
    pub required: &'static [&'static str],
}

pub fn fx(tag: &'static str, data: Value, required: &'static [&'static str]) -> Fixture {
    Fixture {
        tag,
        data,
        required,
    }
}

pub fn arch_element(name: &str) -> Value {
    json!({
        "name": name,
        "description": "Architecture element fixture",
        "responsibilities": ["serve leave requests"],
        "owner_ref": "actor:hr:platform-team"
    })
}

/// One fixed golden fixture per `NodePayload` variant, in metamodel §24 order.
pub fn fixtures() -> Vec<Fixture> {
    vec![
        fx(
            "SourceArtifact",
            json!({
                "source_kind": "docx",
                "display_name": "requirements.docx",
                "content_hash": H1,
                "media_type": "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                "external_uri": null,
                "external_version": null,
                "producer": "HR team",
                "created_at_source": T1,
                "language": "en",
                "classification": null
            }),
            &["source_kind", "display_name", "content_hash", "media_type"],
        ),
        fx(
            "EvidenceFragment",
            json!({
                "source_ref": "src:0123456789abcdef",
                "locator": {"kind": "TextRange", "data": {"start": 0, "end": 42}},
                "content_hash": H2,
                "extracted_text": "Employees shall submit leave requests.",
                "speaker": null,
                "source_timestamp": null
            }),
            &["source_ref", "locator", "content_hash"],
        ),
        fx(
            "DerivationRecord",
            json!({
                "id": "drv:s0:segment-1",
                "kind": "llm_inference",
                "stage": "S0.segment",
                "input_refs": ["evd:0123456789abcdef", H1],
                "output_refs": ["prop:0123456789abcdef"],
                "created_at": T1,
                "provider": "anthropic",
                "model": "claude-opus-5-5",
                "prompt_template_hash": H1,
                "schema_hash": H2,
                "context_hash": H3,
                "parameters": {"temperature": 0},
                "raw_response_hash": H4,
                "validated_output_hash": H5
            }),
            &[
                "id",
                "kind",
                "stage",
                "input_refs",
                "output_refs",
                "created_at",
            ],
        ),
        fx("Agent", json!({"agent_kind": "llm_model"}), &["agent_kind"]),
        fx(
            "Finding",
            json!({
                "code": "PLUMB.F1.REQ.EVIDENCE",
                "family": "requirements",
                "severity": "blocker",
                "message": "Requirement has no evidence.",
                "status": "Open",
                "affected_refs": ["req:HR-001"],
                "standard_rule_ref": "ISO29148.F1.REQ.GROUNDED",
                "suggested_resolution": null,
                "waiver_ref": null
            }),
            &[
                "code",
                "family",
                "severity",
                "message",
                "status",
                "affected_refs",
            ],
        ),
        fx(
            "Question",
            json!({
                "finding_ref": "fnd:0123456789abcdef",
                "question_kind": "Cardinality",
                "prompt": "How many approvers does a leave request have?",
                "status": "open",
                "answer_schema": {"type": "string"},
                "stakeholder_ref": "stakeholder:hr-lead",
                "priority": "high",
                "round_ref": null,
                "context_refs": ["req:HR-004"]
            }),
            &["finding_ref", "question_kind", "prompt", "status"],
        ),
        fx(
            "ResolutionDecision",
            json!({
                "question_ref": "q:0123456789abcdef",
                "proposal_ref": null,
                "answer": {"cardinality": "one_to_many"},
                "decided_by": "actor:hr-lead",
                "decided_at": T2,
                "patch_ref": H3,
                "rationale": "Confirmed in workshop.",
                "supersedes": null
            }),
            &["answer", "decided_by", "decided_at", "patch_ref"],
        ),
        fx(
            "Assumption",
            json!({
                "statement": "Half days count as 0.5.",
                "owner_ref": "actor:analyst",
                "status": "active",
                "finding_ref": null,
                "default_value": 0.5,
                "expires_at": T3,
                "risk_ref": null
            }),
            &["statement", "owner_ref", "status"],
        ),
        fx(
            "Stakeholder",
            json!({
                "name": "HR Lead",
                "stakeholder_kind": "business_owner",
                "organization": "HR",
                "responsibilities": ["approve leave policy"],
                "contact_ref": "mailto:hr-lead@example.com"
            }),
            &["name", "stakeholder_kind"],
        ),
        fx(
            "Concern",
            json!({
                "name": "Correct balances",
                "description": "Leave balances must never go negative."
            }),
            &["name", "description"],
        ),
        fx(
            "Goal",
            json!({
                "statement": "Reduce leave processing time.",
                "success_measures": ["median approval under 1 day"],
                "priority": null
            }),
            &["statement"],
        ),
        fx(
            "Need",
            json!({
                "statement": "Employees need to request leave online.",
                "stakeholder_refs": ["stakeholder:employee"],
                "goal_refs": null,
                "context": "Remote teams"
            }),
            &["statement", "stakeholder_refs"],
        ),
        fx(
            "Requirement",
            json!({
                "statement": "The system shall let an employee submit a leave request.",
                "requirement_kind": "functional",
                "level": "system",
                "modality": "shall",
                "title": "Submit leave request",
                "rationale": null,
                "priority": "must",
                "source_identifier": "HR-001",
                "verification_method": "scenario",
                "owner_refs": null,
                "stakeholder_refs": ["stakeholder:employee"]
            }),
            &["statement", "requirement_kind", "level", "modality"],
        ),
        fx(
            "AcceptanceCriterion",
            json!({
                "statement": "Given a balance, when submitting, then the request is Submitted.",
                "criterion_kind": "given_when_then",
                "verification_method": null,
                "measure_ref": null,
                "scenario_ref": "scn:hr:submit-1"
            }),
            &["statement", "criterion_kind"],
        ),
        fx(
            "Constraint",
            json!({
                "statement": "Leave data must stay in the EU.",
                "constraint_category": "regulatory",
                "strength": "mandatory"
            }),
            &["statement", "constraint_category", "strength"],
        ),
        fx(
            "Term",
            json!({
                "term": "leave request",
                "language": "en",
                "definition_ref": null,
                "aliases": ["absence request"],
                "status": "accepted"
            }),
            &["term", "language"],
        ),
        fx(
            "Concept",
            json!({
                "name": "LeaveRequest",
                "definition": "A request by an employee for time off.",
                "concept_kind": "object_type"
            }),
            &["name", "definition", "concept_kind"],
        ),
        fx(
            "Actor",
            json!({"name": "Employee", "actor_kind": "human"}),
            &["name", "actor_kind"],
        ),
        fx("BusinessRole", json!({"name": "Approver"}), &["name"]),
        fx(
            "Entity",
            json!({
                "name": "LeaveRequest",
                "description": null,
                "aggregate_root": true
            }),
            &["name"],
        ),
        fx(
            "Attribute",
            json!({
                "name": "days",
                "value_type": "decimal",
                "nullable": false,
                "unit": "day",
                "precision": 1,
                "enum_values": null,
                "data_classification": "internal"
            }),
            &["name", "value_type", "nullable"],
        ),
        fx(
            "DomainRelationship",
            json!({
                "from_entity": "ent:hr:employee",
                "to_entity": "ent:hr:leave-request",
                "relationship_kind": "association",
                "cardinality_from": "1",
                "cardinality_to": "0..*",
                "name": "submits",
                "snapshot_semantics": false,
                "ownership": null
            }),
            &[
                "from_entity",
                "to_entity",
                "relationship_kind",
                "cardinality_from",
                "cardinality_to",
            ],
        ),
        fx("State", json!({"name": "Submitted"}), &["name"]),
        fx(
            "Transition",
            json!({
                "stateful_ref": "ent:hr:leave-request",
                "from_state": "state:hr:submitted",
                "to_state": "state:hr:approved",
                "guard_expr": "request.days <= balance.remaining",
                "effect_refs": ["evt:hr:approved"]
            }),
            &["stateful_ref", "from_state", "to_state"],
        ),
        fx(
            "Invariant",
            json!({
                "scope_ref": "ent:hr:leave-balance",
                "expression": "remaining >= 0"
            }),
            &["scope_ref", "expression"],
        ),
        fx(
            "Operation",
            json!({
                "name": "ApproveLeaveRequest",
                "operation_kind": "command",
                "input_schema_ref": null,
                "output_schema_ref": null,
                "preconditions": ["request.status = Submitted"],
                "postconditions": ["request.status = Approved"],
                "idempotency": "idempotent",
                "transaction_semantics": null
            }),
            &["name", "operation_kind"],
        ),
        fx(
            "Outcome",
            json!({"name": "InsufficientBalance", "outcome_kind": "business_failure"}),
            &["name", "outcome_kind"],
        ),
        fx(
            "Event",
            json!({
                "name": "LeaveApproved",
                "payload_schema_ref": null,
                "semantic_type": "domain_event"
            }),
            &["name"],
        ),
        fx(
            "Process",
            json!({
                "name": "Leave approval",
                "description": null,
                "process_kind": "approval"
            }),
            &["name"],
        ),
        fx(
            "ProcessNode",
            json!({
                "process_ref": "proc:hr:leave-approval",
                "node_kind": "exclusive_gateway",
                "operation_ref": null,
                "condition_expr": "request.days > 10",
                "message_ref": null,
                "timer_expr": null
            }),
            &["process_ref", "node_kind"],
        ),
        fx(
            "Rule",
            json!({
                "name": "Maximum consecutive days",
                "rule_kind": "validation",
                "condition_expr": "request.days > 20",
                "action_expr": null
            }),
            &["name", "rule_kind"],
        ),
        fx(
            "DecisionTable",
            json!({
                "name": "Approval routing",
                "hit_policy": "UNIQUE",
                "inputs": [{"name": "days", "type": "decimal"}],
                "outputs": [{"name": "approver", "type": "text"}],
                "rows": [{"when": ["<= 5"], "then": ["manager"]}]
            }),
            &["name", "hit_policy", "inputs", "outputs", "rows"],
        ),
        fx(
            "Calculation",
            json!({
                "name": "LeaveDays",
                "expression": "working_days(period, calendar, true)",
                "result_type": "decimal",
                "unit": "day",
                "rounding": "half_up",
                "calendar_ref": "cal:hr:pl",
                "examples": [{"input": {"from": "2026-01-05", "to": "2026-01-09"}, "output": 5}]
            }),
            &["name", "expression", "result_type"],
        ),
        fx(
            "Calendar",
            json!({
                "time_zone": "Europe/Warsaw",
                "week_pattern": {"mon": true, "sat": false},
                "region": "PL",
                "holiday_source": null
            }),
            &["time_zone", "week_pattern"],
        ),
        fx(
            "Scenario",
            json!({
                "name": "Half-day request",
                "scenario_kind": "boundary",
                "given": [{"balance": 10}],
                "when": {"operation": "op:hr:submit"},
                "then": [{"outcome": "success"}],
                "requirement_refs": ["req:HR-001"],
                "derived_from_refs": null,
                "confirmation_status": "confirmed"
            }),
            &["name", "scenario_kind", "given", "when", "then"],
        ),
        fx(
            "Principal",
            json!({"name": "HR portal user", "principal_kind": "user"}),
            &["name", "principal_kind"],
        ),
        fx(
            "SecurityRole",
            json!({"name": "hr-approver", "description": null}),
            &["name"],
        ),
        fx("Permission", json!({"name": "approve-leave"}), &["name"]),
        fx(
            "ResourceScope",
            json!({
                "resource_ref": "ent:hr:leave-request",
                "scope_kind": "organizational_unit"
            }),
            &["resource_ref", "scope_kind"],
        ),
        fx(
            "PolicyCondition",
            json!({"expression": "request.employee.manager = principal"}),
            &["expression"],
        ),
        fx(
            "SeparationConstraint",
            json!({
                "constraint_kind": "static_separation_of_duty",
                "role_refs": ["role:hr:requester", "role:hr:approver"]
            }),
            &["constraint_kind", "role_refs"],
        ),
        fx(
            "QualityCharacteristic",
            json!({
                "name": "Performance efficiency",
                "scheme": "ISO/IEC 25010:2023"
            }),
            &["name", "scheme"],
        ),
        fx(
            "Measure",
            json!({
                "name": "p95 latency",
                "measure_type": "latency_percentile",
                "unit": "ms",
                "aggregation": "p95",
                "sampling_window": null,
                "measurement_method": null
            }),
            &["name", "measure_type", "unit"],
        ),
        fx(
            "QualityScenario",
            json!({
                "stimulus": "5000 concurrent users submit leave requests",
                "response": "requests are accepted and processed",
                "threshold": "<= 400 ms",
                "source_ref": null,
                "environment_condition": "normal production operation",
                "affected_refs": ["api:hr:leave"],
                "priority": null
            }),
            &["stimulus", "response", "threshold"],
        ),
        fx(
            "SystemOfInterest",
            json!({"name": "Leave Management"}),
            &["name"],
        ),
        fx(
            "ArchitectureDescription",
            json!({
                "system_of_interest_ref": "soi:hr:leave",
                "purpose": null,
                "scope": "pilot",
                "architecture_candidate_ref": null
            }),
            &["system_of_interest_ref"],
        ),
        fx(
            "ArchitectureCandidate",
            json!({"name": "Modular monolith", "status": "exploring"}),
            &["name", "status"],
        ),
        fx(
            "Viewpoint",
            json!({
                "name": "Functional",
                "concern_refs": ["concern:hr:correctness"],
                "stakeholder_refs": ["stakeholder:hr-lead"],
                "model_kind_refs": ["mk:hr:process"],
                "purpose": null,
                "conventions": null
            }),
            &[
                "name",
                "concern_refs",
                "stakeholder_refs",
                "model_kind_refs",
            ],
        ),
        fx(
            "View",
            json!({
                "name": "Leave process",
                "viewpoint_ref": "vp:hr:functional",
                "architecture_description_ref": "ad:hr:leave",
                "root_refs": ["proc:hr:leave-approval"],
                "filter": {"namespace": "hr"},
                "projection_rules": ["include ProcessNode"],
                "layout_ref": H4,
                "style_ref": null
            }),
            &[
                "name",
                "viewpoint_ref",
                "architecture_description_ref",
                "projection_rules",
            ],
        ),
        fx(
            "ModelKind",
            json!({"name": "process", "semantic_scope": "business process"}),
            &["name", "semantic_scope"],
        ),
        fx(
            "SoftwareSystem",
            arch_element("Leave Management System"),
            &["name"],
        ),
        fx("Container", arch_element("Leave API"), &["name"]),
        fx("Component", arch_element("Approval service"), &["name"]),
        fx("Module", arch_element("leave-core"), &["name"]),
        fx("Interface", arch_element("Approval port"), &["name"]),
        fx("DataStore", arch_element("Leave database"), &["name"]),
        fx("ExternalSystem", arch_element("Payroll"), &["name"]),
        fx("DeploymentNode", arch_element("eu-west cluster"), &["name"]),
        fx("RuntimeEnvironment", arch_element("JVM 21"), &["name"]),
        fx("NetworkZone", arch_element("Internal zone"), &["name"]),
        fx(
            "ArchitectureDecision",
            json!({
                "question": "Monolith or microservices?",
                "status": "accepted",
                "alternatives": ["Modular monolith", "Microservices"],
                "selected_option": "Modular monolith",
                "rationale": "Small team, single domain.",
                "positive_consequences": ["simpler operations"],
                "negative_consequences": null,
                "affected_refs": null,
                "supersedes": null
            }),
            &[
                "question",
                "status",
                "alternatives",
                "selected_option",
                "rationale",
            ],
        ),
        fx(
            "Technology",
            json!({
                "name": "PostgreSQL",
                "technology_kind": "database",
                "vendor": "PostgreSQL Global Development Group",
                "version_scheme": "major.minor"
            }),
            &["name", "technology_kind"],
        ),
        fx(
            "TechnologySelection",
            json!({
                "technology_ref": "tech:postgresql",
                "status": "selected",
                "version_range": ">=16",
                "alternatives": ["tech:sqlite"],
                "rationale": null,
                "architecture_decision_ref": "adr:hr:1"
            }),
            &["technology_ref", "status"],
        ),
        fx(
            "ApiContract",
            json!({
                "name": "Leave API",
                "contract_kind": "http",
                "version": "1.0.0",
                "base_uri": "/api",
                "external_spec_ref": "https://example.com/specs/leave-openapi.yaml"
            }),
            &["name", "contract_kind"],
        ),
        fx(
            "ApiOperation",
            json!({
                "api_contract_ref": "api:hr:leave",
                "operation_id": "approveLeave",
                "method": "POST",
                "path": "/leave/{id}/approve",
                "security_refs": null
            }),
            &["api_contract_ref", "operation_id"],
        ),
        fx(
            "EventContract",
            json!({"name": "Leave events", "external_spec_ref": "specs/leave asyncapi.yaml"}),
            &["name"],
        ),
        fx(
            "Channel",
            json!({
                "name": "leave.approved",
                "address": "hr/leave/approved",
                "protocol": "amqp"
            }),
            &["name", "address"],
        ),
        fx(
            "Message",
            json!({"name": "LeaveApprovedMessage", "correlation_ref": null}),
            &["name"],
        ),
        fx(
            "DataSchema",
            json!({
                "name": "LeaveRequest",
                "schema_kind": "json_schema",
                "external_ref": "schemas/LeaveRequest.json#/definitions/Leave",
                "inline_schema": {"type": "object"}
            }),
            &["name", "schema_kind"],
        ),
        fx(
            "TechnicalWorkflow",
            json!({"name": "Approve flow"}),
            &["name"],
        ),
        fx(
            "Capability",
            json!({
                "name": "Leave management",
                "goal_refs": null,
                "requirement_refs": ["req:HR-001"]
            }),
            &["name"],
        ),
        fx(
            "ImplementationSlice",
            json!({
                "name": "Slice 1",
                "intent": "Submit and approve leave",
                "sequence_hint": "1",
                "size_hint": "M"
            }),
            &["name", "intent"],
        ),
        fx(
            "WorkPackage",
            json!({
                "name": "WP-1",
                "owner_ref": null,
                "target_release_ref": "release:hr:1"
            }),
            &["name"],
        ),
        fx(
            "TaskContract",
            json!({
                "name": "Implement approval",
                "intent": "Approve submitted leave requests",
                "must": ["check balance"],
                "must_not": ["touch payroll"],
                "allowed_change_refs": ["module:hr:leave"],
                "forbidden_change_refs": ["module:hr:payroll"],
                "completion_criteria": ["all approval scenarios pass"],
                "implementation_slice_ref": "slice:hr:1",
                "decision_refs": null,
                "verification_refs": null
            }),
            &[
                "name",
                "intent",
                "must",
                "must_not",
                "allowed_change_refs",
                "forbidden_change_refs",
                "completion_criteria",
            ],
        ),
        fx(
            "Migration",
            json!({
                "name": "Import legacy balances",
                "migration_kind": "data",
                "source_ref": "ds:hr:legacy",
                "target_ref": "ds:hr:new"
            }),
            &["name", "migration_kind", "source_ref", "target_ref"],
        ),
        fx(
            "Release",
            json!({"name": "Pilot", "version": "0.1.0", "target_date": "2026-12-01"}),
            &["name", "version"],
        ),
        fx(
            "VerificationObligation",
            json!({
                "name": "Verify approval",
                "verification_kind": "scenario",
                "method": null,
                "acceptance_condition": "all approval scenarios pass",
                "required_evidence_kind": null
            }),
            &["name", "verification_kind"],
        ),
        fx(
            "TestCase",
            json!({
                "name": "Approve happy path",
                "steps": [{"action": "submit"}],
                "expected": [{"status": "Approved"}],
                "automation_ref": "tests/e2e/Approve Leave.spec.ts"
            }),
            &["name", "steps", "expected"],
        ),
        fx(
            "ScenarioRun",
            json!({
                "scenario_ref": "scn:hr:half-day",
                "specification_hash": H5,
                "result": "undecidable",
                "trace": [{"step": "materialize"}]
            }),
            &["scenario_ref", "specification_hash", "result", "trace"],
        ),
        fx(
            "TestExecution",
            json!({
                "test_case_ref": "test:hr:approve",
                "result": "passed",
                "started_at": T1,
                "finished_at": T2
            }),
            &["test_case_ref", "result", "started_at", "finished_at"],
        ),
        fx(
            "TestReceipt",
            json!({
                "verification_obligation_ref": "verify:hr:approval",
                "specification_hash": H5,
                "code_revision": "abc123",
                "test_revision": "def456",
                "environment_ref": "env:ci",
                "result": "passed",
                "executed_at": T2,
                "artifact_hashes": [H1],
                "logs_ref": H2,
                "runner_identity": null
            }),
            &[
                "verification_obligation_ref",
                "specification_hash",
                "code_revision",
                "test_revision",
                "environment_ref",
                "result",
                "executed_at",
            ],
        ),
        fx(
            "CodeBinding",
            json!({
                "repository_ref": "git@example.com:acme/leave.git",
                "code_locator": "src/approve.rs::approve",
                "code_revision": "abc123"
            }),
            &["repository_ref", "code_locator", "code_revision"],
        ),
        fx(
            "ArchitectureCheck",
            json!({
                "rule_code": "PLUMB.A2.ALLOCATED",
                "architecture_hash": H3,
                "result": "pass",
                "affected_refs": []
            }),
            &["rule_code", "architecture_hash", "result", "affected_refs"],
        ),
        fx(
            "CoverageRecord",
            json!({
                "coverage_kind": "requirement_to_scenario",
                "source_refs": ["req:HR-001"],
                "target_refs": ["scn:hr:half-day"],
                "status": "covered"
            }),
            &["coverage_kind", "source_refs", "target_refs", "status"],
        ),
        fx(
            "StandardsProfile",
            json!({
                "id": "profile:plumb-software-2026.1",
                "name": "Plumb software profile",
                "version": "2026.1",
                "standards": [{
                    "standard_id": "ISO/IEC 25010",
                    "version": "2023",
                    "role": "taxonomy",
                    "required": true
                }],
                "validation_packs": ["I0", "F1"]
            }),
            &["id", "name", "version", "standards", "validation_packs"],
        ),
        fx(
            "Extension",
            json!({
                "extension_type": "acme:risk_register",
                "data": {"risk": "high", "score": 7}
            }),
            &["extension_type", "data"],
        ),
    ]
}
