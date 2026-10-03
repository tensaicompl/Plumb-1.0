//! The canonical evidence manifest: the baseline-participating sources and fragments of a
//! graph with their artifact references, and the graph's existing evidence hash (compiler
//! architecture §6, plan S0.3).
//!
//! Building, validating, parsing and wrapping a manifest are pure functions: no store, file,
//! clock or network is touched, and the returned artifact record is persisted elsewhere.

use plumb_artifacts::{Artifact, ArtifactKind};
use plumb_core::{to_canonical_json, CoreError, Hash, HashKind, Id, Timestamp};
use plumb_psg::{is_baseline, EvidenceLocator, ExtensionKey, Graph, Node, NodePayload, NodeType};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::source::{
    FragmentMetadata, SourceArtifactLinks, EXTRACTED_TEXT_MEDIA_TYPE, FRAGMENT_EXTENSION,
    SOURCE_ARTIFACTS_EXTENSION,
};

/// The version of [`EvidenceManifest`].
pub const EVIDENCE_MANIFEST_VERSION: u32 = 1;

/// Why a manifest could not be built, parsed or accepted.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ManifestError {
    #[error(transparent)]
    Core(#[from] CoreError),
    /// The bytes are not the exact manifest schema.
    #[error("manifest JSON is invalid: {reason}")]
    InvalidJson { reason: String },
    /// The bytes are a valid manifest but not its canonical serialization.
    #[error("manifest bytes are not canonical")]
    NonCanonicalBytes,
    /// The graph's evidence cannot be represented by the pilot manifest.
    #[error("evidence {element_ref} cannot be manifested: {reason}")]
    InvalidGraphEvidence { element_ref: Id, reason: String },
    /// The manifest breaks its own invariants or differs from the graph's manifest.
    #[error("invalid manifest: {reason}")]
    InvalidManifest { reason: String },
}

/// One baseline SourceArtifact and its artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceManifestSource {
    pub source_ref: Id,
    pub content_hash: Hash,
    pub original_ref: Hash,
    pub extracted_ref: Hash,
}

/// One baseline EvidenceFragment and the extracted text it locates into.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceManifestFragment {
    pub fragment_ref: Id,
    pub source_ref: Id,
    pub extracted_ref: Hash,
    pub locator: EvidenceLocator,
    pub content_hash: Hash,
}

/// The evidence baseline of a graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceManifest {
    pub version: u32,
    pub evidence_hash: Hash,
    pub sources: Vec<EvidenceManifestSource>,
    pub fragments: Vec<EvidenceManifestFragment>,
}

fn invalid(reason: impl Into<String>) -> ManifestError {
    ManifestError::InvalidManifest {
        reason: reason.into(),
    }
}

fn require_kind(what: &str, hash: &Hash, kind: HashKind) -> Result<(), ManifestError> {
    if hash.kind() == kind {
        Ok(())
    } else {
        Err(invalid(format!("{what} {hash} is not a {kind:?} hash")))
    }
}

/// Requires `ids` to be strictly increasing.
fn require_strictly_sorted<'a>(
    what: &str,
    ids: impl Iterator<Item = &'a Id>,
) -> Result<(), ManifestError> {
    let mut previous: Option<&Id> = None;
    for id in ids {
        if let Some(previous) = previous {
            if previous == id {
                return Err(invalid(format!("duplicate {what} {id}")));
            }
            if previous > id {
                return Err(invalid(format!("{what} entries are not sorted")));
            }
        }
        previous = Some(id);
    }
    Ok(())
}

