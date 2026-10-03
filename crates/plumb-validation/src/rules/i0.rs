//! The six I0 evaluators: the evidence corpus is reproducible and addressable (rulebook I0,
//! plan S0.3).
//!
//! Every evaluator reads only the graph and the already-acquired evidence artifacts of the
//! context. Bad evidence is a violation; an evaluator failure is reserved for a genuine
//! internal inability to evaluate.

use plumb_artifacts::ArtifactKind;
use plumb_core::{to_canonical_json, Hash, HashKind, Id};
use plumb_import::{
    build_evidence_manifest, parse_evidence_manifest, source_parse_metadata,
    validate_evidence_manifest, ExtractedTextArtifact, FragmentMetadata, SourceArtifactLinks,
    SourceKind, FRAGMENT_EXTENSION, SOURCE_ARTIFACTS_EXTENSION,
};
use plumb_psg::{
    is_baseline, EvidenceFragment, EvidenceLocator, ExtensionKey, Graph, Node, NodePayload,
    NodeType, SourceArtifact,
};
use serde::de::DeserializeOwned;

use crate::evaluator::{
    EvaluationError, EvaluatorFailure, EvaluatorRegistry, RuleEvaluation, RuleEvaluator,
    ValidationContext,
};
use crate::model::RuleMetadata;

const CONTENT_ADDRESSED: &str = "PLUMB.I0.SOURCE.CONTENT_ADDRESSED";
const LOCATABLE: &str = "PLUMB.I0.EVIDENCE.LOCATABLE";
const HASH_MATCH: &str = "PLUMB.I0.EVIDENCE.HASH_MATCH";
const PARSE_STATUS: &str = "PLUMB.I0.SOURCE.PARSE_STATUS";
const AGENT_IDENTIFIED: &str = "PPMN.I0.PROVENANCE.AGENT_IDENTIFIED";
const BASELINE_HASHABLE: &str = "PLUMB.I0.BASELINE.HASHABLE";

const JSON_MEDIA_TYPE: &str = "application/json";

/// Binds the six I0 evaluators. Rule metadata stays in the registry.
pub fn register_i0_evaluators(registry: &mut EvaluatorRegistry) -> Result<(), EvaluationError> {
    let evaluators: [(&str, RuleEvaluator); 6] = [
        (CONTENT_ADDRESSED, content_addressed),
        (LOCATABLE, locatable),
        (HASH_MATCH, hash_match),
        (PARSE_STATUS, parse_status),
        (AGENT_IDENTIFIED, agent_identified),
        (BASELINE_HASHABLE, baseline_hashable),
    ];
    for (rule_id, evaluator) in evaluators {
        registry.register(rule_id, evaluator)?;
    }
    Ok(())
}

type Outcome = Result<RuleEvaluation, EvaluatorFailure>;

fn internal(code: &str, message: String) -> EvaluatorFailure {
    EvaluatorFailure {
        code: code.to_owned(),
        message,
        targets: Vec::new(),
        evidence: Vec::new(),
    }
}

/// PASS over every checked element, or one violation naming every violating element.
fn universal(checked: Vec<Id>, violating: Vec<Id>, condition_key: &str, message: &str) -> Outcome {
    let evaluation = if violating.is_empty() {
        RuleEvaluation::pass(checked, Vec::new())
    } else {
        RuleEvaluation::violation(
            violating,
            Vec::new(),
            condition_key.to_owned(),
            message.to_owned(),
            None,
        )
    };
    evaluation.map_err(|e| internal("E_I0_OUTPUT", e.to_string()))
}

// ============================================================================ graph access

fn baseline_nodes(graph: &Graph, node_type: NodeType) -> impl Iterator<Item = &Node> {
    graph
        .node_ids_by_type(node_type)
        .iter()
        .filter_map(|id| graph.node(id))
        .filter(|node| is_baseline(node.status))
}

fn baseline_sources(graph: &Graph) -> impl Iterator<Item = (&Node, &SourceArtifact)> {
    baseline_nodes(graph, NodeType::SourceArtifact).filter_map(|node| match &node.payload {
        NodePayload::SourceArtifact(source) => Some((node, source)),
        _ => None,
    })
}

