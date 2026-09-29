//! Deterministic compile-run records and their artifact persistence (compiler architecture
//! §§4.2, 24).

use plumb_artifacts::{ArtifactKind, ArtifactStore};
use plumb_core::{to_canonical_json, Hash, HashKind, Id, StageId, Timestamp};
use plumb_store::RevisionId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::context::{require_kind, require_text, CompileContext};
use crate::stage::{ArtifactSet, CompilerError, StageEvaluation, JSON_MEDIA_TYPE};

/// The deterministic record of one stage evaluation. It contains no timestamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "CompileRunFields")]
pub struct CompileRun {
    pub id: Hash,
    pub stage: StageId,

    pub input_revision: RevisionId,
    pub input_semantic_hash: Hash,

    pub profile_ref: Id,
    pub profile_hash: Hash,
    pub rule_pack_hash: Hash,

    pub compiler_version: String,
    pub config_hash: Hash,

    pub deterministic_artifacts: Vec<Hash>,
    pub inference_artifacts: Vec<Hash>,
    pub validation_artifacts: Vec<Hash>,

    pub output_derivation_patch_ref: Option<Hash>,
    pub output_proposal_refs: Vec<Hash>,

    pub output_hash: Hash,
}

/// The output artifact refs of `evaluation`: derivation patch ref and sorted proposal refs.
fn output_refs(evaluation: &StageEvaluation) -> Result<(Option<Hash>, Vec<Hash>), CompilerError> {
    let patch_ref = evaluation
        .derivation_patch_set
        .as_ref()
        .map(|p| to_canonical_json(p).map(|b| Hash::content_sha256(&b)))
        .transpose()?;
    let mut proposal_refs = evaluation
        .proposals
        .iter()
        .map(|p| to_canonical_json(p).map(|b| Hash::content_sha256(&b)))
        .collect::<Result<Vec<_>, _>>()?;
    proposal_refs.sort();
    Ok((patch_ref, proposal_refs))
}

impl CompileRun {
    /// Builds the run of `evaluation` for `stage` from its context and exact input artifacts.
    /// Reads no artifact store and no clock.
    pub fn new(
        stage: StageId,
        ctx: &CompileContext,
        artifacts: &ArtifactSet,
        evaluation: &StageEvaluation,
    ) -> Result<CompileRun, CompilerError> {
        ctx.validate()?;
        artifacts.validate()?;
        evaluation.validate(stage, &ctx.input_semantic_hash)?;
        let hashes = |list: &[crate::stage::ArtifactInput]| -> Vec<Hash> {
            list.iter().map(|a| a.hash.clone()).collect()
        };
        let (output_derivation_patch_ref, output_proposal_refs) = output_refs(evaluation)?;
        let mut run = CompileRun {
            // Placeholder replaced below; identity never includes `id`.
            id: Hash::content_sha256(b""),
            stage,
            input_revision: ctx.input_revision.clone(),
            input_semantic_hash: ctx.input_semantic_hash.clone(),
            profile_ref: ctx.profile_ref.clone(),
            profile_hash: ctx.profile_hash.clone(),
            rule_pack_hash: ctx.rule_pack_hash.clone(),
            compiler_version: ctx.compiler_version.clone(),
            config_hash: ctx.config_hash()?,
            deterministic_artifacts: hashes(&artifacts.deterministic_artifacts),
            inference_artifacts: hashes(&artifacts.inference_artifacts),
            validation_artifacts: hashes(&artifacts.validation_artifacts),
            output_derivation_patch_ref,
            output_proposal_refs,
            output_hash: evaluation.output_hash()?,
        };
        run.id = run.recompute_id()?;
        run.validate()?;
        Ok(run)
    }

    /// The complete run body excluding only `id`.
    pub fn identity_projection(&self) -> Result<Value, CompilerError> {
        let mut body = serde_json::to_value(self)
            .map_err(|e| CompilerError::InvalidCompileRun(e.to_string()))?;
        if let Value::Object(map) = &mut body {
            map.remove("id");
        }
        Ok(body)
    }

    /// Generic SHA-256 of the canonical identity projection.
    pub fn recompute_id(&self) -> Result<Hash, CompilerError> {
        Ok(Hash::content_sha256(&to_canonical_json(
            &self.identity_projection()?,
        )?))
    }

