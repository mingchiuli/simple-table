export type GridItem = {
  index: number;
  top: number;
  height: number;
};

export type ColumnResizeHandle = {
  colIndex: number;
  left: number;
};

export type RowResizeHandle = {
  rowIndex: number;
  top: number;
};

export function buildOffsets(count: number, sizeAt: (index: number) => number): number[] {
  const offsets = [0];
  for (let index = 0; index < count; index += 1) {
    offsets.push(offsets[index] + sizeAt(index));
  }
  return offsets;
}

export function collectVisibleItems(
  offsets: number[],
  count: number,
  scrollStart: number,
  viewportSize: number,
  overscanPx: number
): GridItem[] {
  if (count <= 0) return [];

  const start = Math.max(0, scrollStart - overscanPx);
  const end = scrollStart + viewportSize + overscanPx;
  const firstIndex = findFirstVisibleIndex(offsets, start);
  const items: GridItem[] = [];

  for (let index = firstIndex; index < count; index += 1) {
    const top = offsets[index] ?? 0;
    const nextTop = offsets[index + 1] ?? top;
    if (top > end) break;
    items.push({ index, top, height: nextTop - top });
  }

  return items;
}

export function findFirstVisibleIndex(offsets: number[], start: number): number {
  let low = 0;
  let high = Math.max(0, offsets.length - 2);
  let result = 0;

  while (low <= high) {
    const mid = Math.floor((low + high) / 2);
    if ((offsets[mid + 1] ?? 0) < start) {
      low = mid + 1;
    } else {
      result = mid;
      high = mid - 1;
    }
  }

  return result;
}

export function offsetAt(offsets: number[], index: number, fallback: number): number {
  const clamped = Math.max(0, Math.min(index, offsets.length - 1));
  return offsets[clamped] ?? fallback;
}

export function spanSize(
  offsets: number[],
  startIndex: number,
  endIndex: number,
  fallback: number
): number {
  const start = offsetAt(offsets, startIndex, fallback);
  const end = offsetAt(offsets, endIndex + 1, fallback);
  return Math.max(0, end - start);
}

export function collectColumnResizeHandles(
  columnCount: number,
  rowHeaderWidth: number,
  scrollLeft: number,
  tableWidth: number,
  widthAt: (index: number) => number
): ColumnResizeHandle[] {
  const handles: ColumnResizeHandle[] = [];
  let boundary = rowHeaderWidth - scrollLeft;

  for (let colIndex = 0; colIndex < columnCount; colIndex += 1) {
    boundary += widthAt(colIndex);
    if (boundary >= rowHeaderWidth && boundary <= tableWidth) {
      handles.push({ colIndex, left: boundary });
    }
    if (boundary > tableWidth) break;
  }

  return handles;
}

export function collectRowResizeHandles(
  rowCount: number,
  headerHeight: number,
  scrollTop: number,
  tableHeight: number,
  heightAt: (index: number) => number
): RowResizeHandle[] {
  const handles: RowResizeHandle[] = [];
  let boundary = headerHeight - scrollTop;

  for (let rowIndex = 0; rowIndex < rowCount; rowIndex += 1) {
    boundary += heightAt(rowIndex);
    if (boundary >= headerHeight && boundary <= tableHeight) {
      handles.push({ rowIndex, top: boundary });
    }
    if (boundary > tableHeight) break;
  }

  return handles;
}

export function areNumberRecordsEqual(
  current: Record<number, number>,
  next: Record<number, number>
): boolean {
  const currentKeys = Object.keys(current);
  const nextKeys = Object.keys(next);
  if (currentKeys.length !== nextKeys.length) return false;
  return currentKeys.every((key) => current[Number(key)] === next[Number(key)]);
}
