export type CellValue = string | number | boolean | null;

export interface SheetData {
  name: string;
  rows: CellValue[][];
}

export interface FileData {
  file_name: string;
  sheets: SheetData[];
}

export interface CellChange {
  row: number;
  col: number;
  value: CellValue;
}

export interface RowChange {
  index: number;
  values: CellValue[];
}

export interface ColumnChange {
  index: number;
}

// Rust 使用 #[serde(tag = "type", content = "data")]，所以格式是 { type: 'SetCell', data: {...} }
export type OperationResult =
  | { type: 'SetCell'; data: { sheet_index: number; cell: CellChange } }
  | { type: 'AddRow'; data: { sheet_index: number; row: RowChange } }
  | { type: 'DeleteRow'; data: { sheet_index: number; row_index: number } }
  | { type: 'AddColumn'; data: { sheet_index: number; column: ColumnChange } }
  | { type: 'DeleteColumn'; data: { sheet_index: number; column_index: number } }
  | { type: 'Batch'; data: { sheet_index: number; changes: CellChange[] } };

export interface SearchResult {
  sheet_index: number;
  sheet_name: string;
  row: number;
  col: number;
  value: string;
  cell_position: string;
}

export type SearchScope = 'currentSheet' | 'allSheets';
