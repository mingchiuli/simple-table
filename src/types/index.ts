export type CellValue = string | number | boolean | null;

export interface MergeRange {
  start_row: number;
  start_col: number;
  end_row: number;
  end_col: number;
}

export interface SheetData {
  name: string;
  rows: CellValue[][];
  merges: MergeRange[];
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

export interface SortState {
  col_index: number;
  ascending: boolean;
}

// Rust 使用 #[serde(tag = "type", content = "data")]，所以格式是 { type: 'SetCell', data: {...} }
export type OperationResult =
  | { type: 'SetCell'; data: { sheet_index: number; cell: CellChange } }
  | { type: 'AddRow'; data: { sheet_index: number; row: RowChange } }
  | { type: 'DeleteRow'; data: { sheet_index: number; row_index: number } }
  | { type: 'AddColumn'; data: { sheet_index: number; column: ColumnChange; col_data: CellValue[] } }
  | { type: 'DeleteColumn'; data: { sheet_index: number; column_index: number } }
  | { type: 'AddSheet'; data: { sheet_index: number; name: string; sheet_data: SheetData } }
  | { type: 'DeleteSheet'; data: { sheet_index: number; sheet_data: SheetData } }
  | { type: 'SortColumn'; data: { sheet_index: number; sheet_data: SheetData; sort_state: SortState | null } };

export interface SearchResult {
  sheet_index: number;
  sheet_name: string;
  row: number;
  col: number;
  value: string;
  cell_position: string;
}

export type SearchScope = 'currentSheet' | 'allSheets';