    pub fn validate(&self) -> Result<(), CompilerError> {
        let invalid = CompilerError::InvalidCompileRun;
        require_kind("id", &self.id, HashKind::Generic).map_err(invalid)?;
        require_kind(
            "input_semantic_hash",
            &self.input_semantic_hash,
            HashKind::Semantic,
        )
        .map_err(invalid)?;
        for (what, hash) in [
            ("profile_hash", &self.profile_hash),
            ("rule_pack_hash", &self.rule_pack_hash),
            ("config_hash", &self.config_hash),
            ("output_hash", &self.output_hash),
        ] {
            require_kind(what, hash, HashKind::Generic).map_err(invalid)?;
        }
        if let Some(r) = &self.output_derivation_patch_ref {
            require_kind("output_derivation_patch_ref", r, HashKind::Generic).map_err(invalid)?;
        }
        require_text("compiler_version", &self.compiler_version).map_err(invalid)?;
        for (what, refs) in [
            ("deterministic_artifacts", &self.deterministic_artifacts),
            ("inference_artifacts", &self.inference_artifacts),
            ("validation_artifacts", &self.validation_artifacts),
            ("output_proposal_refs", &self.output_proposal_refs),
        ] {
            for r in refs {
                require_kind(what, r, HashKind::Generic).map_err(invalid)?;
            }
            if refs.windows(2).any(|pair| pair[0] >= pair[1]) {
                return Err(invalid(format!("{what} is not sorted and unique")));
            }
        }
        let recomputed = self.recompute_id()?;
        if recomputed != self.id {
            return Err(invalid(format!(
                "id {} does not match recomputed id {recomputed}",
                self.id
            )));
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompileRunFields {
    id: Hash,
    stage: StageId,
    input_revision: RevisionId,
    input_semantic_hash: Hash,
    profile_ref: Id,
    profile_hash: Hash,
    rule_pack_hash: Hash,
    compiler_version: String,
    config_hash: Hash,
    deterministic_artifacts: Vec<Hash>,
    inference_artifacts: Vec<Hash>,
    validation_artifacts: Vec<Hash>,
    output_derivation_patch_ref: Option<Hash>,
    output_proposal_refs: Vec<Hash>,
    output_hash: Hash,
}

impl TryFrom<CompileRunFields> for CompileRun {
    type Error = CompilerError;

    fn try_from(f: CompileRunFields) -> Result<Self, Self::Error> {
        let run = CompileRun {
            id: f.id,
            stage: f.stage,
            input_revision: f.input_revision,
            input_semantic_hash: f.input_semantic_hash,
            profile_ref: f.profile_ref,
            profile_hash: f.profile_hash,
            rule_pack_hash: f.rule_pack_hash,
            compiler_version: f.compiler_version,
            config_hash: f.config_hash,
            deterministic_artifacts: f.deterministic_artifacts,
            inference_artifacts: f.inference_artifacts,
            validation_artifacts: f.validation_artifacts,
            output_derivation_patch_ref: f.output_derivation_patch_ref,
            output_proposal_refs: f.output_proposal_refs,
            output_hash: f.output_hash,
        };
        run.validate()?;
        Ok(run)
    }
}

/// A persisted run and the artifact hash of its complete canonical JSON (which, unlike
/// `run.id`, includes `id`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedCompileRun {
    pub run: CompileRun,
    pub run_artifact_ref: Hash,
}

/// Persists the derivation patch set, every proposal and the run as canonical JSON artifacts.
///
/// Everything is validated before the first write. This is never part of the graph-commit
/// transaction: an immutable artifact stored before a later failure may remain, a retry is
/// idempotent and no accepted PSG state changes.
pub fn persist_compile_run<S: ArtifactStore>(
    store: &mut S,
    run: &CompileRun,
    evaluation: &StageEvaluation,
    created_at: Timestamp,
) -> Result<PersistedCompileRun, CompilerError> {
    let invalid = CompilerError::InvalidCompileRun;
    run.validate()?;
    evaluation.validate(run.stage, &run.input_semantic_hash)?;
    if evaluation.output_hash()? != run.output_hash {
        return Err(invalid("output_hash does not match the evaluation".into()));
    }
    let (patch_ref, proposal_refs) = output_refs(evaluation)?;
    if patch_ref != run.output_derivation_patch_ref || proposal_refs != run.output_proposal_refs {
        return Err(invalid("output refs do not match the evaluation".into()));
    }

    if let Some(patch_set) = &evaluation.derivation_patch_set {
        let stored = store.put(
            ArtifactKind::Patch,
            JSON_MEDIA_TYPE,
            &to_canonical_json(patch_set)?,
            created_at,
        )?;
        if Some(&stored) != run.output_derivation_patch_ref.as_ref() {
            return Err(invalid("stored derivation patch hash differs".into()));
        }
    }
    for proposal in &evaluation.proposals {
        let stored = store.put(
            ArtifactKind::Proposal,
            JSON_MEDIA_TYPE,
            &to_canonical_json(proposal)?,
            created_at,
        )?;
        if run.output_proposal_refs.binary_search(&stored).is_err() {
            return Err(invalid(format!(
                "stored proposal hash {stored} is not a run ref"
            )));
        }
    }
    let run_artifact_ref = store.put(
        ArtifactKind::CompileRun,
        JSON_MEDIA_TYPE,
        &to_canonical_json(run)?,
        created_at,
    )?;
    Ok(PersistedCompileRun {
        run: run.clone(),
        run_artifact_ref,
    })
}
