#![allow(dead_code)]

use quote::ToTokens;
use syn::{FnArg, GenericArgument, Item, PathArguments, ReturnType, Type};
use ts_rs::{Config, TS};

use crate::editor_protocol::{
    EDITOR_MUTATION_PROTOCOL_VERSION, MAX_CELL_TEXT_BYTES, MAX_MUTATION_RESPONSE_BYTES,
    MAX_MUTATION_TEXT_BYTES, MAX_SET_CELL_CHANGES, MAX_SHEET_REGION_RESPONSE_BYTES,
};
use crate::recent::types::{AddRecentFileRequest, RecentFile, StorageType};
use crate::types::{
    CellData, CellFormatProjection, CellFormulaProjection, CellKind, CellStyleProjection,
    CellValue, ColumnDeletedPatch, ColumnInsertedPatch, DocumentCapabilities, DocumentManifest,
    DrawingKind, DrawingProjection, EditorCommandContext, EditorMutationResponse, EditorPatch,
    EditorSessionInfo, EditorStateInfo, FormulaDiagnostics, FormulaIssue, FormulaIssueKind,
    FormulaStatus, FreezePaneProjection, HistoryStatus, HyperlinkProjection, LayoutPatch,
    MergeRange, MutationResultLookup, MutationResultStatus, NativeSavePlan, OpenDocumentResponse,
    PreparedOpenDocument, ReadOnlyRichProjection, ResyncRequiredPatch, RowDeletedPatch,
    RowInsertedPatch, SavedDocumentIdentity, SavedDocumentResponse, ScalarCellValue,
    SearchResponse, SearchResult, SearchScope, SetCellRequest, SheetCapabilities,
    SheetDeletedPatch, SheetExtent, SheetInsertedPatch, SheetInvalidatedPatch,
    SheetLayoutProjection, SheetManifest, SheetRegion, SheetRegionMetadata,
    SheetRegionProjectionResponse, SheetsReplacedPatch, SpreadsheetFormatOptions, UpdateInfo,
    WorkbookCapabilities, WorkbookRichCapabilities, WorkbookSaveCapabilities,
    WorkbookStructureCapabilities,
};

