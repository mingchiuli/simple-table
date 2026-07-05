export * from './generated';

import type { WorkbookCapabilities } from './generated';

export function defaultWorkbookCapabilities(): WorkbookCapabilities {
  return {
    canEditCells: true,
    canResizeRowsColumns: true,
    canInsertDeleteRows: true,
    canInsertDeleteColumns: true,
    canInsertDeleteSheets: true,
    canNativeSave: true,
    blockedStructureReasons: [],
    blockedEditReasons: [],
    blockedResizeReasons: [],
    blockedRowStructureReasons: [],
    blockedColumnStructureReasons: [],
    blockedSheetStructureReasons: [],
    detectedFeatures: [],
  };
}
