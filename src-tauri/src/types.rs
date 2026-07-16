#![allow(clippy::module_inception)]

pub mod formula;
pub mod projection;
pub mod search;
pub mod types;
#[cfg(test)]
pub mod typescript;
pub mod update;

pub use formula::*;
#[cfg(test)]
pub use projection::*;
pub use search::*;
pub use types::*;
pub use update::*;

pub(crate) mod u64_string {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}
