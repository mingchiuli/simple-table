#![allow(dead_code)]

use ts_rs::{Config, TS};

use crate::recent::types::{RecentFile, StorageType};
use crate::state::state::{EditorSessionInfo, EditorStateInfo, HistoryStatus};
use crate::types::{
    CellData, CellFormatProjection, CellFormulaProjection, CellKind, CellStyleProjection,
    CellValue, ColumnDeletedPatch, ColumnInsertedPatch, DocumentCapabilities, DrawingKind,
    DrawingProjection, EditorMutationResponse, EditorPatch, FileData, FormulaDiagnostics,
    FormulaIssue, FormulaIssueKind, FormulaStatus, FreezePaneProjection, HyperlinkProjection,
    LayoutPatch, MergeRange, NativeSavePlan, OpenDocumentResponse, ReadOnlyRichProjection,
    ResyncRequiredPatch, RichProjectionPatch, RichProjectionPatchScope, RowDeletedPatch,
    RowInsertedPatch, SavedDocumentResponse, ScalarCellValue, SearchResult, SearchScope,
    SetCellRequest, SheetCapabilities, SheetData, SheetDeletedPatch, SheetInsertedPatch,
    SheetShapePatch, SheetStructureMetadataPatch, SheetUpdatedPatch, SheetsReplacedPatch,
    WorkbookCapabilities, WorkbookRichCapabilities, WorkbookSaveCapabilities,
    WorkbookStructureCapabilities,
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
    push_decl::<SheetCapabilities>(&mut output, &cfg);
    push_decl::<WorkbookSaveCapabilities>(&mut output, &cfg);
    push_decl::<WorkbookStructureCapabilities>(&mut output, &cfg);
    push_decl::<WorkbookRichCapabilities>(&mut output, &cfg);
    push_decl::<WorkbookCapabilities>(&mut output, &cfg);
    push_decl::<DocumentCapabilities>(&mut output, &cfg);
    push_decl::<NativeSavePlan>(&mut output, &cfg);
    push_decl::<crate::types::SheetCellChange>(&mut output, &cfg);
    push_decl::<SetCellRequest>(&mut output, &cfg);
    push_decl::<SearchResult>(&mut output, &cfg);
    push_decl::<SearchScope>(&mut output, &cfg);
    push_decl::<StorageType>(&mut output, &cfg);
    push_decl::<RecentFile>(&mut output, &cfg);
    push_decl::<HistoryStatus>(&mut output, &cfg);
    push_decl::<EditorStateInfo>(&mut output, &cfg);
    push_decl::<FormulaIssueKind>(&mut output, &cfg);
    push_decl::<FormulaIssue>(&mut output, &cfg);
    push_decl::<FormulaDiagnostics>(&mut output, &cfg);
    push_decl::<FormulaStatus>(&mut output, &cfg);
    push_decl::<LayoutPatch>(&mut output, &cfg);
    push_decl::<SheetInsertedPatch>(&mut output, &cfg);
    push_decl::<SheetDeletedPatch>(&mut output, &cfg);
    push_decl::<SheetUpdatedPatch>(&mut output, &cfg);
    push_decl::<SheetsReplacedPatch>(&mut output, &cfg);
    push_decl::<RichProjectionPatchScope>(&mut output, &cfg);
    push_decl::<RichProjectionPatch>(&mut output, &cfg);
    push_decl::<SheetStructureMetadataPatch>(&mut output, &cfg);
    push_decl::<RowInsertedPatch>(&mut output, &cfg);
    push_decl::<RowDeletedPatch>(&mut output, &cfg);
    push_decl::<ColumnInsertedPatch>(&mut output, &cfg);
    push_decl::<ColumnDeletedPatch>(&mut output, &cfg);
    push_decl::<SheetShapePatch>(&mut output, &cfg);
    push_decl::<ResyncRequiredPatch>(&mut output, &cfg);
    push_decl::<EditorPatch>(&mut output, &cfg);
    push_decl::<EditorMutationResponse>(&mut output, &cfg);
    push_decl::<EditorSessionInfo>(&mut output, &cfg);
    push_decl::<OpenDocumentResponse>(&mut output, &cfg);
    push_decl::<SavedDocumentResponse>(&mut output, &cfg);

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
