pub mod android;
mod document;
mod editor;
mod execution_runtime;
mod file;
mod input;
pub mod ios;
#[cfg(any(target_os = "android", target_os = "ios"))]
pub mod mobile;
mod recent;
mod search;

pub use document::*;
pub use editor::*;
pub use execution_runtime::CommandExecutionRuntime;
pub use file::*;
pub use recent::*;
pub use search::*;

#[derive(Clone, Copy, Debug)]
pub(crate) struct CommandU64(u64);

impl CommandU64 {
    pub(crate) fn get(self) -> u64 {
        self.0
    }
}

impl<'de> serde::Deserialize<'de> for CommandU64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = <String as serde::Deserialize>::deserialize(deserializer)?;
        value.parse().map(Self).map_err(serde::de::Error::custom)
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
pub use mobile::check_update_mobile;

#[cfg(test)]
mod tests {
    use super::CommandU64;

    #[test]
    fn command_u64_accepts_decimal_strings_across_the_full_range() {
        let value: CommandU64 =
            serde_json::from_str(r#""18446744073709551615""#).expect("deserialize u64 max");

        assert_eq!(value.get(), u64::MAX);
    }

    #[test]
    fn command_u64_rejects_json_numbers() {
        let result = serde_json::from_str::<CommandU64>("9007199254740993");

        assert!(result.is_err());
    }
}
