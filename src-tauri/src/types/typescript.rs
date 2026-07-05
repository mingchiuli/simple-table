/// TypeScript editor protocol emitted for the frontend.
///
/// The frontend imports `src/types/generated.ts` rather than maintaining a
/// handwritten mirror in `src/types/index.ts`. The Rust test below keeps the
/// committed generated file anchored to this backend contract location.
#[allow(dead_code)]
pub fn generated_typescript_contract() -> &'static str {
    include_str!("../../../src/types/generated.ts")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn generated_typescript_contract_is_committed() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../src/types/generated.ts")
            .canonicalize()
            .expect("generated types path");
        let committed = fs::read_to_string(path).expect("read generated types");

        assert_eq!(committed, generated_typescript_contract());
    }
}
