import type {
  CellValue,
  EditorSessionInfo,
  MergeRange,
  OpenDocumentResponse,
  ReadOnlyRichProjection,
  SavedDocumentResponse,
  SheetRegionProjectionResponse,
} from '@/types';
import { calculateSheetExtent } from '@/table-geometry/sheetExtent';

export type SheetData = {
  name: string;
  rows: CellValue[][];
  merges: MergeRange[];
  columnWidths?: Record<number, number>;
  rowHeights?: Record<number, number>;
  rich: ReadOnlyRichProjection;
};

export type FileData = {
  path: string;
  fileName: string;
  sheets: SheetData[];
};

export function openResponseFromFileData(
  fileData: FileData,
  editorSession: EditorSessionInfo,
  initialSheetIndex = 0
): OpenDocumentResponse {
  return {
    document: {
      path: fileData.path,
      fileName: fileData.fileName,
      sheets: fileData.sheets.map((sheet) => ({
        name: sheet.name,
        extent: extentOf(sheet),
        layout: layoutOf(sheet),
      })),
    },
    editorSession,
    initialRegion: fileData.sheets[initialSheetIndex]
      ? regionFromSheet(fileData.sheets[initialSheetIndex], initialSheetIndex, editorSession)
      : undefined,
  };
}

export function savedResponseFromFileData(
  fileData: FileData,
  editorSession: EditorSessionInfo
): SavedDocumentResponse {
  return {
    document: {
      path: fileData.path,
      fileName: fileData.fileName,
      sheets: fileData.sheets.map((sheet) => ({
        name: sheet.name,
        extent: extentOf(sheet),
        layout: layoutOf(sheet),
      })),
    },
    editorSession,
  };
}

function regionFromSheet(
  sheet: SheetData,
  sheetIndex: number,
  editorSession: EditorSessionInfo
): SheetRegionProjectionResponse {
  const extent = extentOf(sheet);
  return {
    documentId: editorSession.documentId,
    revision: editorSession.revision,
    region: {
      sheetIndex,
      rowStart: 0,
      rowEnd: extent.rowCount,
      colStart: 0,
      colEnd: extent.columnCount,
    },
    cells: sheet.rows.flatMap((row, rowIndex) =>
      row.map((value, col) => ({ sheetIndex, row: rowIndex, col, value }))
    ),
    mergeAnchorCells: [],
    metadata: {
      merges: sheet.merges,
      cellFormats: sheet.rich.cellFormats ?? {},
      cellStyles: sheet.rich.cellStyles ?? {},
    },
  };
}

function layoutOf(sheet: SheetData) {
  return {
    columnWidths: sheet.columnWidths ?? {},
    rowHeights: sheet.rowHeights ?? {},
  };
}

function extentOf(sheet: SheetData) {
  return calculateSheetExtent(
    sheet.rows,
    sheet.merges,
    sheet.columnWidths,
    sheet.rowHeights,
    sheet.rich
  );
}
