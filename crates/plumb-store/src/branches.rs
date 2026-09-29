//! Branch names and branch-head persistence (compiler architecture §§4.1, 5.8).

use std::fmt;
use std::str::FromStr;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

use crate::revisions::RevisionId;
use crate::StoreError;

const MAX_BRANCH_NAME_LEN: usize = 255;

/// A string that does not satisfy the pilot branch-name grammar.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid branch name {0:?}")]
pub struct InvalidBranchName(pub String);

/// A validated branch name: 1..=255 bytes, an ASCII alphanumeric first character, and otherwise
/// only ASCII letters, digits and `.`, `_`, `:`, `/`, `-`. Serialized as the bare string.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BranchName(String);

impl BranchName {
    /// The branch every store is initialized with.
    pub fn main() -> BranchName {
        BranchName("main".to_owned())
    }

    /// Returns the exact validated name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_valid_branch_name(s: &str) -> bool {
    let mut bytes = s.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    s.len() <= MAX_BRANCH_NAME_LEN
        && first.is_ascii_alphanumeric()
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'/' | b'-'))
}

impl TryFrom<String> for BranchName {
    type Error = InvalidBranchName;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if is_valid_branch_name(&value) {
            Ok(BranchName(value))
        } else {
            Err(InvalidBranchName(value))
        }
    }
}

impl FromStr for BranchName {
    type Err = InvalidBranchName;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.to_owned().try_into()
    }
}

impl fmt::Display for BranchName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for BranchName {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for BranchName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .try_into()
            .map_err(serde::de::Error::custom)
    }
}

/// The current head of `branch`, or `None` when the branch does not exist.
pub(crate) fn read_head(
    conn: &Connection,
    branch: &BranchName,
) -> Result<Option<RevisionId>, StoreError> {
    let stored: Option<String> = conn
        .query_row(
            "SELECT revision_id FROM branch_heads WHERE name = ?1",
            [branch.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    Ok(stored.map(|s| s.parse()).transpose()?)
}

/// Moves `branch` from `expected` to `target`, returning `StaleBase` unless exactly one row moved.
pub(crate) fn cas_head(
    conn: &Connection,
    branch: &BranchName,
    expected: &RevisionId,
    target: &RevisionId,
    updated_at: &str,
) -> Result<(), StoreError> {
    let changed = conn.execute(
        "UPDATE branch_heads SET revision_id = ?1, updated_at = ?2 \
         WHERE name = ?3 AND revision_id = ?4",
        params![
            target.as_str(),
            updated_at,
            branch.as_str(),
            expected.as_str()
        ],
    )?;
    if changed == 1 {
        return Ok(());
    }
    match read_head(conn, branch)? {
        Some(actual) => Err(StoreError::StaleBase {
            branch: branch.clone(),
            expected: expected.clone(),
            actual,
        }),
        None => Err(StoreError::BranchNotFound(branch.clone())),
    }
}
