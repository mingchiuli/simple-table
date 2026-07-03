import type { CellFormatProjection, CellValue, ScalarCellValue } from "@/types";

export function formatCellDisplay(
  value: CellValue | undefined,
  format?: CellFormatProjection
): string {
  if (!value) return "";
  if (value.formula?.error) return value.formula.error;
  const raw = value.raw;
  const numberFormat = format?.numberFormat;
  if (numberFormat && typeof raw === "number") {
    return formatNumberWithExcelPattern(raw, numberFormat) ?? value.display ?? String(raw);
  }
  return value.display ?? scalarToString(raw);
}

function formatNumberWithExcelPattern(value: number, pattern: string): string | null {
  const normalized = pattern.toLowerCase();
  if (normalized.includes("yy") || normalized.includes("dd") || normalized.includes("m/")) {
    return formatExcelDate(value, pattern);
  }

  const percent = pattern.includes("%");
  const displayed = percent ? value * 100 : value;
  const decimals = decimalPlaces(pattern);
  const currency = /[$¥￥€£]/.exec(pattern)?.[0];

  const formatted = new Intl.NumberFormat(undefined, {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
    useGrouping: pattern.includes(","),
  }).format(displayed);

  return `${currency ?? ""}${formatted}${percent ? "%" : ""}`;
}

function decimalPlaces(pattern: string): number {
  const decimal = /[.](0+|#+)/.exec(pattern);
  return decimal?.[1]?.length ?? 0;
}

function formatExcelDate(value: number, pattern: string): string | null {
  if (!Number.isFinite(value)) return null;
  const date = new Date(Date.UTC(1899, 11, 30) + Math.round(value) * 86_400_000);
  if (Number.isNaN(date.getTime())) return null;

  const year = String(date.getUTCFullYear());
  const month = String(date.getUTCMonth() + 1).padStart(2, "0");
  const day = String(date.getUTCDate()).padStart(2, "0");

  if (pattern.includes("-")) return `${year}-${month}-${day}`;
  if (pattern.includes("/")) return `${year}/${month}/${day}`;
  return `${year}-${month}-${day}`;
}

function scalarToString(value: ScalarCellValue | undefined): string {
  if (value === null || value === undefined) return "";
  return String(value);
}