/// The built-in import kind of a source: its source_kind parsed exactly as SourceKind.
fn builtin_source_kind(source: &SourceArtifact) -> Option<SourceKind> {
    source.source_kind.parse().ok()
}

fn extension<T: DeserializeOwned>(node: &Node, key: &str) -> Option<T> {
    let key: ExtensionKey = key.parse().ok()?;
    serde_json::from_value(node.extensions.get(&key)?.clone()).ok()
}

/// The baseline fragments whose resolved baseline source is of a built-in kind.
fn builtin_fragments(graph: &Graph) -> Vec<(&Node, &EvidenceFragment, &Node)> {
    baseline_nodes(graph, NodeType::EvidenceFragment)
        .filter_map(|node| {
            let NodePayload::EvidenceFragment(fragment) = &node.payload else {
                return None;
            };
            let source_node = graph
                .node(&fragment.source_ref)
                .filter(|s| is_baseline(s.status))?;
            match &source_node.payload {
                NodePayload::SourceArtifact(source) if builtin_source_kind(source).is_some() => {
                    Some((node, fragment, source_node))
                }
                _ => None,
            }
        })
        .collect()
}

/// The exact extracted bytes a built-in fragment locates, if it can be resolved.
fn resolve_range(
    ctx: &ValidationContext,
    fragment_node: &Node,
    fragment: &EvidenceFragment,
    source_node: &Node,
) -> Option<Vec<u8>> {
    fragment.locator.validate().ok()?;
    let EvidenceLocator::TextRange { start, end } = fragment.locator else {
        return None;
    };
    let metadata: FragmentMetadata = extension(fragment_node, FRAGMENT_EXTENSION)?;
    let links: SourceArtifactLinks = extension(source_node, SOURCE_ARTIFACTS_EXTENSION)?;
    if metadata.extracted_ref != links.extracted_ref {
        return None;
    }
    let artifact = ctx.evidence_artifact(&metadata.extracted_ref)?;
    if artifact.kind != ArtifactKind::SourceExtracted || artifact.media_type != JSON_MEDIA_TYPE {
        return None;
    }
    let extracted: ExtractedTextArtifact = serde_json::from_slice(&artifact.bytes).ok()?;
    if to_canonical_json(&extracted).ok()? != artifact.bytes {
        return None;
    }
    let (start, end) = (usize::try_from(start).ok()?, usize::try_from(end).ok()?);
    let text = &extracted.text;
    if start >= end
        || end > text.len()
        || !text.is_char_boundary(start)
        || !text.is_char_boundary(end)
    {
        return None;
    }
    Some(text.as_bytes()[start..end].to_vec())
}

// ============================================================================ evaluators

fn content_addressed(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let mut checked = Vec::new();
    let mut violating = Vec::new();
    for (node, source) in baseline_sources(graph) {
        let backed = source.content_hash.kind() == HashKind::Generic
            && ctx
                .evidence_artifact(&source.content_hash)
                .is_some_and(|artifact| {
                    artifact.kind == ArtifactKind::SourceOriginal
                        && artifact.media_type == source.media_type
                });
        let linked = builtin_source_kind(source).is_none()
            || extension::<SourceArtifactLinks>(node, SOURCE_ARTIFACTS_EXTENSION)
                .is_some_and(|links| links.original_ref == source.content_hash);
        if !(backed && linked) {
            violating.push(node.id.clone());
        }
        checked.push(node.id.clone());
    }
    universal(
        checked,
        violating,
        "source-content-addressed",
        "One or more SourceArtifact nodes are not backed by the required content-addressed source artifact.",
    )
}

fn locatable(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let mut checked = Vec::new();
    let mut violating = Vec::new();
    for (node, fragment, source) in builtin_fragments(graph) {
        if resolve_range(ctx, node, fragment, source).is_none() {
            violating.push(node.id.clone());
        }
        checked.push(node.id.clone());
    }
    universal(
        checked,
        violating,
        "evidence-locatable",
        "One or more EvidenceFragment nodes cannot be resolved to their exact extracted source range.",
    )
}

