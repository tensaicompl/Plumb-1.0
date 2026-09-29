//! Deterministic replay of explicitly registered executions.

use std::collections::BTreeMap;

use plumb_core::{Hash, HashKind};

use crate::model::{InferenceRequest, ProviderExecution};
use crate::provider::{InferenceProvider, ProviderError};

/// Replays exactly the executions registered for exact request IDs.
///
/// It never synthesizes output, never rewrites provider, model or output, and performs no I/O.
/// The request's policy may name any provider: a replay keeps the recorded provider and model.
#[derive(Debug, Clone, Default)]
pub struct MockProvider {
    fixtures: BTreeMap<Hash, ProviderExecution>,
}

impl MockProvider {
    pub fn new() -> MockProvider {
        MockProvider::default()
    }

    /// Builds a provider from `(request ID, execution)` fixtures.
    pub fn with_fixtures(
        fixtures: impl IntoIterator<Item = (Hash, ProviderExecution)>,
    ) -> Result<MockProvider, ProviderError> {
        let mut provider = MockProvider::new();
        for (key, execution) in fixtures {
            provider.register(key, execution)?;
        }
        Ok(provider)
    }

    /// Registers `execution` for request ID `key` after validating both.
    pub fn register(
        &mut self,
        key: Hash,
        execution: ProviderExecution,
    ) -> Result<(), ProviderError> {
        if key.kind() != HashKind::Generic {
            return Err(ProviderError::FixtureKeyNotGeneric(key));
        }
        execution
            .validate()
            .map_err(ProviderError::InvalidFixture)?;
        if execution.artifact.request_hash != key {
            return Err(ProviderError::FixtureRequestMismatch {
                key,
                request_hash: execution.artifact.request_hash,
            });
        }
        if self.fixtures.contains_key(&key) {
            return Err(ProviderError::DuplicateFixture(key));
        }
        self.fixtures.insert(key, execution);
        Ok(())
    }
}

impl InferenceProvider for MockProvider {
    fn execute(&self, request: &InferenceRequest) -> Result<ProviderExecution, ProviderError> {
        request.validate().map_err(ProviderError::InvalidRequest)?;
        let execution = self
            .fixtures
            .get(&request.id)
            .ok_or_else(|| ProviderError::FixtureNotFound(request.id.clone()))?;
        execution
            .validate_for(request)
            .map_err(ProviderError::InvalidFixture)?;
        Ok(execution.clone())
    }
}
