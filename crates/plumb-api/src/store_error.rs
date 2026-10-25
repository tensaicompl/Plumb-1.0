//! Typed mapping of store failures (architecture §33.9, H0.9): branch not found is 404
//! `E_BRANCH_NOT_FOUND`, an uninitialized database 409 `E_DATABASE_NOT_INITIALIZED`, SQLite busy
//! or locked 503 `E_DATABASE_BUSY`, a write on a read-only database 409 `E_DATABASE_READ_ONLY`
//! and detected corruption (including an artifact whose bytes no longer hash to its key, H0.10)
//! 500 `E_STORE_CORRUPT`. Data written under an unsupported store or PSG schema version is 409
//! `E_STORE_VERSION_UNSUPPORTED` (Fable #3; never corruption). Every other failure keeps its
//! operation-specific mapping. SQLite messages and paths never form the contract.

use axum::http::StatusCode;
use plumb_artifacts::{Artifact, ArtifactStoreError};
use plumb_core::Hash;
use plumb_store::{artifact_error_class, StoreError, StoreErrorClass};
use serde_json::json;

use crate::error::ApiError;
use crate::state::ApiState;

impl ApiError {
    /// The typed error of a store failure `class`, or `None` when it has no typed mapping.
    pub fn store_class(class: StoreErrorClass) -> Option<ApiError> {
        let (status, code, message) = match class {
            StoreErrorClass::BranchNotFound => (
                StatusCode::NOT_FOUND,
                "E_BRANCH_NOT_FOUND",
                "the branch does not exist in this database",
            ),
            StoreErrorClass::NotInitialized => (
                StatusCode::CONFLICT,
                "E_DATABASE_NOT_INITIALIZED",
                "the database is not an initialized PLUMB project",
            ),
            StoreErrorClass::Busy => (
                StatusCode::SERVICE_UNAVAILABLE,
                "E_DATABASE_BUSY",
                "the database is busy; retry later",
            ),
            StoreErrorClass::ReadOnly => (
                StatusCode::CONFLICT,
                "E_DATABASE_READ_ONLY",
                "the database is read-only",
            ),
            StoreErrorClass::Corrupt => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "E_STORE_CORRUPT",
                "persisted store data is corrupt",
            ),
            StoreErrorClass::UnsupportedVersion => (
                StatusCode::CONFLICT,
                "E_STORE_VERSION_UNSUPPORTED",
                "the database was written under an unsupported schema version",
            ),
            StoreErrorClass::Other => return None,
        };
        Some(ApiError::with_status(status, code, message))
    }

    /// The typed error of `error`, else 500 `E_INTERNAL` naming `subsystem`.
    pub fn store(error: &StoreError, subsystem: &str) -> ApiError {
        ApiError::store_class(error.class())
            .map(|e| e.with_details(json!({ "subsystem": subsystem })))
            .unwrap_or_else(|| ApiError::internal(subsystem))
    }

    /// The typed error of an artifact-store failure, else 500 `E_INTERNAL` naming `subsystem`.
    pub fn artifact_store(error: &ArtifactStoreError, subsystem: &str) -> ApiError {
        ApiError::store_class(artifact_error_class(error))
            .map(|e| e.with_details(json!({ "subsystem": subsystem })))
            .unwrap_or_else(|| ApiError::internal(subsystem))
    }
}

impl ApiState {
    /// Reads the artifact `hash`. Typed store failures (corruption: the stored bytes no longer
    /// hash to the key, H0.10; busy; ...) are the outer error, so a corrupted artifact is never
    /// handled as valid or merely missing; any other failure is returned for the caller to map.
    pub(crate) fn get_artifact(
        &self,
        hash: &Hash,
    ) -> Result<Result<Artifact, ArtifactStoreError>, ApiError> {
        let result =
            self.with_artifact_store(|store| plumb_artifacts::ArtifactStore::get(&*store, hash))?;
        match result {
            Err(error) if !matches!(artifact_error_class(&error), StoreErrorClass::Other) => {
                Err(ApiError::artifact_store(&error, "artifact_store"))
            }
            other => Ok(other),
        }
    }
}