/// TypeScript editor protocol emitted for the frontend from Rust definitions.
pub fn generated_typescript_contract() -> String {
    let cfg = Config::default();
    let mut output =
        String::from("// Generated from Rust editor contract by ts-rs. Do not edit by hand.\n\n");
    output.push_str("export type U64String = `${bigint}`;\n\n");
    output.push_str(&format!(
        "export const EDITOR_MUTATION_PROTOCOL_VERSION = {EDITOR_MUTATION_PROTOCOL_VERSION} as const;\n"
    ));
    output.push_str(&format!(
        "export const MAX_MUTATION_RESPONSE_BYTES = {MAX_MUTATION_RESPONSE_BYTES} as const;\n"
    ));
    output.push_str(&format!(
        "export const MAX_SHEET_REGION_RESPONSE_BYTES = {MAX_SHEET_REGION_RESPONSE_BYTES} as const;\n\n"
    ));
    output.push_str(&format!(
        "export const MAX_SET_CELL_CHANGES = {MAX_SET_CELL_CHANGES} as const;\n"
    ));
    output.push_str(&format!(
        "export const MAX_CELL_TEXT_BYTES = {MAX_CELL_TEXT_BYTES} as const;\n"
    ));
    output.push_str(&format!(
        "export const MAX_MUTATION_TEXT_BYTES = {MAX_MUTATION_TEXT_BYTES} as const;\n\n"
    ));

    push_decl::<ScalarCellValue>(&mut output, &cfg);
    push_decl::<CellKind>(&mut output, &cfg);
    push_decl::<CellFormulaProjection>(&mut output, &cfg);
    push_decl::<CellData>(&mut output, &cfg);
    push_decl::<CellValue>(&mut output, &cfg);
    push_decl::<CellFormatProjection>(&mut output, &cfg);
    push_decl::<MergeRange>(&mut output, &cfg);
    push_decl::<SheetExtent>(&mut output, &cfg);
    push_decl::<SheetLayoutProjection>(&mut output, &cfg);
    push_decl::<SheetManifest>(&mut output, &cfg);
    push_decl::<DocumentManifest>(&mut output, &cfg);
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
    push_decl::<SpreadsheetFormatOptions>(&mut output, &cfg);
    push_decl::<crate::types::SheetCellChange>(&mut output, &cfg);
    push_decl::<SetCellRequest>(&mut output, &cfg);
    push_decl::<SearchResult>(&mut output, &cfg);
    push_decl::<SearchResponse>(&mut output, &cfg);
    push_decl::<SearchScope>(&mut output, &cfg);
    push_decl::<StorageType>(&mut output, &cfg);
    push_decl::<RecentFile>(&mut output, &cfg);
    push_decl::<AddRecentFileRequest>(&mut output, &cfg);
    push_decl::<UpdateInfo>(&mut output, &cfg);
    push_decl::<HistoryStatus>(&mut output, &cfg);
    push_decl::<EditorStateInfo>(&mut output, &cfg);
    push_decl::<FormulaIssueKind>(&mut output, &cfg);
    push_decl::<FormulaIssue>(&mut output, &cfg);
    push_decl::<FormulaDiagnostics>(&mut output, &cfg);
    push_decl::<FormulaStatus>(&mut output, &cfg);
    push_decl::<LayoutPatch>(&mut output, &cfg);
    push_decl::<SheetInsertedPatch>(&mut output, &cfg);
    push_decl::<SheetDeletedPatch>(&mut output, &cfg);
    push_decl::<SheetInvalidatedPatch>(&mut output, &cfg);
    push_decl::<SheetsReplacedPatch>(&mut output, &cfg);
    push_decl::<RowInsertedPatch>(&mut output, &cfg);
    push_decl::<RowDeletedPatch>(&mut output, &cfg);
    push_decl::<ColumnInsertedPatch>(&mut output, &cfg);
    push_decl::<ColumnDeletedPatch>(&mut output, &cfg);
    push_decl::<ResyncRequiredPatch>(&mut output, &cfg);
    push_decl::<EditorPatch>(&mut output, &cfg);
    push_decl::<EditorCommandContext>(&mut output, &cfg);
    push_decl::<EditorMutationResponse>(&mut output, &cfg);
    push_decl::<MutationResultStatus>(&mut output, &cfg);
    push_decl::<MutationResultLookup>(&mut output, &cfg);
    push_decl::<EditorSessionInfo>(&mut output, &cfg);
    push_decl::<OpenDocumentResponse>(&mut output, &cfg);
    push_decl::<SheetRegion>(&mut output, &cfg);
    push_decl::<SheetRegionMetadata>(&mut output, &cfg);
    push_decl::<SheetRegionProjectionResponse>(&mut output, &cfg);
    push_decl::<PreparedOpenDocument>(&mut output, &cfg);
    push_decl::<SavedDocumentIdentity>(&mut output, &cfg);
    push_decl::<SavedDocumentResponse>(&mut output, &cfg);
    push_tauri_command_map(&mut output);

    output
}

fn push_tauri_command_map(output: &mut String) {
    output.push_str("export type TauriCommandMap = {\n");
    for source in [
        include_str!("../commands/common.rs"),
        include_str!("../commands/android.rs"),
        include_str!("../commands/ios.rs"),
        include_str!("../commands/mobile.rs"),
    ] {
        let syntax = syn::parse_file(source).expect("parse Tauri command source");
        for item in syntax.items {
            let Item::Fn(function) = item else { continue };
            if !function.attrs.iter().any(is_tauri_command_attribute) {
                continue;
            }
            let camel_case = !function.attrs.iter().any(command_uses_snake_case);
            let arguments = function
                .sig
                .inputs
                .iter()
                .filter_map(|argument| command_argument(argument, camel_case))
                .collect::<Vec<_>>();
            let arguments = if arguments.is_empty() {
                "Record<string, never>".to_string()
            } else {
                format!("{{ {} }}", arguments.join(", "))
            };
            let result = match &function.sig.output {
                ReturnType::Default => "void".to_string(),
                ReturnType::Type(_, ty) => command_type(ty),
            };
            output.push_str(&format!(
                "  \"{}\": {{ args: {}, result: {} }},\n",
                function.sig.ident, arguments, result
            ));
        }
    }
    output.push_str("}\n\n");
}

