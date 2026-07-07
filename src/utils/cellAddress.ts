export type CellAddress = {
  row: number;
  col: number;
};

export function cellKey(row: number, col: number): string {
  return `${columnName(col)}${row + 1}`;
}

export function parseCellKey(key: string): CellAddress | null {
  const match = /^([A-Z]+)([1-9]\d*)$/i.exec(key.trim());
  if (!match) return null;

  const colName = match[1].toUpperCase();
  let col = 0;
  for (const char of colName) {
    col = col * 26 + char.charCodeAt(0) - 64;
  }

  return {
    row: Number(match[2]) - 1,
    col: col - 1,
  };
}

function columnName(colIndex: number): string {
  let col = colIndex + 1;
  let result = "";
  while (col > 0) {
    const rem = (col - 1) % 26;
    result = String.fromCharCode(65 + rem) + result;
    col = Math.floor((col - 1) / 26);
  }
  return result;
}
