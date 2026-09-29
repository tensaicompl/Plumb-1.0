//! F0.4 contract tests for `NodePayload`, `NodeType` and `Node`.
//!
//! Every test lives in `payload_roundtrip` so that the task command
//! `cargo test -p plumb-psg payload_roundtrip` selects all of them.

mod payload_roundtrip {
    use std::collections::{BTreeMap, BTreeSet};

    use plumb_core::Id;
    use plumb_psg::*;
    use serde_json::{json, Map, Value};

    const H1: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
    const H2: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";
    const H3: &str = "sha256:3333333333333333333333333333333333333333333333333333333333333333";
    const H4: &str = "sha256:4444444444444444444444444444444444444444444444444444444444444444";
    const H5: &str = "sha256:5555555555555555555555555555555555555555555555555555555555555555";
    const T1: &str = "2026-09-29T08:00:00.000000000Z";
    const T2: &str = "2026-09-29T09:30:00.250000000Z";
    const T3: &str = "2026-12-31T23:59:59.999999999Z";

    /// A hand-authored golden fixture: variant tag, complete `data` object, required fields.
    struct Fixture {
        tag: &'static str,
        data: Value,
        required: &'static [&'static str],
    }

    fn fx(tag: &'static str, data: Value, required: &'static [&'static str]) -> Fixture {
        Fixture {
            tag,
            data,
            required,
        }
    }

    fn arch_element(name: &str) -> Value {
        json!({
            "name": name,
            "description": "Architecture element fixture",
            "responsibilities": ["serve leave requests"],
            "technology_selection_refs": null,
            "owner_ref": "actor:hr:platform-team"
        })
    }

    /// One fixed golden fixture per `NodePayload` variant, in metamodel §24 order.
    fn fixtures() -> Vec<Fixture> {
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
                    "contact_ref": null
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
                    "entity_ref": "ent:hr:leave-request",
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
            fx(
                "State",
                json!({"name": "Submitted", "owner_ref": "ent:hr:leave-request"}),
                &["name", "owner_ref"],
            ),
            fx(
                "Transition",
                json!({
                    "stateful_ref": "ent:hr:leave-request",
                    "from_state": "Submitted",
                    "to_state": "Approved",
                    "trigger_ref": "op:hr:approve",
                    "guard_expr": "request.days <= balance.remaining",
                    "effect_refs": ["evt:hr:approved"]
                }),
                &["stateful_ref", "from_state", "to_state", "trigger_ref"],
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
                    "actor_ref": null,
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
            fx(
                "Permission",
                json!({
                    "name": "approve-leave",
                    "operation_ref": "op:hr:approve",
                    "resource_scope_ref": "scope:hr:own-team",
                    "policy_condition_refs": null
                }),
                &["name", "operation_ref", "resource_scope_ref"],
            ),
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
                    "quality_characteristic_ref": "quality:hr:performance",
                    "stimulus": "5000 concurrent users submit leave requests",
                    "response": "requests are accepted and processed",
                    "measure_ref": "measure:hr:p95",
                    "threshold": "<= 400 ms",
                    "source_ref": null,
                    "environment_condition": "normal production operation",
                    "affected_refs": ["api:hr:leave"],
                    "priority": null
                }),
                &[
                    "quality_characteristic_ref",
                    "stimulus",
                    "response",
                    "measure_ref",
                    "threshold",
                ],
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
                    "drivers": ["quality:hr:performance"],
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
                    "drivers",
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
                    "applies_to_refs": ["container:hr:db"],
                    "version_range": ">=16",
                    "alternatives": ["tech:sqlite"],
                    "drivers": null,
                    "rationale": null,
                    "architecture_decision_ref": "adr:hr:1"
                }),
                &["technology_ref", "status", "applies_to_refs"],
            ),
            fx(
                "ApiContract",
                json!({
                    "name": "Leave API",
                    "contract_kind": "http",
                    "version": "1.0.0",
                    "base_uri": "/api",
                    "external_spec_ref": null
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
                    "request_schema_ref": null,
                    "response_schema_refs": ["schema:hr:leave"],
                    "error_schema_refs": null,
                    "security_refs": null
                }),
                &["api_contract_ref", "operation_id"],
            ),
            fx(
                "EventContract",
                json!({"name": "Leave events", "external_spec_ref": null}),
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
                json!({
                    "name": "LeaveApprovedMessage",
                    "payload_schema_ref": "schema:hr:leave-approved",
                    "headers_schema_ref": null,
                    "correlation_ref": null
                }),
                &["name", "payload_schema_ref"],
            ),
            fx(
                "DataSchema",
                json!({
                    "name": "LeaveRequest",
                    "schema_kind": "json_schema",
                    "external_ref": null,
                    "inline_schema": {"type": "object"}
                }),
                &["name", "schema_kind"],
            ),
            fx(
                "TechnicalWorkflow",
                json!({
                    "name": "Approve flow",
                    "step_refs": ["apiop:hr:get", "apiop:hr:approve"]
                }),
                &["name", "step_refs"],
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
                json!({
                    "name": "Pilot",
                    "version": "0.1.0",
                    "target_date": "2026-12-01",
                    "slice_refs": null
                }),
                &["name", "version"],
            ),
            fx(
                "VerificationObligation",
                json!({
                    "name": "Verify approval",
                    "verification_kind": "scenario",
                    "target_refs": ["req:HR-001"],
                    "method": null,
                    "acceptance_condition": "all approval scenarios pass",
                    "required_evidence_kind": null
                }),
                &["name", "verification_kind", "target_refs"],
            ),
            fx(
                "TestCase",
                json!({
                    "name": "Approve happy path",
                    "steps": [{"action": "submit"}],
                    "expected": [{"status": "Approved"}],
                    "automation_ref": null,
                    "verification_obligation_refs": ["verify:hr:approval"]
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
                    "semantic_ref": "op:hr:approve",
                    "repository_ref": "git@example.com:acme/leave.git",
                    "code_locator": "src/approve.rs::approve",
                    "code_revision": "abc123"
                }),
                &[
                    "semantic_ref",
                    "repository_ref",
                    "code_locator",
                    "code_revision",
                ],
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

    /// Optional fields whose omission is rejected for a cross-field reason, not a type reason.
    const CROSS_FIELD_OPTIONALS: &[(&str, &str)] = &[
        ("DerivationRecord", "provider"),
        ("DerivationRecord", "model"),
        ("DerivationRecord", "prompt_template_hash"),
        ("DerivationRecord", "schema_hash"),
        ("DerivationRecord", "context_hash"),
        ("DerivationRecord", "parameters"),
        ("DerivationRecord", "raw_response_hash"),
        ("DerivationRecord", "validated_output_hash"),
        ("ResolutionDecision", "question_ref"),
    ];

    fn wire(tag: &str, data: &Value) -> Value {
        json!({"type": tag, "data": data})
    }

    fn parse(value: Value) -> Result<NodePayload, serde_json::Error> {
        serde_json::from_value(value)
    }

    fn fields(data: &Value) -> &Map<String, Value> {
        data.as_object().expect("fixture data is an object")
    }

    // ------------------------------------------------------------------ coverage and tags

    #[test]
    fn there_are_exactly_86_variants_and_fixtures() {
        let fixtures = fixtures();
        assert_eq!(fixtures.len(), 86);
        assert_eq!(NodeType::ALL.len(), 86);
        let fixture_tags: BTreeSet<&str> = fixtures.iter().map(|f| f.tag).collect();
        let type_tags: BTreeSet<&str> = NodeType::ALL.iter().map(|t| t.as_str()).collect();
        assert_eq!(fixture_tags.len(), 86, "fixture tags are unique");
        assert_eq!(type_tags.len(), 86, "NodeType tags are unique");
        assert_eq!(fixture_tags, type_tags);
    }

    #[test]
    fn every_golden_fixture_round_trips_exactly() {
        for f in fixtures() {
            let expected = wire(f.tag, &f.data);
            let payload = parse(expected.clone())
                .unwrap_or_else(|e| panic!("{} fixture should deserialize: {e}", f.tag));
            assert_eq!(
                serde_json::to_value(&payload).unwrap(),
                expected,
                "{}",
                f.tag
            );
            let text = serde_json::to_string(&payload).unwrap();
            assert_eq!(serde_json::from_str::<NodePayload>(&text).unwrap(), payload);
        }
    }

    #[test]
    fn every_variant_has_its_exact_type_tag_and_node_type() {
        for f in fixtures() {
            let payload = parse(wire(f.tag, &f.data)).unwrap();
            let node_type = payload.node_type();
            assert_eq!(node_type.as_str(), f.tag);
            assert_eq!(
                serde_json::to_value(&payload).unwrap()["type"],
                json!(f.tag)
            );
            let from_all = NodeType::ALL.iter().find(|t| t.as_str() == f.tag).copied();
            assert_eq!(from_all, Some(node_type), "{}", f.tag);
        }
    }

    #[test]
    fn node_type_order_follows_the_metamodel_skeleton() {
        let tags: Vec<&str> = fixtures().iter().map(|f| f.tag).collect();
        let types: Vec<&str> = NodeType::ALL.iter().map(|t| t.as_str()).collect();
        assert_eq!(tags, types);
        assert_eq!(NodeType::ALL[2], NodeType::DerivationRecord);
        assert_eq!(NodeType::ALL[1], NodeType::EvidenceFragment);
        assert_eq!(*NodeType::ALL.last().unwrap(), NodeType::Extension);
    }

    // ------------------------------------------------------------------ field presence

    #[test]
    fn every_required_field_omission_is_rejected() {
        let mut checked = 0;
        for f in fixtures() {
            for field in f.required {
                assert!(
                    fields(&f.data).contains_key(*field),
                    "{}.{field} missing from fixture",
                    f.tag
                );
                let mut data = f.data.clone();
                data.as_object_mut().unwrap().remove(*field);
                assert!(
                    parse(wire(f.tag, &data)).is_err(),
                    "{} without required {field} should be rejected",
                    f.tag
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 223);
    }

    #[test]
    fn every_optional_field_may_be_omitted_and_reserializes_as_null() {
        let mut checked = 0;
        for f in fixtures() {
            for field in fields(&f.data).keys() {
                if f.required.contains(&field.as_str())
                    || CROSS_FIELD_OPTIONALS.contains(&(f.tag, field.as_str()))
                {
                    continue;
                }
                let mut data = f.data.clone();
                data.as_object_mut().unwrap().remove(field);
                let payload = parse(wire(f.tag, &data))
                    .unwrap_or_else(|e| panic!("{} without optional {field}: {e}", f.tag));
                let reserialized = serde_json::to_value(&payload).unwrap();
                assert_eq!(
                    reserialized["data"][field],
                    Value::Null,
                    "{}.{field}",
                    f.tag
                );
                checked += 1;
            }
        }
        assert!(checked > 150, "only {checked} optional fields checked");
    }

    #[test]
    fn optional_none_serializes_as_explicit_null() {
        let payload = NodePayload::Entity(Entity {
            name: "LeaveRequest".into(),
            description: None,
            aggregate_root: None,
        });
        assert_eq!(
            serde_json::to_value(&payload).unwrap(),
            json!({"type": "Entity", "data": {"name": "LeaveRequest", "description": null, "aggregate_root": null}})
        );
    }

    // ------------------------------------------------------------------ unknown input

    #[test]
    fn unknown_payload_fields_are_rejected_for_every_variant() {
        for f in fixtures() {
            let mut data = f.data.clone();
            data.as_object_mut()
                .unwrap()
                .insert("confidence".into(), json!(0.9));
            assert!(
                parse(wire(f.tag, &data)).is_err(),
                "{} accepted an unknown field",
                f.tag
            );
        }
    }

    #[test]
    fn unknown_node_payload_types_are_rejected() {
        for tag in [
            "Unknown",
            "requirement",
            "REQUIREMENT",
            "ArchitectureElement",
            "Confirmed",
            "",
        ] {
            assert!(
                parse(json!({"type": tag, "data": {"name": "x"}})).is_err(),
                "{tag}"
            );
        }
        assert!(parse(json!({"data": {"name": "x"}})).is_err());
        assert!(parse(json!({"type": "BusinessRole"})).is_err());
    }

    #[test]
    fn extra_top_level_node_payload_fields_are_rejected() {
        let extra = json!({"type": "BusinessRole", "data": {"name": "Approver"}, "props": {}});
        assert!(parse(extra).is_err());
        let kind_props = json!({"kind": "BusinessRole", "props": {"name": "Approver"}});
        assert!(parse(kind_props).is_err());
    }

    // ------------------------------------------------------------------ closed vocabularies

    macro_rules! assert_closed_enum {
        ($ty:ident, [$($variant:ident => $text:literal),+ $(,)?]) => {{
            fn expected(v: $ty) -> &'static str {
                match v { $($ty::$variant => $text),+ }
            }
            $(
                assert_eq!(serde_json::to_value($ty::$variant).unwrap(), json!($text));
                let parsed: $ty = serde_json::from_value(json!($text)).unwrap();
                assert_eq!(expected(parsed), $text);
            )+
            for bad in ["", "unknown", "__not_a_value__"] {
                assert!(serde_json::from_value::<$ty>(json!(bad)).is_err(), "{} accepted {bad:?}", stringify!($ty));
            }
            $(
                let variant_name = stringify!($variant);
                if variant_name != $text {
                    assert!(serde_json::from_value::<$ty>(json!(variant_name)).is_err(),
                        "{} accepted the Rust variant name {variant_name}", stringify!($ty));
                }
            )+
        }};
    }

    #[test]
    fn all_closed_enums_serialize_exactly_and_reject_unknown_values() {
        assert_closed_enum!(DerivationKind, [DeterministicRule => "deterministic_rule", Parser => "parser",
            Import => "import", HumanEdit => "human_edit", HumanResolution => "human_resolution",
            LlmInference => "llm_inference", Recovery => "recovery", Migration => "migration",
            ExternalSync => "external_sync"]);
        assert_closed_enum!(AgentKind, [Human => "human", Organization => "organization",
            SoftwareService => "software_service", LlmModel => "llm_model",
            CompilerStage => "compiler_stage", ExternalSystem => "external_system"]);
        assert_closed_enum!(FindingSeverity, [Blocker => "blocker", Error => "error", Warn => "warn", Info => "info"]);
        assert_closed_enum!(QuestionKind, [YesNo => "YesNo", PickOne => "PickOne", PickMany => "PickMany",
            Number => "Number", Text => "Text", Cardinality => "Cardinality", Unit => "Unit",
            Precision => "Precision", Rounding => "Rounding", FormulaConfirm => "FormulaConfirm",
            RuleCell => "RuleCell", Calendar => "Calendar", RoleAssignment => "RoleAssignment",
            Permission => "Permission", QualityThreshold => "QualityThreshold",
            ArchitectureChoice => "ArchitectureChoice", TechnologyChoice => "TechnologyChoice",
            InterfaceChoice => "InterfaceChoice", VerificationMethod => "VerificationMethod"]);
        assert_closed_enum!(RequirementKind, [Functional => "functional", Quality => "quality",
            Interface => "interface", Data => "data", Security => "security", Operational => "operational",
            Compliance => "compliance", Transition => "transition", Constraint => "constraint"]);
        assert_closed_enum!(RequirementLevel, [Stakeholder => "stakeholder", System => "system",
            Software => "software", Subsystem => "subsystem", Component => "component", Interface => "interface"]);
        assert_closed_enum!(Modality, [Shall => "shall", Should => "should", May => "may", ShallNot => "shall_not"]);
        assert_closed_enum!(ConstraintCategory, [Business => "business", Technical => "technical",
            Technology => "technology", Security => "security", Data => "data", Integration => "integration",
            Operational => "operational", Legal => "legal", Regulatory => "regulatory",
            Organizational => "organizational", Legacy => "legacy"]);
        assert_closed_enum!(ConstraintStrength, [Mandatory => "mandatory", Preferred => "preferred", Prohibited => "prohibited"]);
        assert_closed_enum!(ConceptKind, [ObjectType => "object_type", FactType => "fact_type",
            ValueType => "value_type", Role => "role", Other => "other"]);
        assert_closed_enum!(ActorKind, [Human => "human", System => "system",
            ExternalSystem => "external_system", Organization => "organization"]);
        assert_closed_enum!(OperationKind, [Command => "command", Query => "query"]);
        assert_closed_enum!(OutcomeKind, [Success => "success", BusinessFailure => "business_failure",
            TechnicalFailure => "technical_failure", Partial => "partial"]);
        assert_closed_enum!(ProcessNodeKind, [Start => "start", End => "end", HumanTask => "human_task",
            ServiceTask => "service_task", ExclusiveGateway => "exclusive_gateway",
            ParallelSplit => "parallel_split", ParallelJoin => "parallel_join",
            MessageEvent => "message_event", TimerEvent => "timer_event", ErrorEvent => "error_event",
            Subprocess => "subprocess"]);
        assert_closed_enum!(RuleKind, [Constraint => "constraint", Derivation => "derivation",
            Permission => "permission", Validation => "validation", Business => "business"]);
        assert_closed_enum!(ScenarioKind, [Acceptance => "acceptance", Boundary => "boundary",
            Failure => "failure", StateTransition => "state_transition", RuleRow => "rule_row",
            Quality => "quality", Integration => "integration", Regression => "regression"]);
        assert_closed_enum!(SeparationConstraintKind, [StaticSeparationOfDuty => "static_separation_of_duty",
            DynamicSeparationOfDuty => "dynamic_separation_of_duty", MutualExclusion => "mutual_exclusion",
            RequiredCombination => "required_combination"]);
        assert_closed_enum!(ArchitectureCandidateStatus, [Exploring => "exploring", Candidate => "candidate",
            Accepted => "accepted", Rejected => "rejected", Superseded => "superseded"]);
        assert_closed_enum!(TechnologySelectionStatus, [Candidate => "candidate", Selected => "selected",
            Rejected => "rejected", Legacy => "legacy", Prohibited => "prohibited"]);
        assert_closed_enum!(ApiContractKind, [Http => "http", Rpc => "rpc", Other => "other"]);
        assert_closed_enum!(VerificationKind, [Test => "test", Scenario => "scenario", Analysis => "analysis",
            Inspection => "inspection", Review => "review", Demonstration => "demonstration",
            FormalCheck => "formal_check", ArchitectureCheck => "architecture_check",
            SecurityCheck => "security_check"]);
        assert_closed_enum!(ScenarioRunResult, [Pass => "pass", Fail => "fail", Undecidable => "undecidable"]);
    }

    #[test]
    fn closed_enum_fields_reject_unknown_values_inside_payloads() {
        for (tag, field, bad) in [
            ("Requirement", "requirement_kind", "nonfunctional"),
            ("Requirement", "modality", "must"),
            ("Finding", "severity", "warning"),
            ("Question", "question_kind", "yes_no"),
            ("Agent", "agent_kind", "robot"),
            ("TechnologySelection", "status", "approved"),
            ("ScenarioRun", "result", "passed"),
        ] {
            let f = fixtures().into_iter().find(|f| f.tag == tag).unwrap();
            let mut data = f.data.clone();
            data[field] = json!(bad);
            assert!(
                parse(wire(tag, &data)).is_err(),
                "{tag}.{field} accepted {bad}"
            );
        }
    }

    // ------------------------------------------------------------------ nested structures

    #[test]
    fn agent_round_trips() {
        let agent = NodePayload::Agent(Agent {
            agent_kind: AgentKind::CompilerStage,
        });
        let expected = json!({"type": "Agent", "data": {"agent_kind": "compiler_stage"}});
        assert_eq!(serde_json::to_value(&agent).unwrap(), expected);
        assert_eq!(parse(expected).unwrap(), agent);
    }

    #[test]
    fn evidence_locator_round_trips_all_seven_variants() {
        let cases = [
            json!({"kind": "TextRange", "data": {"start": 3, "end": 17}}),
            json!({"kind": "PageRegion", "data": {"page": 2, "x": 0.5, "y": 10.25, "width": null, "height": null}}),
            json!({"kind": "TableCell", "data": {"table": 1, "row": 4, "column": 2}}),
            json!({"kind": "XmlPath", "data": {"xpath": "/w:document/w:body/w:p[3]"}}),
            json!({"kind": "JsonPointer", "data": {"pointer": "/paths/~1leave/post"}}),
            json!({"kind": "ConversationTurn", "data": {"turn_id": "turn:ses:0001"}}),
            json!({"kind": "ExternalObject", "data": {"object_id": "JIRA-42", "field": "description"}}),
        ];
        for case in cases {
            let locator: EvidenceLocator = serde_json::from_value(case.clone()).unwrap();
            assert_eq!(serde_json::to_value(&locator).unwrap(), case);
        }
        let bad = [
            json!({"kind": "TextRange", "data": {"start": 3, "end": 17, "line": 1}}),
            json!({"kind": "TextRange", "data": {"start": -1, "end": 17}}),
            json!({"kind": "LineRange", "data": {"start": 1, "end": 2}}),
            json!({"kind": "ConversationTurn", "data": {"turn_id": "Not An Id"}}),
            json!({"kind": "TableCell", "data": {"table": 1, "row": 4}}),
            json!({"kind": "XmlPath", "data": {"xpath": "/a"}, "extra": 1}),
        ];
        for case in bad {
            assert!(
                serde_json::from_value::<EvidenceLocator>(case.clone()).is_err(),
                "{case}"
            );
        }
    }

    fn llm_record() -> Value {
        fixtures()
            .into_iter()
            .find(|f| f.tag == "DerivationRecord")
            .unwrap()
            .data
    }

    const LLM_FIELDS: [&str; 8] = [
        "provider",
        "model",
        "prompt_template_hash",
        "schema_hash",
        "context_hash",
        "parameters",
        "raw_response_hash",
        "validated_output_hash",
    ];

    #[test]
    fn llm_derivation_record_accepts_all_llm_fields() {
        let record: DerivationRecord = serde_json::from_value(llm_record()).unwrap();
        assert_eq!(record.kind, DerivationKind::LlmInference);
        assert_eq!(record.validate(), Ok(()));
        assert_eq!(serde_json::to_value(&record).unwrap(), llm_record());
    }

    #[test]
    fn llm_derivation_record_rejects_each_missing_llm_field() {
        let valid: DerivationRecord = serde_json::from_value(llm_record()).unwrap();
        for field in LLM_FIELDS {
            let mut omitted = llm_record();
            omitted.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<DerivationRecord>(omitted).is_err(),
                "omitted {field}"
            );
            let mut nulled = llm_record();
            nulled[field] = Value::Null;
            assert!(
                serde_json::from_value::<DerivationRecord>(nulled).is_err(),
                "null {field}"
            );
            let mut record = valid.clone();
            match field {
                "provider" => record.provider = None,
                "model" => record.model = None,
                "prompt_template_hash" => record.prompt_template_hash = None,
                "schema_hash" => record.schema_hash = None,
                "context_hash" => record.context_hash = None,
                "parameters" => record.parameters = None,
                "raw_response_hash" => record.raw_response_hash = None,
                _ => record.validated_output_hash = None,
            }
            assert_eq!(
                record.validate(),
                Err(DerivationRecordError::MissingLlmField(field))
            );
        }
    }

    #[test]
    fn non_llm_derivation_record_rejects_any_llm_field() {
        let mut base = llm_record();
        base["kind"] = json!("parser");
        for field in LLM_FIELDS {
            base[field] = Value::Null;
        }
        let parser: DerivationRecord = serde_json::from_value(base.clone()).unwrap();
        assert_eq!(parser.validate(), Ok(()));
        let full = llm_record();
        for field in LLM_FIELDS {
            let mut data = base.clone();
            data[field] = full[field].clone();
            assert!(
                serde_json::from_value::<DerivationRecord>(data).is_err(),
                "parser with {field}"
            );
        }
        let mut record = parser;
        record.model = Some("claude-opus-5-5".into());
        assert_eq!(
            record.validate(),
            Err(DerivationRecordError::UnexpectedLlmField {
                kind: DerivationKind::Parser,
                field: "model"
            })
        );
    }

    #[test]
    fn resolution_decision_requires_a_question_or_proposal() {
        let f = fixtures()
            .into_iter()
            .find(|f| f.tag == "ResolutionDecision")
            .unwrap();
        let mut neither = f.data.clone();
        neither["question_ref"] = Value::Null;
        assert!(parse(wire("ResolutionDecision", &neither)).is_err());
        let mut both = f.data.clone();
        both["proposal_ref"] = json!("prop:0123456789abcdef");
        let decision: ResolutionDecision = serde_json::from_value(both).unwrap();
        assert_eq!(decision.validate(), Ok(()));
        let mut only_proposal = f.data.clone();
        only_proposal["question_ref"] = Value::Null;
        only_proposal["proposal_ref"] = json!("prop:0123456789abcdef");
        assert!(serde_json::from_value::<ResolutionDecision>(only_proposal).is_ok());
        let mut programmatic: ResolutionDecision = serde_json::from_value(f.data).unwrap();
        programmatic.question_ref = None;
        assert_eq!(
            programmatic.validate(),
            Err(ResolutionDecisionError::MissingQuestionOrProposal)
        );
    }

    #[test]
    fn extension_payload_round_trips_and_requires_a_namespaced_type() {
        let payload = NodePayload::Extension(ExtensionPayload {
            extension_type: "acme:risk_register".parse().unwrap(),
            data: json!({"risk": "high"}),
        });
        let expected = json!({"type": "Extension", "data": {"extension_type": "acme:risk_register", "data": {"risk": "high"}}});
        assert_eq!(serde_json::to_value(&payload).unwrap(), expected);
        assert_eq!(parse(expected).unwrap(), payload);
        assert!(parse(
            json!({"type": "Extension", "data": {"extension_type": "risk_register", "data": {}}})
        )
        .is_err());
    }

    #[test]
    fn view_round_trips_the_merged_fields() {
        let f = fixtures().into_iter().find(|f| f.tag == "View").unwrap();
        let keys: BTreeSet<&str> = fields(&f.data).keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            BTreeSet::from([
                "name",
                "viewpoint_ref",
                "architecture_description_ref",
                "root_refs",
                "filter",
                "projection_rules",
                "layout_ref",
                "style_ref",
            ])
        );
        let payload = parse(wire("View", &f.data)).unwrap();
        let NodePayload::View(view) = &payload else {
            panic!("not a View")
        };
        assert_eq!(view.layout_ref.as_ref().map(|h| h.as_str()), Some(H4));
        assert_eq!(
            view.projection_rules,
            vec!["include ProcessNode".to_string()]
        );
        let mut with_positions = f.data.clone();
        with_positions["node_positions"] = json!({"a": [0, 0]});
        assert!(parse(wire("View", &with_positions)).is_err());
    }

    #[test]
    fn standards_profile_round_trips_nested_standards() {
        let f = fixtures()
            .into_iter()
            .find(|f| f.tag == "StandardsProfile")
            .unwrap();
        let payload = parse(wire("StandardsProfile", &f.data)).unwrap();
        let NodePayload::StandardsProfile(profile) = &payload else {
            panic!("not a profile")
        };
        assert_eq!(profile.standards[0].role, MappingRole::Taxonomy);
        assert!(profile.standards[0].required);
        for (field, bad) in [
            ("role", json!("lifecycle_alignment")),
            ("required", json!("yes")),
            ("extra", json!(1)),
        ] {
            let mut data = f.data.clone();
            data["standards"][0][field] = bad;
            assert!(
                parse(wire("StandardsProfile", &data)).is_err(),
                "nested {field}"
            );
        }
    }

    // ------------------------------------------------------------------ Node envelope

    fn requirement_node() -> Value {
        json!({
            "id": "req:HR-001",
            "revision": 1,
            "status": "Accepted",
            "payload": {
                "type": "Requirement",
                "data": {
                    "statement": "The system shall let an employee submit a leave request.",
                    "requirement_kind": "functional",
                    "level": "system",
                    "modality": "shall",
                    "title": null,
                    "rationale": null,
                    "priority": null,
                    "source_identifier": "HR-001",
                    "verification_method": null,
                    "owner_refs": null,
                    "stakeholder_refs": null
                }
            },
            "evidence": ["evd:0123456789abcdef"],
            "derivations": ["drv:s1:classify-1"],
            "standards": [{
                "standard_id": "ISO/IEC/IEEE 29148",
                "version": "2018",
                "concept": "requirement",
                "clause_ref": null,
                "mapping_role": "semantic_alignment",
                "mapping_strength": "compatible",
                "validator_rules": []
            }],
            "tags": ["hr", "pilot"],
            "extensions": {"acme:priority": 2, "jira:issue_key": "HR-42"},
            "audit": {
                "created_by": "actor:analyst",
                "created_at": T1,
                "updated_by": "actor:reviewer",
                "updated_at": T2
            }
        })
    }

    fn parse_node(value: Value) -> Result<Node, serde_json::Error> {
        serde_json::from_value(value)
    }

    #[test]
    fn node_with_a_requirement_payload_round_trips() {
        let node = parse_node(requirement_node()).unwrap();
        assert_eq!(node.payload.node_type(), NodeType::Requirement);
        assert_eq!(serde_json::to_value(&node).unwrap(), requirement_node());
        assert_eq!(node.validate(), Ok(()));
    }

    #[test]
    fn node_revision_one_is_accepted_and_zero_rejected() {
        assert_eq!(parse_node(requirement_node()).unwrap().revision, 1);
        let mut zero = requirement_node();
        zero["revision"] = json!(0);
        assert!(parse_node(zero).is_err());
        let mut node = parse_node(requirement_node()).unwrap();
        node.revision = 0;
        assert_eq!(node.validate(), Err(NodeError::ZeroRevision));
    }

    #[test]
    fn node_rejects_unknown_fields_including_confidence() {
        for (field, value) in [
            ("confidence", json!(0.9)),
            ("kind", json!("Requirement")),
            ("props", json!({})),
        ] {
            let mut node = requirement_node();
            node[field] = value;
            assert!(parse_node(node).is_err(), "Node accepted {field}");
        }
        let keys: BTreeSet<String> = serde_json::to_value(parse_node(requirement_node()).unwrap())
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(
            keys,
            [
                "audit",
                "derivations",
                "evidence",
                "extensions",
                "id",
                "payload",
                "revision",
                "standards",
                "status",
                "tags"
            ]
            .into_iter()
            .map(String::from)
            .collect()
        );
    }

    #[test]
    fn node_extension_map_round_trips_and_rejects_unqualified_keys() {
        let node = parse_node(requirement_node()).unwrap();
        let expected: BTreeMap<ExtensionKey, Value> = BTreeMap::from([
            ("acme:priority".parse().unwrap(), json!(2)),
            ("jira:issue_key".parse().unwrap(), json!("HR-42")),
        ]);
        assert_eq!(node.extensions, expected);
        let mut bad = requirement_node();
        bad["extensions"] = json!({"priority": 2});
        assert!(parse_node(bad).is_err());
    }

    #[test]
    fn node_keeps_typed_evidence_and_derivation_refs() {
        let node = parse_node(requirement_node()).unwrap();
        let evidence: EvidenceRef = Id::try_from("evd:0123456789abcdef".to_string())
            .unwrap()
            .into();
        let derivation: DerivationRef = Id::try_from("drv:s1:classify-1".to_string())
            .unwrap()
            .into();
        assert_eq!(node.evidence, vec![evidence]);
        assert_eq!(node.derivations, vec![derivation]);
        let mut bad = requirement_node();
        bad["evidence"] = json!(["Not An Id"]);
        assert!(parse_node(bad).is_err());
    }

    #[test]
    fn node_enforces_audit_meta_validation() {
        let mut backwards = requirement_node();
        backwards["audit"]["updated_at"] = json!("2026-01-01T00:00:00.000000000Z");
        assert!(parse_node(backwards).is_err());
        let mut node = parse_node(requirement_node()).unwrap();
        node.audit.updated_at = None;
        assert_eq!(
            node.validate(),
            Err(NodeError::InvalidAudit(
                AuditMetaError::UpdatedByWithoutUpdatedAt
            ))
        );
    }

    #[test]
    fn node_tags_are_preserved_exactly() {
        let mut value = requirement_node();
        value["tags"] = json!(["Pilot ", "hr", "ÄÖ"]);
        let node = parse_node(value.clone()).unwrap();
        assert!(node.tags.contains("Pilot "));
        assert_eq!(
            serde_json::to_value(&node).unwrap()["tags"],
            json!(["Pilot ", "hr", "ÄÖ"])
        );
    }

    fn node_with_payload(id: &str, tag: &str) -> Value {
        let mut node = requirement_node();
        node["id"] = json!(id);
        node["payload"] = wire(
            tag,
            &fixtures().into_iter().find(|f| f.tag == tag).unwrap().data,
        );
        node
    }

    #[test]
    fn derivation_record_node_id_must_match_payload_id() {
        assert!(parse_node(node_with_payload("drv:s0:segment-1", "DerivationRecord")).is_ok());
        assert!(parse_node(node_with_payload("drv:s0:other", "DerivationRecord")).is_err());
        let mut node =
            parse_node(node_with_payload("drv:s0:segment-1", "DerivationRecord")).unwrap();
        node.id = "drv:s0:other".parse().unwrap();
        assert!(matches!(
            node.validate(),
            Err(NodeError::PayloadIdMismatch {
                node_type: NodeType::DerivationRecord,
                ..
            })
        ));
    }

    #[test]
    fn standards_profile_node_id_must_match_payload_id() {
        assert!(parse_node(node_with_payload(
            "profile:plumb-software-2026.1",
            "StandardsProfile"
        ))
        .is_ok());
        assert!(parse_node(node_with_payload("profile:other", "StandardsProfile")).is_err());
        let mut node = parse_node(node_with_payload(
            "profile:plumb-software-2026.1",
            "StandardsProfile",
        ))
        .unwrap();
        node.id = "profile:other".parse().unwrap();
        assert!(matches!(
            node.validate(),
            Err(NodeError::PayloadIdMismatch {
                node_type: NodeType::StandardsProfile,
                ..
            })
        ));
    }

    #[test]
    fn other_payloads_do_not_constrain_the_node_id() {
        assert!(parse_node(node_with_payload("anything:goes", "Agent")).is_ok());
    }
}