impl EvidenceManifest {
    /// Structural invariants; nothing is normalized or repaired.
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.version != EVIDENCE_MANIFEST_VERSION {
            return Err(invalid(format!(
                "version {} is not {EVIDENCE_MANIFEST_VERSION}",
                self.version
            )));
        }
        require_kind("evidence_hash", &self.evidence_hash, HashKind::Evidence)?;
        for source in &self.sources {
            require_kind(
                "source content_hash",
                &source.content_hash,
                HashKind::Generic,
            )?;
            require_kind(
                "source original_ref",
                &source.original_ref,
                HashKind::Generic,
            )?;
            require_kind(
                "source extracted_ref",
                &source.extracted_ref,
                HashKind::Generic,
            )?;
            if source.content_hash != source.original_ref {
                return Err(invalid(format!(
                    "source {} content_hash is not its original_ref",
                    source.source_ref
                )));
            }
        }
        for fragment in &self.fragments {
            require_kind(
                "fragment extracted_ref",
                &fragment.extracted_ref,
                HashKind::Generic,
            )?;
            require_kind(
                "fragment content_hash",
                &fragment.content_hash,
                HashKind::Generic,
            )?;
            fragment
                .locator
                .validate()
                .map_err(|e| invalid(format!("fragment {} locator: {e}", fragment.fragment_ref)))?;
        }
        require_strictly_sorted("source", self.sources.iter().map(|s| &s.source_ref))?;
        require_strictly_sorted("fragment", self.fragments.iter().map(|f| &f.fragment_ref))
    }

    /// RFC 8785 canonical JSON of the complete manifest.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ManifestError> {
        Ok(to_canonical_json(self)?)
    }
}

fn graph_evidence(element: &Node, reason: impl Into<String>) -> ManifestError {
    ManifestError::InvalidGraphEvidence {
        element_ref: element.id.clone(),
        reason: reason.into(),
    }
}

fn extension<T: serde::de::DeserializeOwned>(node: &Node, key: &str) -> Result<T, ManifestError> {
    let key: ExtensionKey = key
        .parse()
        .map_err(|e: plumb_psg::InvalidExtensionKey| graph_evidence(node, e.to_string()))?;
    let value = node
        .extensions
        .get(&key)
        .ok_or_else(|| graph_evidence(node, format!("missing {key}")))?;
    serde_json::from_value(value.clone())
        .map_err(|e| graph_evidence(node, format!("malformed {key}: {e}")))
}

fn baseline_nodes(graph: &Graph, node_type: NodeType) -> impl Iterator<Item = &Node> {
    graph
        .node_ids_by_type(node_type)
        .iter()
        .filter_map(|id| graph.node(id))
        .filter(|node| is_baseline(node.status))
}

/// The manifest of exactly the baseline-participating evidence of `graph`.
pub fn build_evidence_manifest(graph: &Graph) -> Result<EvidenceManifest, ManifestError> {
    let mut sources = Vec::new();
    for node in baseline_nodes(graph, NodeType::SourceArtifact) {
        let NodePayload::SourceArtifact(source) = &node.payload else {
            continue;
        };
        let links: SourceArtifactLinks = extension(node, SOURCE_ARTIFACTS_EXTENSION)?;
        sources.push(EvidenceManifestSource {
            source_ref: node.id.clone(),
            content_hash: source.content_hash.clone(),
            original_ref: links.original_ref,
            extracted_ref: links.extracted_ref,
        });
    }
    sources.sort_by(|a, b| a.source_ref.cmp(&b.source_ref));

    let mut fragments = Vec::new();
    for node in baseline_nodes(graph, NodeType::EvidenceFragment) {
        let NodePayload::EvidenceFragment(fragment) = &node.payload else {
            continue;
        };
        let metadata: FragmentMetadata = extension(node, FRAGMENT_EXTENSION)?;
        let Some(source) = sources
            .binary_search_by(|s| s.source_ref.cmp(&fragment.source_ref))
            .ok()
            .map(|index| &sources[index])
        else {
            return Err(graph_evidence(
                node,
                format!("source {} is not baseline evidence", fragment.source_ref),
            ));
        };
        if metadata.extracted_ref != source.extracted_ref {
            return Err(graph_evidence(
                node,
                "fragment extracted_ref differs from its source's extracted_ref",
            ));
        }
        fragments.push(EvidenceManifestFragment {
            fragment_ref: node.id.clone(),
            source_ref: fragment.source_ref.clone(),
            extracted_ref: metadata.extracted_ref,
            locator: fragment.locator.clone(),
            content_hash: fragment.content_hash.clone(),
        });
    }
    fragments.sort_by(|a, b| a.fragment_ref.cmp(&b.fragment_ref));

    let manifest = EvidenceManifest {
        version: EVIDENCE_MANIFEST_VERSION,
        evidence_hash: graph.evidence_hash()?,
        sources,
        fragments,
    };
    manifest.validate()?;
    Ok(manifest)
}

