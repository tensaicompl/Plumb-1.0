//! The compile scope and the deterministic compile context (compiler architecture §28).

use plumb_core::{CanonicalJson, CoreError, Hash, HashKind, Id};
use plumb_psg::Graph;
use plumb_store::{LoadedRevision, RevisionId};
use serde::{Deserialize, Serialize};

use crate::stage::CompilerError;

/// Which part of the graph a stage run covers. Never inferred by name matching.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
#[serde(try_from = "ScopeWire")]
pub enum Scope {
    Project,
    Elements { refs: Vec<Id> },
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
enum ScopeWire {
    // A struct variant so that extra fields such as `refs` are rejected.
    Project {},
    Elements { refs: Vec<Id> },
}

impl TryFrom<ScopeWire> for Scope {
    type Error = CompilerError;

    fn try_from(wire: ScopeWire) -> Result<Self, Self::Error> {
        let scope = match wire {
            ScopeWire::Project {} => Scope::Project,
            ScopeWire::Elements { refs } => Scope::Elements { refs },
        };
        scope.validate()?;
        Ok(scope)
    }
}

impl Scope {
    /// An element scope with `refs` sorted; duplicates and an empty list are rejected.
    pub fn elements(mut refs: Vec<Id>) -> Result<Scope, CompilerError> {
        refs.sort();
        let scope = Scope::Elements { refs };
        scope.validate()?;
        Ok(scope)
    }

    /// Element refs are non-empty, sorted and unique.
    pub fn validate(&self) -> Result<(), CompilerError> {
        let Scope::Elements { refs } = self else {
            return Ok(());
        };
        if refs.is_empty() {
            return Err(CompilerError::InvalidScope("element scope is empty".into()));
        }
        for pair in refs.windows(2) {
            if pair[0] == pair[1] {
                return Err(CompilerError::InvalidScope(format!(
                    "duplicate ref {}",
                    pair[0]
                )));
            }
            if pair[0] > pair[1] {
                return Err(CompilerError::InvalidScope("refs are not sorted".into()));
            }
        }
        Ok(())
    }

    /// Additionally requires every element ref to be an existing node or edge of `graph`.
    pub fn validate_for_graph(&self, graph: &Graph) -> Result<(), CompilerError> {
        self.validate()?;
        if let Scope::Elements { refs } = self {
            if let Some(missing) = refs
                .iter()
                .find(|id| graph.node(id).is_none() && graph.edge(id).is_none())
            {
                return Err(CompilerError::InvalidScope(format!(
                    "ref {missing} is not a node or edge of the graph"
                )));
            }
        }
        Ok(())
    }
}

/// Deterministic stage input metadata. It contains no timestamp and reads no clock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "CompileContextFields")]
pub struct CompileContext {
    pub input_revision: RevisionId,
    pub input_semantic_hash: Hash,

    pub profile_ref: Id,
    pub profile_hash: Hash,
    pub rule_pack_hash: Hash,

    pub compiler_version: String,
    pub config: CanonicalJson,
}

pub(crate) fn require_kind(what: &str, hash: &Hash, kind: HashKind) -> Result<(), String> {
    if hash.kind() == kind {
        Ok(())
    } else {
        Err(format!(
            "{what} must be a {kind:?} hash, got {:?}",
            hash.kind()
        ))
    }
}

/// Non-empty, no leading/trailing whitespace, no control characters.
pub(crate) fn require_text(what: &str, value: &str) -> Result<(), String> {
    if !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control) {
        Ok(())
    } else {
        Err(format!("invalid {what} {value:?}"))
    }
}

impl CompileContext {
    /// Builds a context whose revision, semantic hash, profile and rule-pack metadata come from
    /// `loaded`, so they cannot drift from the graph being compiled.
    pub fn from_loaded_revision(
        loaded: &LoadedRevision,
        compiler_version: String,
        config: CanonicalJson,
    ) -> Result<CompileContext, CompilerError> {
        let revision = &loaded.revision;
        let ctx = CompileContext {
            input_revision: revision.id.clone(),
            input_semantic_hash: revision.semantic_hash.clone(),
            profile_ref: revision.profile_ref.clone(),
            profile_hash: revision.profile_hash.clone(),
            rule_pack_hash: revision.rule_pack_hash.clone(),
            compiler_version,
            config,
        };
        ctx.validate_for_graph(&loaded.graph)?;
        Ok(ctx)
    }

    pub fn validate(&self) -> Result<(), CompilerError> {
        let invalid = CompilerError::InvalidContext;
        require_kind(
            "input_semantic_hash",
            &self.input_semantic_hash,
            HashKind::Semantic,
        )
        .map_err(invalid)?;
        require_kind("profile_hash", &self.profile_hash, HashKind::Generic).map_err(invalid)?;
        require_kind("rule_pack_hash", &self.rule_pack_hash, HashKind::Generic).map_err(invalid)?;
        require_text("compiler_version", &self.compiler_version).map_err(invalid)?;
        if !self.config.as_value().is_object() {
            return Err(invalid("config must be a JSON object".into()));
        }
        Ok(())
    }

    /// Also requires `graph` to be the graph this context describes.
    pub fn validate_for_graph(&self, graph: &Graph) -> Result<(), CompilerError> {
        self.validate()?;
        if graph.semantic_hash()? != self.input_semantic_hash {
            return Err(CompilerError::InvalidContext(
                "graph semantic hash differs from input_semantic_hash".into(),
            ));
        }
        if graph.profile_id() != &self.profile_ref {
            return Err(CompilerError::InvalidContext(
                "graph profile_id differs from profile_ref".into(),
            ));
        }
        Ok(())
    }

    /// Generic canonical content hash of `config`.
    pub fn config_hash(&self) -> Result<Hash, CoreError> {
        self.config.content_hash()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompileContextFields {
    input_revision: RevisionId,
    input_semantic_hash: Hash,
    profile_ref: Id,
    profile_hash: Hash,
    rule_pack_hash: Hash,
    compiler_version: String,
    config: CanonicalJson,
}

impl TryFrom<CompileContextFields> for CompileContext {
    type Error = CompilerError;

    fn try_from(f: CompileContextFields) -> Result<Self, Self::Error> {
        let ctx = CompileContext {
            input_revision: f.input_revision,
            input_semantic_hash: f.input_semantic_hash,
            profile_ref: f.profile_ref,
            profile_hash: f.profile_hash,
            rule_pack_hash: f.rule_pack_hash,
            compiler_version: f.compiler_version,
            config: f.config,
        };
        ctx.validate()?;
        Ok(ctx)
    }
}
