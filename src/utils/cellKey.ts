export function getCellKey(sheetIndex: number, row: number, col: number): string {
  return `${sheetIndex},${row},${col}`;
}
