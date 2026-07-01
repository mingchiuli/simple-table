import type { CellKind, CellValue, ScalarCellValue } from "@/types";

export function blankCell(): CellValue {
  return {
    type: "cell",
    kind: "blank",
    raw: null,
    display: "",
  };
}

export function cellToEditorString(value: CellValue | undefined): string {
  if (!value) return "";
  return value.formula?.formula ?? scalarToString(value.raw);
}

export function cellToDisplayString(value: CellValue | undefined): string {
  if (!value) return "";
  return value.formula?.error ?? value.display ?? scalarToString(value.raw);
}

export function cellKind(value: CellValue | undefined): CellKind {
  return value?.kind ?? "blank";
}

function scalarToString(value: ScalarCellValue | undefined): string {
  if (value === null || value === undefined) return "";
  return String(value);
}
