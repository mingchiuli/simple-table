import type { ImageAnchor, ImageMarker } from '@/types/documentRuntime';

export const EMU_PER_PIXEL = 9525;

export type ImageRect = {
  left: number;
  top: number;
  width: number;
  height: number;
};

export type ImageGridGeometry = {
  getColumnOffset: (colIndex: number) => number;
  getRowOffset: (rowIndex: number) => number;
  getColumnIndexAt: (left: number) => number;
  getRowIndexAt: (top: number) => number;
};

export function imageAnchorRect(
  anchor: ImageAnchor,
  geometry: Pick<ImageGridGeometry, 'getColumnOffset' | 'getRowOffset'>,
): ImageRect {
  const from = markerPoint(anchor.data.from, geometry);
  if (anchor.type === 'OneCell') {
    return {
      ...from,
      width: Math.max(1, anchor.data.widthEmu / EMU_PER_PIXEL),
      height: Math.max(1, anchor.data.heightEmu / EMU_PER_PIXEL),
    };
  }
  const to = markerPoint(anchor.data.to, geometry);
  return {
    ...from,
    width: Math.max(1, to.left - from.left),
    height: Math.max(1, to.top - from.top),
  };
}

export function imageAnchorForRect(rect: ImageRect, geometry: ImageGridGeometry): ImageAnchor {
  const col = geometry.getColumnIndexAt(rect.left);
  const row = geometry.getRowIndexAt(rect.top);
  const colOffset = Math.max(0, rect.left - geometry.getColumnOffset(col));
  const rowOffset = Math.max(0, rect.top - geometry.getRowOffset(row));
  return {
    type: 'OneCell',
    data: {
      from: {
        row,
        col,
        rowOffsetEmu: Math.round(rowOffset * EMU_PER_PIXEL),
        colOffsetEmu: Math.round(colOffset * EMU_PER_PIXEL),
      },
      widthEmu: Math.max(1, Math.round(rect.width * EMU_PER_PIXEL)),
      heightEmu: Math.max(1, Math.round(rect.height * EMU_PER_PIXEL)),
    },
  };
}

export function resizeImageRect(
  initial: ImageRect,
  deltaX: number,
  deltaY: number,
  minimumLongestSide = 24,
): ImageRect {
  const minimumScale = minimumLongestSide / Math.max(initial.width, initial.height);
  const widthScale = (initial.width + deltaX) / initial.width;
  const heightScale = (initial.height + deltaY) / initial.height;
  const scale = Math.max(minimumScale, widthScale, heightScale);
  return {
    ...initial,
    width: initial.width * scale,
    height: initial.height * scale,
  };
}

function markerPoint(
  marker: ImageMarker,
  geometry: Pick<ImageGridGeometry, 'getColumnOffset' | 'getRowOffset'>,
) {
  return {
    left: geometry.getColumnOffset(marker.col) + marker.colOffsetEmu / EMU_PER_PIXEL,
    top: geometry.getRowOffset(marker.row) + marker.rowOffsetEmu / EMU_PER_PIXEL,
  };
}
