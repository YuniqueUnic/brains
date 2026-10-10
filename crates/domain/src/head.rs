//! HEAD is the only commit point per logical path (design §7.1).
//! Serialized form matches contracts/head.schema.json.

use serde::{Deserialize, Serialize};

use crate::error::{DomainError, DomainResult};
use crate::id::HexId;
use crate::path::VaultPath;
use crate::timestamp::validate_rfc3339;

pub const SCHEMA_VERSION: u64 = 1;

/// Persisted HEAD state. Only `live` / `trashed` exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HeadState {
    Live,
    Trashed,
}

impl HeadState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Trashed => "trashed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Head {
    pub schema_version: u64,
    pub path: String,
    pub revision: HexId,
    pub generation: i64,
    pub state: HeadState,
    pub updated_at: String,
}

pub const MAX_GENERATION: i64 = 9_007_199_254_740_991; // 2^53 - 1

impl Head {
    pub fn new(
        path: &VaultPath,
        revision: HexId,
        generation: i64,
        state: HeadState,
        updated_at: String,
    ) -> DomainResult<Self> {
        let head = Self {
            schema_version: SCHEMA_VERSION,
            path: path.as_str().to_owned(),
            revision,
            generation,
            state,
            updated_at,
        };
        head.validate()?;
        Ok(head)
    }

    /// Structural validation applied on every load from durable storage.
    pub fn validate(&self) -> DomainResult<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(DomainError::Internal(format!(
                "unsupported HEAD schema_version {}",
                self.schema_version
            )));
        }
        VaultPath::new(&self.path)?;
        if self.generation < 1 || self.generation > MAX_GENERATION {
            return Err(DomainError::Internal(format!(
                "HEAD generation {} out of range",
                self.generation
            )));
        }
        validate_rfc3339(&self.updated_at)?;
        Ok(())
    }

    pub fn path(&self) -> VaultPath {
        VaultPath::trusted(self.path.clone())
    }

    pub fn is_live(&self) -> bool {
        self.state == HeadState::Live
    }
}
