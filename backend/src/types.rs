mod capabilities;
mod cell;
mod cell_change;
mod document;
mod editor_session;
mod file;
mod formula;
mod image;
mod mutation;
mod recent;
mod search;

pub use capabilities::*;
pub use cell::*;
pub use cell_change::*;
pub use document::*;
pub use editor_session::*;
pub use file::*;
pub use formula::*;
pub use image::*;
pub use mutation::*;
pub use recent::*;
pub use search::*;

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
