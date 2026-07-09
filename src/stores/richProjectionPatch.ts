import type {
  DrawingProjection,
  ReadOnlyRichProjection,
  RichProjectionPatch,
} from "@/types";
import { defaultRichProjection } from "@/types";
import { parseCellKey } from "@/utils/cellAddress";

export function applyRichProjectionPatch(
  current: ReadOnlyRichProjection | undefined,
  patch: RichProjectionPatch
): ReadOnlyRichProjection {
  const base = normalizeRichProjection(current);
  const projection = normalizeRichProjection(patch.projection);

  if (patch.scope.type === "all") {
    return finalizeRichProjection(projection);
  }

  if (patch.scope.type === "rows") {
    const start = patch.scope.start;
    return finalizeRichProjection({
      ...base,
      ...projection,
      hasMoreDrawings: base.hasMoreDrawings || projection.hasMoreDrawings,
      cellFormats: mergeCellMetadataByScope(base.cellFormats, projection.cellFormats, (row) => row >= start),
      cellStyles: mergeCellMetadataByScope(base.cellStyles, projection.cellStyles, (row) => row >= start),
      hyperlinks: mergeCellMetadataByScope(base.hyperlinks, projection.hyperlinks, (row) => row >= start),
      hiddenRows: mergeNumberArrayByScope(base.hiddenRows, projection.hiddenRows, (row) => row >= start),
      hiddenColumns: base.hiddenColumns,
      freezePane: mergeFreezePaneByScope(base.freezePane, projection.freezePane, (row) => row >= start),
      drawings: mergeDrawingsByScope(base.drawings, projection.drawings, (drawing) => drawingRowScopeAffected(drawing, start)),
    });
  }

  const start = patch.scope.start;
  return finalizeRichProjection({
    ...base,
    ...projection,
    hasMoreDrawings: base.hasMoreDrawings || projection.hasMoreDrawings,
    cellFormats: mergeCellMetadataByScope(base.cellFormats, projection.cellFormats, (_row, col) => col >= start),
    cellStyles: mergeCellMetadataByScope(base.cellStyles, projection.cellStyles, (_row, col) => col >= start),
    hyperlinks: mergeCellMetadataByScope(base.hyperlinks, projection.hyperlinks, (_row, col) => col >= start),
    hiddenRows: base.hiddenRows,
    hiddenColumns: mergeNumberArrayByScope(base.hiddenColumns, projection.hiddenColumns, (col) => col >= start),
    freezePane: mergeFreezePaneByScope(base.freezePane, projection.freezePane, (_row, col) => col >= start),
    drawings: mergeDrawingsByScope(base.drawings, projection.drawings, (drawing) => drawingColumnScopeAffected(drawing, start)),
  });
}

function normalizeRichProjection(
  projection: ReadOnlyRichProjection | undefined
): ReadOnlyRichProjection {
  return {
    ...defaultRichProjection(),
    ...(projection ?? {}),
    cellFormats: { ...(projection?.cellFormats ?? {}) },
    cellStyles: { ...(projection?.cellStyles ?? {}) },
    hyperlinks: { ...(projection?.hyperlinks ?? {}) },
    drawings: [...(projection?.drawings ?? [])],
    hiddenRows: [...(projection?.hiddenRows ?? [])],
    hiddenColumns: [...(projection?.hiddenColumns ?? [])],
  };
}

function finalizeRichProjection(projection: ReadOnlyRichProjection): ReadOnlyRichProjection {
  return {
    ...projection,
    hasStyleMetadata:
      Object.keys(projection.cellFormats ?? {}).length > 0 ||
      Object.keys(projection.cellStyles ?? {}).length > 0,
    hasHyperlinks: Object.keys(projection.hyperlinks ?? {}).length > 0,
    hasFreezePane: projection.freezePane != null,
    hasMoreDrawings: Boolean(projection.hasMoreDrawings),
  };
}

function mergeCellMetadataByScope<T>(
  current: Record<string, T> | undefined,
  patch: Record<string, T> | undefined,
  isAffected: (row: number, col: number) => boolean
): Record<string, T> {
  const next: Record<string, T> = {};
  for (const [key, value] of Object.entries(current ?? {})) {
    const address = parseCellKey(key);
    if (!address || !isAffected(address.row, address.col)) {
      next[key] = value;
    }
  }
  return { ...next, ...(patch ?? {}) };
}

function mergeNumberArrayByScope(
  current: number[] | undefined,
  patch: number[] | undefined,
  isAffected: (value: number) => boolean
): number[] {
  const retained = (current ?? []).filter((value) => !isAffected(value));
  return Array.from(new Set([...retained, ...(patch ?? [])])).sort((a, b) => a - b);
}

function mergeFreezePaneByScope(
  current: ReadOnlyRichProjection["freezePane"],
  patch: ReadOnlyRichProjection["freezePane"],
  isAffected: (row: number, col: number) => boolean
): ReadOnlyRichProjection["freezePane"] {
  const address = current?.topLeftCell ? parseCellKey(current.topLeftCell) : null;
  return address && !isAffected(address.row, address.col) ? current : patch;
}

function mergeDrawingsByScope(
  current: ReadOnlyRichProjection["drawings"],
  patch: ReadOnlyRichProjection["drawings"],
  isAffected: (drawing: DrawingProjection) => boolean
): ReadOnlyRichProjection["drawings"] {
  return [...(current ?? []).filter((drawing) => !isAffected(drawing)), ...(patch ?? [])];
}

function drawingRowScopeAffected(
  drawing: DrawingProjection,
  rowIndex: number
): boolean {
  return drawing.fromRow >= rowIndex || (drawing.toRow != null && drawing.toRow >= rowIndex);
}

function drawingColumnScopeAffected(
  drawing: DrawingProjection,
  colIndex: number
): boolean {
  return drawing.fromCol >= colIndex || (drawing.toCol != null && drawing.toCol >= colIndex);
}
