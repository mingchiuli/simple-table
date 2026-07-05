import { cellKey } from "@/utils/cellAddress";

/**
 * 将列索引转换为 Excel 列名
 * @param col 列索引 (0-based)
 * @returns Excel 列名 (0 -> "A", 25 -> "Z", 26 -> "AA", ...)
 */
export function colToLetter(col: number): string {
  let result = "";
  let n = col;
  while (n >= 26) {
    result = String.fromCharCode(65 + (n % 26)) + result;
    n = Math.floor(n / 26) - 1;
  }
  result = String.fromCharCode(65 + n) + result;
  return result;
}

/**
 * 将单元格位置转换为 Excel 格式
 * @param row 行索引 (0-based)
 * @param col 列索引 (0-based)
 * @returns Excel 格式的单元格位置 (如 "A1", "B2", "AA10")
 */
export function toCellPosition(row: number, col: number): string {
  return cellKey(row, col);
}