fn is_tauri_command_attribute(attribute: &syn::Attribute) -> bool {
    let segments = &attribute.path().segments;
    segments.len() == 2 && segments[0].ident == "tauri" && segments[1].ident == "command"
}

fn command_uses_snake_case(attribute: &syn::Attribute) -> bool {
    is_tauri_command_attribute(attribute)
        && attribute
            .meta
            .to_token_stream()
            .to_string()
            .contains("snake_case")
}

fn command_argument(argument: &FnArg, camel_case: bool) -> Option<String> {
    let FnArg::Typed(argument) = argument else {
        return None;
    };
    let syn::Pat::Ident(name) = argument.pat.as_ref() else {
        return None;
    };
    if matches!(
        type_name(&argument.ty).as_deref(),
        Some("AppHandle" | "State")
    ) {
        return None;
    }
    let name = if camel_case {
        snake_to_camel(&name.ident.to_string())
    } else {
        name.ident.to_string()
    };
    Some(format!("{name}: {}", command_type(&argument.ty)))
}

fn command_type(ty: &Type) -> String {
    match ty {
        Type::Reference(reference) => command_type(&reference.elem),
        Type::Tuple(tuple) if tuple.elems.is_empty() => "void".to_string(),
        Type::Path(path) => {
            let Some(segment) = path.path.segments.last() else {
                return "unknown".to_string();
            };
            let name = segment.ident.to_string();
            if matches!(name.as_str(), "Option" | "Vec" | "Result") {
                let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
                    return "unknown".to_string();
                };
                let mut types = arguments.args.iter().filter_map(|argument| match argument {
                    GenericArgument::Type(ty) => Some(command_type(ty)),
                    _ => None,
                });
                let first = types.next().unwrap_or_else(|| "unknown".to_string());
                return match name.as_str() {
                    "Option" => format!("{first} | null"),
                    "Vec" => format!("Array<{first}>"),
                    "Result" => first,
                    _ => unreachable!(),
                };
            }
            match name.as_str() {
                "String" | "str" => "string".to_string(),
                "usize" | "u8" | "u16" | "u32" | "i32" | "f32" | "f64" => "number".to_string(),
                "bool" => "boolean".to_string(),
                "CommandU64" => "U64String".to_string(),
                "BoundedCellText" => "string".to_string(),
                "SetCellBatch" => "Array<SetCellRequest>".to_string(),
                "DesktopOpenFileInfo" => "{ path: string, fileName: string }".to_string(),
                "PickedFileInfo" => {
                    "{ path: string, originalPath: string, fileName: string }".to_string()
                }
                _ => name,
            }
        }
        _ => "unknown".to_string(),
    }
}

fn type_name(ty: &Type) -> Option<String> {
    let Type::Path(path) = ty else { return None };
    path.path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
}

fn snake_to_camel(value: &str) -> String {
    let mut parts = value.split('_');
    let mut output = parts.next().unwrap_or_default().to_string();
    for part in parts {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            output.extend(first.to_uppercase());
            output.extend(chars);
        }
    }
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

    #[test]
    fn generated_commands_are_registered_and_have_known_types() {
        let generated = generated_typescript_contract();
        assert!(!generated.contains("unknown"));
        let lib = include_str!("../lib.rs");
        let handler = lib
            .split("tauri::generate_handler![")
            .nth(1)
            .and_then(|tail| tail.split("])\n        .run").next())
            .expect("registered Tauri handler");

        for line in generated.lines().filter(|line| line.starts_with("  \"")) {
            let command = line
                .trim_start()
                .split('"')
                .nth(1)
                .expect("generated command name");
            assert!(
                handler.contains(command),
                "generated command {command} is not registered"
            );
        }
    }
}
