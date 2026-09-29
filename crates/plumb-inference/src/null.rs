//! The disabled provider.

use crate::model::{InferenceRequest, ProviderExecution};
use crate::provider::{InferenceProvider, ProviderError};

/// Always returns [`ProviderError::ProviderDisabled`], for every request and policy, without I/O.
#[derive(Debug, Clone, Copy, Default)]
pub struct NullProvider;

impl InferenceProvider for NullProvider {
    fn execute(&self, _request: &InferenceRequest) -> Result<ProviderExecution, ProviderError> {
        Err(ProviderError::ProviderDisabled)
    }
}