/// Requires `manifest` to be valid and exactly the manifest of `graph`.
pub fn validate_evidence_manifest(
    manifest: &EvidenceManifest,
    graph: &Graph,
) -> Result<(), ManifestError> {
    manifest.validate()?;
    let expected = build_evidence_manifest(graph)?;
    if manifest == &expected {
        Ok(())
    } else {
        Err(invalid(
            "manifest differs from the graph's evidence manifest",
        ))
    }
}

/// Parses persisted manifest bytes, which must be the canonical serialization of a valid
/// manifest. Caller bytes are never canonicalized.
pub fn parse_evidence_manifest(bytes: &[u8]) -> Result<EvidenceManifest, ManifestError> {
    let manifest: EvidenceManifest =
        serde_json::from_slice(bytes).map_err(|e| ManifestError::InvalidJson {
            reason: e.to_string(),
        })?;
    manifest.validate()?;
    if manifest.canonical_bytes()? != bytes {
        return Err(ManifestError::NonCanonicalBytes);
    }
    Ok(manifest)
}

/// The `evidence-manifest` artifact record of `manifest`; `created_at` is supplied by the
/// caller and never affects bytes or hash.
pub fn evidence_manifest_artifact(
    manifest: &EvidenceManifest,
    created_at: Timestamp,
) -> Result<Artifact, ManifestError> {
    manifest.validate()?;
    let bytes = manifest.canonical_bytes()?;
    Ok(Artifact {
        hash: Hash::content_sha256(&bytes),
        kind: ArtifactKind::EvidenceManifest,
        media_type: EXTRACTED_TEXT_MEDIA_TYPE.to_owned(),
        bytes,
        created_at,
    })
}

#[cfg(test)]
mod manifest_contract {
    use super::*;
    use crate::source::{ImportAudit, ImportedSource};
    use crate::{import_markdown, import_plain_text};
    use serde_json::{json, Value};

    const T1: &str = "2026-10-03T08:00:00.000000000Z";
    const T2: &str = "2031-01-01T00:00:00.000000000Z";

    fn id(s: &str) -> Id {
        s.parse().unwrap()
    }

    fn audit() -> ImportAudit {
        ImportAudit {
            created_by: id("actor:importer"),
            created_at: T1.parse().unwrap(),
        }
    }

    fn imported() -> (ImportedSource, ImportedSource) {
        (
            import_markdown("a.md", b"# Title\n\n- item\n\nText\n", &audit()).unwrap(),
            import_plain_text("b.txt", b"Plain paragraph.\n1. one\n", &audit()).unwrap(),
        )
    }

    fn graph_of(nodes: Vec<Node>) -> Graph {
        Graph::new(
            id("project:leave-management"),
            id("profile:plumb-software-2026.1"),
            nodes,
            vec![],
        )
        .unwrap_or_else(|v| panic!("{v:#?}"))
    }

    fn nodes() -> Vec<Node> {
        let (a, b) = imported();
        let mut nodes = vec![a.source, b.source];
        nodes.extend(a.fragments);
        nodes.extend(b.fragments);
        nodes
    }

    fn graph() -> Graph {
        graph_of(nodes())
    }

