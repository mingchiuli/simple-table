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

export type AxisSizeOverrides = Readonly<Record<number, number>>;

export class SparseAxisGeometry {
  readonly count: number;
  readonly defaultSize: number;
  private readonly minimumSize: number;
  private readonly overrideIndexes: number[];
  private readonly overrideSizes: number[];
  private readonly prefixDeltas: number[];

  constructor(
    count: number,
    defaultSize: number,
    overrides: AxisSizeOverrides = {},
    minimumSize = 0
  ) {
    this.count = Math.max(0, Math.trunc(count));
    this.minimumSize = Math.max(0, minimumSize);
    this.defaultSize = this.normalizeSize(defaultSize);
    const entries = Object.entries(overrides)
      .map(([index, size]) => [Number(index), size] as const)
      .filter(([index, size]) =>
        Number.isInteger(index)
        && index >= 0
        && index < this.count
        && Number.isFinite(size)
        && size >= 0
      )
      .map(([index, size]) => [index, this.normalizeSize(size)] as const)
      .filter(([, size]) => size !== this.defaultSize)
      .sort(([left], [right]) => left - right);
    this.overrideIndexes = entries.map(([index]) => index);
    this.overrideSizes = entries.map(([, size]) => size);
    this.prefixDeltas = [0];
    for (const size of this.overrideSizes) {
      this.prefixDeltas.push(
        this.prefixDeltas[this.prefixDeltas.length - 1] + size - this.defaultSize
      );
    }
  }

  sizeAt(index: number, transient: AxisSizeOverrides = {}): number {
    const transientSize = transient[index];
    if (transientSize !== undefined) return this.normalizeSize(transientSize);
    const position = this.overridePosition(index);
    return position >= 0 ? this.overrideSizes[position] : this.defaultSize;
  }

  offsetAt(index: number, transient: AxisSizeOverrides = {}): number {
    const clamped = Math.max(0, Math.min(Math.trunc(index), this.count));
    const overrideCount = lowerBound(this.overrideIndexes, clamped);
    return clamped * this.defaultSize
      + this.prefixDeltas[overrideCount]
      + transientDeltaBefore(this, transient, clamped);
  }

  totalSize(transient: AxisSizeOverrides = {}): number {
    return this.offsetAt(this.count, transient);
  }

  indexAt(offset: number, transient: AxisSizeOverrides = {}): number {
    if (this.count <= 0) return 0;
    const target = Math.max(0, offset);
    let low = 0;
    let high = this.count;
    while (low < high) {
      const middle = Math.floor((low + high) / 2);
      if (this.offsetAt(middle + 1, transient) < target) {
        low = middle + 1;
      } else {
        high = middle;
      }
    }
    return Math.min(low, this.count - 1);
  }

  private overridePosition(index: number): number {
    const position = lowerBound(this.overrideIndexes, index);
    return this.overrideIndexes[position] === index ? position : -1;
  }

  private normalizeSize(size: number): number {
    return Math.max(this.minimumSize, size);
  }
}

export function collectVisibleItems(
  geometry: SparseAxisGeometry,
  scrollStart: number,
  viewportSize: number,
  overscanPx: number,
  transient: AxisSizeOverrides = {}
): GridItem[] {
  if (geometry.count <= 0) return [];
  const start = Math.max(0, scrollStart - overscanPx);
  const end = scrollStart + viewportSize + overscanPx;
  const firstIndex = geometry.indexAt(start, transient);
  const items: GridItem[] = [];
  for (let index = firstIndex; index < geometry.count; index += 1) {
    const top = geometry.offsetAt(index, transient);
    if (top > end) break;
    items.push({ index, top, height: geometry.sizeAt(index, transient) });
  }
  return items;
}

export function collectColumnResizeHandles(
  geometry: SparseAxisGeometry,
  rowHeaderWidth: number,
  scrollLeft: number,
  tableWidth: number,
  transient: AxisSizeOverrides = {}
): ColumnResizeHandle[] {
  return collectVisibleItems(
    geometry,
    scrollLeft,
    Math.max(0, tableWidth - rowHeaderWidth),
    0,
    transient
  ).map((item) => ({
    colIndex: item.index,
    left: rowHeaderWidth + item.top + item.height - scrollLeft,
  })).filter((handle) => handle.left >= rowHeaderWidth && handle.left <= tableWidth);
}

export function collectRowResizeHandles(
  geometry: SparseAxisGeometry,
  headerHeight: number,
  scrollTop: number,
  tableHeight: number,
  transient: AxisSizeOverrides = {}
): RowResizeHandle[] {
  return collectVisibleItems(
    geometry,
    scrollTop,
    Math.max(0, tableHeight - headerHeight),
    0,
    transient
  ).map((item) => ({
    rowIndex: item.index,
    top: headerHeight + item.top + item.height - scrollTop,
  })).filter((handle) => handle.top >= headerHeight && handle.top <= tableHeight);
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

function transientDeltaBefore(
  geometry: SparseAxisGeometry,
  transient: AxisSizeOverrides,
  index: number
): number {
  let delta = 0;
  for (const entry of Object.keys(transient)) {
    const overrideIndex = Number(entry);
    if (Number.isInteger(overrideIndex) && overrideIndex >= 0 && overrideIndex < index) {
      delta += geometry.sizeAt(overrideIndex, transient) - geometry.sizeAt(overrideIndex);
    }
  }
  return delta;
}

function lowerBound(values: number[], target: number): number {
  let low = 0;
  let high = values.length;
  while (low < high) {
    const middle = Math.floor((low + high) / 2);
    if (values[middle] < target) low = middle + 1;
    else high = middle;
  }
  return low;
}
