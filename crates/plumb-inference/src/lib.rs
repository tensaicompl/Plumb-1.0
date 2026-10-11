//! Inference request/artifact contracts, the provider boundary (null and mock providers),
//! acquisition of inference artifacts and deterministic provenance materialization
//! (compiler architecture §§4.3-4.4, 19-20; metamodel §§5.3-5.4).
//!
//! X1.1 adds hash-bound in-memory request material for live providers and, behind the
//! `anthropic` feature, the Anthropic Messages API provider. Hotfix 066 binds that material to
//! every provider call: `execute(request, material)` validates the pair before any work.
//!
//! Providers never receive graph, store, commit or artifact-store access. `Agent` and
//! `DerivationRecord` are the PSG types, re-exported here; no duplicates are defined.

#[cfg(feature = "anthropic")]
pub mod anthropic;
pub mod mock;
pub mod model;
pub mod null;
pub mod provider;

pub use mock::MockProvider;
pub use model::{
    materialize_derivation_record, persist_inference_bundle, InferenceArtifact, InferenceError,
    InferenceRequest, PersistedInferenceRefs, ProviderExecution, ProviderPolicy, JSON_MEDIA_TYPE,
};
pub use null::NullProvider;
pub use plumb_core::{CanonicalJson, StageId};
pub use plumb_psg::{Agent, AgentKind, DerivationKind, DerivationRecord};
pub use provider::{
    validate_binding, InferenceProvider, InferenceRequestMaterial, InferenceRequestMaterialSet,
    ProviderError,
};

#[cfg(feature = "anthropic")]
pub use anthropic::AnthropicProvider;