    fn manifest() -> EvidenceManifest {
        build_evidence_manifest(&graph()).unwrap()
    }

    fn manifest_value() -> Value {
        serde_json::to_value(manifest()).unwrap()
    }

    fn canonical(value: &Value) -> Vec<u8> {
        to_canonical_json(value).unwrap()
    }

    fn sha(bytes: &[u8]) -> String {
        Hash::content_sha256(bytes).to_string()
    }

    // ------------------------------------------------------------------ positive

    #[test]
    fn manifest_is_built_deterministically_from_the_graph() {
        let graph = graph();
        let manifest = build_evidence_manifest(&graph).unwrap();
        assert_eq!(manifest.version, 1);
        assert_eq!(manifest.evidence_hash, graph.evidence_hash().unwrap());
        assert_eq!(manifest.evidence_hash.kind(), HashKind::Evidence);
        let (a, b) = imported();
        assert_eq!(manifest.sources.len(), 2);
        assert_eq!(
            manifest.fragments.len(),
            a.fragments.len() + b.fragments.len()
        );
        for imported in [&a, &b] {
            let source = manifest
                .sources
                .iter()
                .find(|s| s.source_ref == imported.source.id)
                .unwrap();
            assert_eq!(source.content_hash, imported.original_artifact.hash);
            assert_eq!(source.original_ref, imported.original_artifact.hash);
            assert_eq!(source.extracted_ref, imported.extracted_artifact.hash);
            for node in &imported.fragments {
                let NodePayload::EvidenceFragment(f) = &node.payload else {
                    unreachable!()
                };
                let entry = manifest
                    .fragments
                    .iter()
                    .find(|e| e.fragment_ref == node.id)
                    .unwrap();
                assert_eq!(
                    entry,
                    &EvidenceManifestFragment {
                        fragment_ref: node.id.clone(),
                        source_ref: f.source_ref.clone(),
                        extracted_ref: imported.extracted_artifact.hash.clone(),
                        locator: f.locator.clone(),
                        content_hash: f.content_hash.clone(),
                    }
                );
            }
        }
        assert!(manifest
            .sources
            .windows(2)
            .all(|w| w[0].source_ref < w[1].source_ref));
        assert!(manifest
            .fragments
            .windows(2)
            .all(|w| w[0].fragment_ref < w[1].fragment_ref));
        // Node insertion order does not matter.
        let mut reversed = nodes();
        reversed.reverse();
        assert_eq!(
            build_evidence_manifest(&graph_of(reversed)).unwrap(),
            manifest
        );
        assert_eq!(validate_evidence_manifest(&manifest, &graph), Ok(()));
    }

    #[test]
    fn manifest_canonical_bytes_artifact_and_parse_round_trip() {
        let manifest = manifest();
        let bytes = manifest.canonical_bytes().unwrap();
        assert_eq!(bytes, canonical(&manifest_value()));
        let artifact = evidence_manifest_artifact(&manifest, T1.parse().unwrap()).unwrap();
        assert_eq!(artifact.kind, ArtifactKind::EvidenceManifest);
        assert_eq!(artifact.media_type, "application/json");
        assert_eq!(artifact.bytes, bytes);
        assert_eq!(artifact.hash.as_str(), sha(&bytes));
        assert_eq!(artifact.hash.kind(), HashKind::Generic);
        assert_ne!(artifact.hash, manifest.evidence_hash);
        let later = evidence_manifest_artifact(&manifest, T2.parse().unwrap()).unwrap();
        assert_eq!(
            (later.bytes.clone(), later.hash.clone()),
            (artifact.bytes.clone(), artifact.hash.clone())
        );
        assert_ne!(later.created_at, artifact.created_at);
        assert_eq!(parse_evidence_manifest(&bytes).unwrap(), manifest);
    }

