use std::fmt;
use std::str::FromStr;

use schemars::JsonSchema;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest as _, Sha256};
use ulid::Ulid;

#[derive(Clone, Debug, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd)]
#[schemars(with = "String")]
pub struct CaptureId(String);

impl CaptureId {
    #[must_use]
    pub fn new() -> Self {
        Self::from_ulid(Ulid::generate())
    }

    #[must_use]
    pub fn from_ulid(value: Ulid) -> Self {
        Self(format!("cap_{value}"))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for CaptureId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for CaptureId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for CaptureId {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let encoded = value
            .strip_prefix("cap_")
            .ok_or_else(|| "capture ID must start with `cap_`".to_owned())?;
        let ulid = Ulid::from_string(encoded).map_err(|error| error.to_string())?;
        Ok(Self::from_ulid(ulid))
    }
}

impl Serialize for CaptureId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for CaptureId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(D::Error::custom)
    }
}

macro_rules! numeric_id {
    ($name:ident, $repr:ty) => {
        #[derive(
            Clone,
            Copy,
            Debug,
            Deserialize,
            Eq,
            Hash,
            JsonSchema,
            Ord,
            PartialEq,
            PartialOrd,
            Serialize,
        )]
        #[serde(transparent)]
        pub struct $name($repr);

        impl $name {
            #[must_use]
            pub const fn new(value: $repr) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn get(self) -> $repr {
                self.0
            }
        }

        impl From<$repr> for $name {
            fn from(value: $repr) -> Self {
                Self::new(value)
            }
        }
    };
}

numeric_id!(FrameId, u64);
numeric_id!(NodeId, u32);
numeric_id!(ResourceId, u32);

#[derive(Clone, Copy, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd)]
#[schemars(with = "String")]
pub struct ContentDigest([u8; Self::BYTE_LENGTH]);

impl ContentDigest {
    pub const BYTE_LENGTH: usize = 32;
    pub const HEX_LENGTH: usize = Self::BYTE_LENGTH * 2;

    #[must_use]
    pub fn sha256(bytes: impl AsRef<[u8]>) -> Self {
        Self(Sha256::digest(bytes.as_ref()).into())
    }

    #[must_use]
    pub const fn from_bytes(bytes: [u8; Self::BYTE_LENGTH]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; Self::BYTE_LENGTH] {
        &self.0
    }

    #[must_use]
    pub fn to_hex(self) -> String {
        hex::encode(self.0)
    }
}

impl fmt::Debug for ContentDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("ContentDigest")
            .field(&self.to_hex())
            .finish()
    }
}

impl fmt::Display for ContentDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.to_hex())
    }
}

impl FromStr for ContentDigest {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != Self::HEX_LENGTH {
            return Err(format!(
                "SHA-256 digest must contain {} hexadecimal characters",
                Self::HEX_LENGTH
            ));
        }
        let decoded = hex::decode(value).map_err(|error| error.to_string())?;
        let bytes = decoded
            .try_into()
            .map_err(|_| "SHA-256 digest has an invalid length".to_owned())?;
        Ok(Self(bytes))
    }
}

impl Serialize for ContentDigest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for ContentDigest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::{CaptureId, ContentDigest};

    #[test]
    fn capture_id_round_trips_through_its_public_string() {
        let id = CaptureId::new();
        let parsed = id.as_str().parse::<CaptureId>();

        assert_eq!(parsed.as_ref(), Ok(&id));
    }

    #[test]
    fn digest_serializes_as_lowercase_sha256() {
        let digest = ContentDigest::sha256(b"offprint");

        assert_eq!(
            digest.to_string(),
            "2ae7f600c8a52ffe2db0c03c530540a18fc46e670b4d1b91b4639ba7d075a3fb"
        );
    }
}
