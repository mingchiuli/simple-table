export type CellValue = string | number | boolean | null;

export interface SheetData {
  name: string;
  rows: CellValue[][];
}

export interface FileData {
  file_name: string;
  sheets: SheetData[];
}