    #[test]
    fn empty_graph_has_an_exact_empty_manifest() {
        let graph = graph_of(vec![]);
        let manifest = build_evidence_manifest(&graph).unwrap();
        assert!(manifest.sources.is_empty() && manifest.fragments.is_empty());
        assert_eq!(manifest.evidence_hash, graph.evidence_hash().unwrap());
        assert_eq!(validate_evidence_manifest(&manifest, &graph), Ok(()));
        let bytes = manifest.canonical_bytes().unwrap();
        assert_eq!(parse_evidence_manifest(&bytes).unwrap(), manifest);
    }

    // ------------------------------------------------------------------ negative: structure

    fn structural_error(mutate: impl Fn(&mut Value)) -> ManifestError {
        let mut value = manifest_value();
        mutate(&mut value);
        let bytes = canonical(&value);
        parse_evidence_manifest(&bytes).unwrap_err()
    }

    type Mutation = Box<dyn Fn(&mut Value)>;

    #[test]
    fn manifest_structural_invariants_are_enforced() {
        let generic = format!("sha256:{}", "a".repeat(64));
        let evidence = format!("ev:sha256:{}", "a".repeat(64));
        let cases: Vec<Mutation> = vec![
            Box::new(|v| v["version"] = json!(2)),
            Box::new({
                let g = generic.clone();
                move |v| v["evidence_hash"] = json!(g)
            }),
            Box::new({
                let e = evidence.clone();
                move |v| v["sources"][0]["content_hash"] = json!(e)
            }),
            Box::new({
                let e = evidence.clone();
                move |v| v["sources"][0]["original_ref"] = json!(e)
            }),
            Box::new({
                let e = evidence.clone();
                move |v| v["sources"][0]["extracted_ref"] = json!(e)
            }),
            Box::new({
                let g = generic.clone();
                move |v| v["sources"][0]["content_hash"] = json!(g)
            }),
            Box::new({
                let e = evidence.clone();
                move |v| v["fragments"][0]["content_hash"] = json!(e)
            }),
            Box::new({
                let e = evidence.clone();
                move |v| v["fragments"][0]["extracted_ref"] = json!(e)
            }),
            Box::new(|v| v["sources"].as_array_mut().unwrap().reverse()),
            Box::new(|v| {
                let first = v["sources"][0].clone();
                v["sources"].as_array_mut().unwrap().insert(0, first);
            }),
            Box::new(|v| v["fragments"].as_array_mut().unwrap().reverse()),
            Box::new(|v| {
                let first = v["fragments"][0].clone();
                v["fragments"].as_array_mut().unwrap().insert(0, first);
            }),
        ];
        for (index, case) in cases.iter().enumerate() {
            assert!(
                matches!(
                    structural_error(case),
                    ManifestError::InvalidManifest { .. }
                ),
                "case {index}"
            );
        }
        // Validation never repairs: an unsorted manifest stays invalid.
        let mut unsorted = manifest();
        unsorted.fragments.reverse();
        assert!(matches!(
            unsorted.validate(),
            Err(ManifestError::InvalidManifest { .. })
        ));
        assert!(matches!(
            validate_evidence_manifest(&unsorted, &graph()),
            Err(ManifestError::InvalidManifest { .. })
        ));
    }

    #[test]
    fn manifest_json_must_be_exact_and_canonical() {
        assert!(matches!(
            structural_error(|v| v["extra"] = json!(1)),
            ManifestError::InvalidJson { .. }
        ));
        assert!(matches!(
            structural_error(|v| v["sources"][0]["filename"] = json!("a.md")),
            ManifestError::InvalidJson { .. }
        ));
        assert!(matches!(
            parse_evidence_manifest(b"{not json"),
            Err(ManifestError::InvalidJson { .. })
        ));
        let pretty = serde_json::to_vec_pretty(&manifest_value()).unwrap();
        assert_eq!(
            parse_evidence_manifest(&pretty),
            Err(ManifestError::NonCanonicalBytes)
        );
    }

