export type CellValue = string | number | boolean | null;

export interface MergeRange {
  startRow: number;
  startCol: number;
  endRow: number;
  endCol: number;
}

export interface SheetData {
  name: string;
  rows: CellValue[][];
  merges: MergeRange[];
  columnWidths?: Record<number, number>;
}

export interface FileData {
  fileName: string;
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
  colIndex: number;
  ascending: boolean;
}

// Rust 使用 #[serde(tag = "type", content = "data")]，所以格式是 { type: 'SetCell', data: {...} }
export type OperationResult =
  | { type: 'SetCell'; data: { sheetIndex: number; cell: CellChange } }
  | { type: 'AddRow'; data: { sheetIndex: number; row: RowChange } }
  | { type: 'DeleteRow'; data: { sheetIndex: number; rowIndex: number } }
  | { type: 'AddColumn'; data: { sheetIndex: number; column: ColumnChange; colData: CellValue[] } }
  | { type: 'DeleteColumn'; data: { sheetIndex: number; columnIndex: number } }
  | { type: 'AddSheet'; data: { sheetIndex: number; name: string; sheetData: SheetData } }
  | { type: 'DeleteSheet'; data: { sheetIndex: number; sheetData: SheetData } }
  | { type: 'SortColumn'; data: { sheetIndex: number; sheetData: SheetData; sortState: SortState | null } };

export interface SearchResult {
  sheetIndex: number;
  sheetName: string;
  row: number;
  col: number;
  value: string;
  cellPosition: string;
}

// Element Plus 表格列配置类型
import type { VNode } from 'vue';

export interface ColumnConfig {
  key: string;
  title: string;
  width?: number;
  fixed?: 'left' | 'right';
  dataKey?: number;
  headerCellRenderer?: () => VNode;
}

export interface RecentFile {
  id: string;
  path: string;
  fileName: string;
  lastOpened: number;
  fileSize: number;
  thumbnail?: string;
}
