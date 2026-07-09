import type { ComputedRef, Ref } from "vue";

type MaybeReadonlyRef<T> = Ref<T> | ComputedRef<T>;

type UseGridResizeOptions = {
  canResize?: MaybeReadonlyRef<boolean>;
  headerHeight: number;
  minColumnWidth: number;
  minRowHeight: number;
  scrollLeft: Ref<number>;
  scrollTop: Ref<number>;
  getColumnWidth: (colIndex: number) => number;
  getRowHeight: (rowIndex: number) => number;
  getColumnOffset: (colIndex: number) => number;
  getRowOffset: (rowIndex: number) => number;
  setColumnWidth: (colIndex: number, width: number) => void;
  setRowHeight: (rowIndex: number, height: number) => void;
  clearColumnWidth?: (colIndex: number) => void;
  clearRowHeight?: (rowIndex: number) => void;
  commitColumnWidth: (colIndex: number, width: number) => void;
  commitRowHeight: (rowIndex: number, height: number) => void;
};

export function useGridResize({
  canResize,
  headerHeight,
  minColumnWidth,
  minRowHeight,
  scrollLeft,
  scrollTop,
  getColumnWidth,
  getRowHeight,
  getColumnOffset,
  getRowOffset,
  setColumnWidth,
  setRowHeight,
  clearColumnWidth,
  clearRowHeight,
  commitColumnWidth,
  commitRowHeight,
}: UseGridResizeOptions) {
  const resizingColumn = ref<number | null>(null);
  const resizingRow = ref<number | null>(null);
  const startX = ref(0);
  const startY = ref(0);
  const startWidth = ref(0);
  const startHeight = ref(0);
  const resizeLineX = ref(0);
  const resizeLineY = ref(0);
  let listenersAttached = false;

  function startColumnResize(event: MouseEvent | TouchEvent, colIndex: number, boundaryX: number) {
    if (canResize?.value === false) return;
    event.preventDefault();
    finishActiveResize();
    resizingColumn.value = colIndex;
    startX.value = getClientX(event);
    startWidth.value = getColumnWidth(colIndex);
    resizeLineX.value = boundaryX;
    addDocumentListeners();
  }

  function startRowResize(event: MouseEvent | TouchEvent, rowIndex: number, boundaryY?: number) {
    if (canResize?.value === false) return;
    event.preventDefault();
    finishActiveResize();
    resizingRow.value = rowIndex;
    startY.value = getClientY(event);
    startHeight.value = getRowHeight(rowIndex);
    resizeLineY.value = boundaryY ?? headerHeight + getRowOffset(rowIndex) + startHeight.value - scrollTop.value;
    addDocumentListeners();
  }

  function onResize(event: MouseEvent | TouchEvent) {
    if (resizingColumn.value === null && resizingRow.value === null) return;

    if (event.type === "touchmove") {
      event.preventDefault();
    }

    if (resizingColumn.value !== null) {
      const delta = getClientX(event) - startX.value;
      const nextWidth = Math.max(minColumnWidth, startWidth.value + delta);
      setColumnWidth(resizingColumn.value, nextWidth);
      resizeLineX.value = getColumnOffset(resizingColumn.value) + nextWidth - scrollLeft.value;
    }

    if (resizingRow.value !== null) {
      const delta = getClientY(event) - startY.value;
      const nextHeight = Math.max(minRowHeight, startHeight.value + delta);
      setRowHeight(resizingRow.value, nextHeight);
      resizeLineY.value = headerHeight + getRowOffset(resizingRow.value) + nextHeight - scrollTop.value;
    }
  }

  function stopResize() {
    finishActiveResize();
    removeDocumentListeners();
  }

  function finishActiveResize() {
    if (resizingColumn.value !== null) {
      const colIndex = resizingColumn.value;
      const width = getColumnWidth(colIndex);
      if (width !== startWidth.value) {
        commitColumnWidth(colIndex, width);
      }
      clearColumnWidth?.(colIndex);
    }

    if (resizingRow.value !== null) {
      const rowIndex = resizingRow.value;
      const height = getRowHeight(rowIndex);
      if (height !== startHeight.value) {
        commitRowHeight(rowIndex, height);
      }
      clearRowHeight?.(rowIndex);
    }

    resizingColumn.value = null;
    resizingRow.value = null;
    resizeLineX.value = 0;
    resizeLineY.value = 0;
  }

  function cancelActiveResize() {
    if (resizingColumn.value !== null) {
      clearColumnWidth?.(resizingColumn.value);
    }
    if (resizingRow.value !== null) {
      clearRowHeight?.(resizingRow.value);
    }
    resizingColumn.value = null;
    resizingRow.value = null;
    resizeLineX.value = 0;
    resizeLineY.value = 0;
    removeDocumentListeners();
  }

  function addDocumentListeners() {
    if (listenersAttached) {
      return;
    }
    listenersAttached = true;
    document.addEventListener("mousemove", onResize);
    document.addEventListener("mouseup", stopResize);
    document.addEventListener("touchmove", onResize, { passive: false });
    document.addEventListener("touchend", stopResize);
    document.addEventListener("touchcancel", stopResize);
  }

  function removeDocumentListeners() {
    if (!listenersAttached) {
      return;
    }
    listenersAttached = false;
    document.removeEventListener("mousemove", onResize);
    document.removeEventListener("mouseup", stopResize);
    document.removeEventListener("touchmove", onResize);
    document.removeEventListener("touchend", stopResize);
    document.removeEventListener("touchcancel", stopResize);
  }

  if (canResize) {
    watch(canResize, (allowed) => {
      if (!allowed) {
        cancelActiveResize();
      }
    }, { flush: "sync" });
  }

  onUnmounted(cancelActiveResize);

  return {
    resizingColumn,
    resizingRow,
    resizeLineX,
    resizeLineY,
    startColumnResize,
    startRowResize,
  };
}

function getClientX(event: MouseEvent | TouchEvent): number {
  if ("clientX" in event) return event.clientX;
  if (event.touches && event.touches.length > 0) return event.touches[0].clientX;
  if (event.changedTouches && event.changedTouches.length > 0) return event.changedTouches[0].clientX;
  return 0;
}

function getClientY(event: MouseEvent | TouchEvent): number {
  if ("clientY" in event) return event.clientY;
  if (event.touches && event.touches.length > 0) return event.touches[0].clientY;
  if (event.changedTouches && event.changedTouches.length > 0) return event.changedTouches[0].clientY;
  return 0;
}