    // ------------------------------------------------------------------ negative: graph evidence

    fn with_extension(index: usize, key: &str, value: Option<Value>) -> Graph {
        let mut nodes = nodes();
        let key: ExtensionKey = key.parse().unwrap();
        match value {
            Some(value) => {
                nodes[index].extensions.insert(key, value);
            }
            None => {
                nodes[index].extensions.remove(&key);
            }
        }
        graph_of(nodes)
    }

    fn graph_evidence_error(graph: &Graph) -> Id {
        match build_evidence_manifest(graph) {
            Err(ManifestError::InvalidGraphEvidence { element_ref, .. }) => element_ref,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn graph_evidence_without_pilot_links_cannot_be_manifested() {
        let nodes = nodes();
        let source = nodes[0].id.clone();
        let fragment = nodes[2].id.clone();
        assert_eq!(
            graph_evidence_error(&with_extension(0, SOURCE_ARTIFACTS_EXTENSION, None)),
            source
        );
        assert_eq!(
            graph_evidence_error(&with_extension(
                0,
                SOURCE_ARTIFACTS_EXTENSION,
                Some(json!({"original_ref": 1}))
            )),
            source
        );
        assert_eq!(
            graph_evidence_error(&with_extension(2, FRAGMENT_EXTENSION, None)),
            fragment
        );
        assert_eq!(
            graph_evidence_error(&with_extension(
                2,
                FRAGMENT_EXTENSION,
                Some(json!({"kind": "heading"}))
            )),
            fragment
        );
        let other = format!("sha256:{}", "b".repeat(64));
        let disagreeing = with_extension(
            2,
            FRAGMENT_EXTENSION,
            Some(json!({"kind": "heading", "extracted_ref": other})),
        );
        assert_eq!(graph_evidence_error(&disagreeing), fragment);
    }

    #[test]
    fn manifest_must_equal_the_graph_manifest() {
        let graph = graph();
        let manifest = manifest();
        let differs = |m: EvidenceManifest| {
            matches!(
                validate_evidence_manifest(&m, &graph),
                Err(ManifestError::InvalidManifest { .. })
            )
        };
        let mut m = manifest.clone();
        m.evidence_hash = Hash::evidence_sha256(b"other");
        assert!(differs(m));
        let mut m = manifest.clone();
        m.sources.pop();
        assert!(differs(m));
        let mut m = manifest.clone();
        let mut extra = m.sources[1].clone();
        extra.source_ref = id("src:ffffffffffffffff");
        m.sources.push(extra);
        assert!(differs(m));
        let mut m = manifest.clone();
        m.sources[0].extracted_ref = Hash::content_sha256(b"other");
        assert!(differs(m));
        let mut m = manifest.clone();
        m.sources[0].content_hash = Hash::content_sha256(b"other");
        m.sources[0].original_ref = Hash::content_sha256(b"other");
        assert!(differs(m));
        let mut m = manifest.clone();
        m.fragments.pop();
        assert!(differs(m));
        let mut m = manifest.clone();
        let mut extra = m.fragments.last().unwrap().clone();
        extra.fragment_ref = id("evd:ffffffffffffffff");
        m.fragments.push(extra);
        assert!(differs(m));
        for change in 0..4 {
            let mut m = manifest.clone();
            let f = &mut m.fragments[0];
            match change {
                0 => f.source_ref = m.sources[1].source_ref.clone(),
                1 => f.extracted_ref = Hash::content_sha256(b"other"),
                2 => f.locator = EvidenceLocator::TextRange { start: 0, end: 1 },
                _ => f.content_hash = Hash::content_sha256(b"other"),
            }
            assert!(differs(m), "change {change}");
        }
        // A fresh manifest after the graph changes is stale.
        let mut more = nodes();
        more.truncate(3);
        assert!(differs(build_evidence_manifest(&graph_of(more)).unwrap()));
    }
}
