//! The disabled provider.

use crate::model::{InferenceRequest, ProviderExecution};
use crate::provider::{
    validate_binding, InferenceProvider, InferenceRequestMaterial, ProviderError,
};

/// Validates the request/material binding, then always returns
/// [`ProviderError::ProviderDisabled`] for every request and policy, without I/O.
#[derive(Debug, Clone, Copy, Default)]
pub struct NullProvider;

impl InferenceProvider for NullProvider {
    fn execute(
        &self,
        request: &InferenceRequest,
        material: &InferenceRequestMaterial,
    ) -> Result<ProviderExecution, ProviderError> {
        validate_binding(request, material)?;
        Err(ProviderError::ProviderDisabled)
    }
}
