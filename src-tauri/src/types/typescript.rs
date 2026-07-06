#![allow(dead_code)]

use ts_rs::{Config, TS};

use crate::recent::types::{RecentFile, StorageType};
use crate::state::state::{EditorSessionInfo, EditorStateInfo};
use crate::types::{
    CellData, CellFormatProjection, CellFormulaProjection, CellKind, CellStyleProjection,
    CellValue, DocumentCapabilities, DrawingKind, DrawingProjection, EditorMutationResponse,
    EditorPatch, FileData, FormulaDiagnostics, FormulaStatus, FreezePaneProjection,
    HyperlinkProjection, LayoutPatch, MergeRange, OpenDocumentResponse, ReadOnlyRichProjection,
    ResyncRequiredPatch, ScalarCellValue, SearchResult, SearchScope, SetCellRequest, SheetData,
    SheetDeletedPatch, SheetInsertedPatch, SheetShapePatch, SheetUpdatedPatch,
    WorkbookCapabilities,
};

/// TypeScript editor protocol emitted for the frontend from Rust definitions.
pub fn generated_typescript_contract() -> String {
    let cfg = Config::default();
    let mut output =
        String::from("// Generated from Rust editor contract by ts-rs. Do not edit by hand.\n\n");

    push_decl::<ScalarCellValue>(&mut output, &cfg);
    push_decl::<CellKind>(&mut output, &cfg);
    push_decl::<CellFormulaProjection>(&mut output, &cfg);
    push_decl::<CellData>(&mut output, &cfg);
    push_decl::<CellValue>(&mut output, &cfg);
    push_decl::<CellFormatProjection>(&mut output, &cfg);
    push_decl::<MergeRange>(&mut output, &cfg);
    push_decl::<SheetData>(&mut output, &cfg);
    push_decl::<FileData>(&mut output, &cfg);
    push_decl::<CellStyleProjection>(&mut output, &cfg);
    push_decl::<FreezePaneProjection>(&mut output, &cfg);
    push_decl::<HyperlinkProjection>(&mut output, &cfg);
    push_decl::<ReadOnlyRichProjection>(&mut output, &cfg);
    push_decl::<DrawingProjection>(&mut output, &cfg);
    push_decl::<DrawingKind>(&mut output, &cfg);
    push_decl::<WorkbookCapabilities>(&mut output, &cfg);
    push_decl::<DocumentCapabilities>(&mut output, &cfg);
    push_decl::<crate::types::SheetCellChange>(&mut output, &cfg);
    push_decl::<SetCellRequest>(&mut output, &cfg);
    push_decl::<SearchResult>(&mut output, &cfg);
    push_decl::<SearchScope>(&mut output, &cfg);
    push_decl::<StorageType>(&mut output, &cfg);
    push_decl::<RecentFile>(&mut output, &cfg);
    push_decl::<EditorStateInfo>(&mut output, &cfg);
    push_decl::<FormulaDiagnostics>(&mut output, &cfg);
    push_decl::<FormulaStatus>(&mut output, &cfg);
    push_decl::<LayoutPatch>(&mut output, &cfg);
    push_decl::<SheetInsertedPatch>(&mut output, &cfg);
    push_decl::<SheetDeletedPatch>(&mut output, &cfg);
    push_decl::<SheetUpdatedPatch>(&mut output, &cfg);
    push_decl::<SheetShapePatch>(&mut output, &cfg);
    push_decl::<ResyncRequiredPatch>(&mut output, &cfg);
    push_decl::<EditorPatch>(&mut output, &cfg);
    push_decl::<EditorMutationResponse>(&mut output, &cfg);
    push_decl::<EditorSessionInfo>(&mut output, &cfg);
    push_decl::<OpenDocumentResponse>(&mut output, &cfg);

    output
}

fn push_decl<T: TS + 'static>(output: &mut String, cfg: &Config) {
    output.push_str("export ");
    output.push_str(&T::decl(cfg));
    output.push_str("\n\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn generated_typescript_contract_is_current() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../src/types/generated.ts")
            .canonicalize()
            .expect("generated types path");
        let generated = generated_typescript_contract();
        if std::env::var_os("UPDATE_GENERATED_TYPES").is_some() {
            fs::write(&path, generated.as_bytes()).expect("write generated types");
        }

        let committed = fs::read_to_string(path).expect("read generated types");

        assert_eq!(committed, generated);
    }
}
