export * from './generated';
export type * from './updateRuntime';
export type * from './documentRuntime';
export type * from './pendingCellSave';
export * from './editorRuntime';

import type { ReadOnlyRichProjection } from './generated';

export function defaultRichProjection(): ReadOnlyRichProjection {
  return {
    cellFormats: {},
    cellStyles: {},
    hiddenRows: [],
    hiddenColumns: [],
    freezePane: undefined,
    hyperlinks: {},
    drawings: [],
    hasMoreDrawings: false,
    hasStyleMetadata: false,
    hasHyperlinks: false,
    hasFreezePane: false,
  };
}
