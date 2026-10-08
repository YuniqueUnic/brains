//! 128-bit identifiers serialized as 32 lowercase hex characters.
//! A revision ID is random, never a timestamp and never a content hash.

use crate::error::{DomainError, DomainResult};

pub const HEX_LEN: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HexId(String);

impl HexId {
    pub fn generate() -> Self {
        // 128 random bits, 32 lowercase hex — same shape as a UUID simple
        // string, one less dependency; revision/operation ids are opaque.
        let bits: u128 = {
            use rand::RngExt;
            let mut bytes = [0u8; 16];
            rand::rng().fill(&mut bytes);
            u128::from_be_bytes(bytes)
        };
        Self(format!("{bits:032x}"))
    }

    pub fn from_hex(raw: &str) -> DomainResult<Self> {
        if raw.len() != HEX_LEN || !raw.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            return Err(DomainError::InvalidArgument(
                "identifier must be 32 lowercase hex characters".into(),
            ));
        }
        Ok(Self(raw.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for HexId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl serde::Serialize for HexId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> serde::Deserialize<'de> for HexId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        HexId::from_hex(&raw).map_err(serde::de::Error::custom)
    }
}
