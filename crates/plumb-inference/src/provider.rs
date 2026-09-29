//! The inference provider boundary (compiler architecture §§19-20).
//!
//! A provider receives only the request. It has no graph, revision, store, branch, commit or
//! artifact-store capability: it cannot mutate PSG or persist artifacts.

use plumb_core::Hash;
use thiserror::Error;

use crate::model::{InferenceError, InferenceRequest, ProviderExecution};

/// Executes one inference request and returns the execution for acquisition to persist.
pub trait InferenceProvider {
    fn execute(&self, request: &InferenceRequest) -> Result<ProviderExecution, ProviderError>;
}

/// Why a provider could not produce an execution.
#[derive(Debug, Error)]
pub enum ProviderError {
    /// Inference is disabled (the null provider).
    #[error("inference provider is disabled")]
    ProviderDisabled,
    /// The request itself is invalid.
    #[error("invalid inference request: {0}")]
    InvalidRequest(InferenceError),
    /// No replay fixture is registered for this request ID.
    #[error("no fixture registered for request {0}")]
    FixtureNotFound(Hash),
    /// A fixture key is not a generic `sha256:` hash.
    #[error("fixture key {0} is not a generic sha256: hash")]
    FixtureKeyNotGeneric(Hash),
    /// A fixture's artifact belongs to a different request than its key.
    #[error("fixture key {key} does not match artifact request_hash {request_hash}")]
    FixtureRequestMismatch { key: Hash, request_hash: Hash },
    /// A fixture is already registered for this request ID.
    #[error("a fixture is already registered for request {0}")]
    DuplicateFixture(Hash),
    /// A fixture violates the execution or request invariants.
    #[error("invalid fixture: {0}")]
    InvalidFixture(InferenceError),
}