fn hash_match(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let mut checked = Vec::new();
    let mut violating = Vec::new();
    for (node, fragment, source) in builtin_fragments(graph) {
        let matches = resolve_range(ctx, node, fragment, source).is_some_and(|bytes| {
            Hash::content_sha256(&bytes) == fragment.content_hash
                && fragment
                    .extracted_text
                    .as_ref()
                    .is_none_or(|text| text.as_bytes() == bytes.as_slice())
        });
        if !matches {
            violating.push(node.id.clone());
        }
        checked.push(node.id.clone());
    }
    universal(
        checked,
        violating,
        "evidence-hash-match",
        "One or more EvidenceFragment content hashes do not match the bytes resolved from the extracted source.",
    )
}

fn parse_status(graph: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let mut checked = Vec::new();
    let mut violating = Vec::new();
    for (node, source) in baseline_sources(graph) {
        if builtin_source_kind(source).is_none() {
            continue;
        }
        if source_parse_metadata(node).is_err() {
            violating.push(node.id.clone());
        }
        checked.push(node.id.clone());
    }
    universal(
        checked,
        violating,
        "source-parse-status",
        "One or more built-in SourceArtifact nodes lack a valid explicit parse status.",
    )
}

fn agent_identified(graph: &Graph, _: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let mut checked = Vec::new();
    let mut violating = Vec::new();
    for node in baseline_nodes(graph, NodeType::DerivationRecord) {
        let identified = graph.node(&node.audit.created_by).is_some_and(|agent| {
            is_baseline(agent.status) && matches!(agent.payload, NodePayload::Agent(_))
        });
        if !identified {
            violating.push(node.id.clone());
        }
        checked.push(node.id.clone());
    }
    if checked.is_empty() {
        return RuleEvaluation::not_applicable(
            "No derivation-producing semantic action is present in the evaluated baseline."
                .to_owned(),
        )
        .map_err(|e| internal("E_I0_OUTPUT", e.to_string()));
    }
    universal(
        checked,
        violating,
        "provenance-agent-identified",
        "One or more DerivationRecord nodes do not identify a known provenance Agent.",
    )
}

fn baseline_hashable(graph: &Graph, ctx: &ValidationContext, _: &RuleMetadata) -> Outcome {
    let evidence_hash = graph
        .evidence_hash()
        .map_err(|e| internal("E_I0_EVIDENCE_HASH", e.to_string()))?;
    let reproducible = evidence_hash == ctx.baseline_evidence_hash
        && manifest_reproduces(graph, ctx, &evidence_hash);
    let evaluation = if reproducible {
        RuleEvaluation::pass(Vec::new(), Vec::new())
    } else {
        RuleEvaluation::violation(
            Vec::new(),
            Vec::new(),
            "baseline-evidence-hash".to_owned(),
            "The evidence baseline cannot be reproduced from the evaluated graph and evidence manifest."
                .to_owned(),
            None,
        )
    };
    evaluation.map_err(|e| internal("E_I0_OUTPUT", e.to_string()))
}

/// Exactly one canonical evidence-manifest artifact that is the graph's manifest.
fn manifest_reproduces(graph: &Graph, ctx: &ValidationContext, evidence_hash: &Hash) -> bool {
    let manifests: Vec<_> = ctx
        .evidence_artifacts
        .iter()
        .filter(|artifact| artifact.kind == ArtifactKind::EvidenceManifest)
        .collect();
    let [artifact] = manifests.as_slice() else {
        return false;
    };
    if artifact.media_type != JSON_MEDIA_TYPE {
        return false;
    }
    let Ok(manifest) = parse_evidence_manifest(&artifact.bytes) else {
        return false;
    };
    &manifest.evidence_hash == evidence_hash
        && manifest.evidence_hash == ctx.baseline_evidence_hash
        && validate_evidence_manifest(&manifest, graph).is_ok()
        && build_evidence_manifest(graph).is_ok_and(|expected| expected == manifest)
}
