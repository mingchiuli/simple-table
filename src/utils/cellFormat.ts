import type { CellFormatProjection, CellValue, ScalarCellValue } from "@/types";

export function formatCellDisplay(
  value: CellValue | undefined,
  _format?: CellFormatProjection
): string {
  if (!value) return "";
  if (value.formula?.error) return value.formula.error;
  if (value.display !== undefined) {
    return value.display;
  }
  return scalarToString(value.raw);
}

function scalarToString(value: ScalarCellValue | undefined): string {
  if (value === null || value === undefined) return "";
  return String(value);
}
