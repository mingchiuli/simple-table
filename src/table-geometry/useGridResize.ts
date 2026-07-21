import type { ComputedRef, Ref } from "vue";

type MaybeReadonlyRef<T> = Ref<T> | ComputedRef<T>;

type UseGridResizeOptions = {
  canResize?: MaybeReadonlyRef<boolean>;
  headerHeight: number;
  minColumnWidth: number;
  minRowHeight: number;
  maxColumnWidth?: number;
  maxRowHeight?: number;
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
  commitColumnWidth: (colIndex: number, width: number) => void | Promise<void>;
  commitRowHeight: (rowIndex: number, height: number) => void | Promise<void>;
};

export function useGridResize({
  canResize,
  headerHeight,
  minColumnWidth,
  minRowHeight,
  maxColumnWidth = Number.MAX_SAFE_INTEGER,
  maxRowHeight = Number.MAX_SAFE_INTEGER,
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
  let resizeSessionId = 0;
  const columnPreviewSessions = new Map<number, number>();
  const rowPreviewSessions = new Map<number, number>();

  function startColumnResize(event: MouseEvent | TouchEvent, colIndex: number, boundaryX: number) {
    if (canResize?.value === false) return;
    event.preventDefault();
    finishActiveResize();
    const sessionId = nextResizeSessionId();
    resizingColumn.value = colIndex;
    startX.value = getClientX(event);
    startWidth.value = getColumnWidth(colIndex);
    resizeLineX.value = boundaryX;
    columnPreviewSessions.set(colIndex, sessionId);
    addDocumentListeners();
  }

  function startRowResize(event: MouseEvent | TouchEvent, rowIndex: number, boundaryY?: number) {
    if (canResize?.value === false) return;
    event.preventDefault();
    finishActiveResize();
    const sessionId = nextResizeSessionId();
    resizingRow.value = rowIndex;
    startY.value = getClientY(event);
    startHeight.value = getRowHeight(rowIndex);
    resizeLineY.value = boundaryY ?? headerHeight + getRowOffset(rowIndex) + startHeight.value - scrollTop.value;
    rowPreviewSessions.set(rowIndex, sessionId);
    addDocumentListeners();
  }

  function onResize(event: MouseEvent | TouchEvent) {
    if (resizingColumn.value === null && resizingRow.value === null) return;

    if (event.type === "touchmove") {
      event.preventDefault();
    }

    if (resizingColumn.value !== null) {
      const delta = getClientX(event) - startX.value;
      const nextWidth = clampPixelSize(startWidth.value + delta, minColumnWidth, maxColumnWidth);
      setColumnWidth(resizingColumn.value, nextWidth);
      resizeLineX.value = getColumnOffset(resizingColumn.value) + nextWidth - scrollLeft.value;
    }

    if (resizingRow.value !== null) {
      const delta = getClientY(event) - startY.value;
      const nextHeight = clampPixelSize(startHeight.value + delta, minRowHeight, maxRowHeight);
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
        const sessionId = columnPreviewSessions.get(colIndex) ?? nextResizeSessionId();
        commitColumnPreview(colIndex, width, sessionId);
      } else {
        clearColumnPreview(colIndex);
      }
    }

    if (resizingRow.value !== null) {
      const rowIndex = resizingRow.value;
      const height = getRowHeight(rowIndex);
      if (height !== startHeight.value) {
        const sessionId = rowPreviewSessions.get(rowIndex) ?? nextResizeSessionId();
        commitRowPreview(rowIndex, height, sessionId);
      } else {
        clearRowPreview(rowIndex);
      }
    }

    resizingColumn.value = null;
    resizingRow.value = null;
    resizeLineX.value = 0;
    resizeLineY.value = 0;
  }

  function cancelActiveResize() {
    if (resizingColumn.value !== null) {
      clearColumnPreview(resizingColumn.value);
    }
    if (resizingRow.value !== null) {
      clearRowPreview(resizingRow.value);
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

  function nextResizeSessionId(): number {
    resizeSessionId += 1;
    return resizeSessionId;
  }

  function commitColumnPreview(colIndex: number, width: number, sessionId: number) {
    try {
      const result = commitColumnWidth(colIndex, width);
      if (isPromiseLike(result)) {
        result
          .catch((error) => console.error("Column resize commit failed:", error))
          .finally(() => clearColumnPreviewIfCurrent(colIndex, sessionId));
        return;
      }
    } catch (error) {
      console.error("Column resize commit failed:", error);
    }
    clearColumnPreviewIfCurrent(colIndex, sessionId);
  }

  function commitRowPreview(rowIndex: number, height: number, sessionId: number) {
    try {
      const result = commitRowHeight(rowIndex, height);
      if (isPromiseLike(result)) {
        result
          .catch((error) => console.error("Row resize commit failed:", error))
          .finally(() => clearRowPreviewIfCurrent(rowIndex, sessionId));
        return;
      }
    } catch (error) {
      console.error("Row resize commit failed:", error);
    }
    clearRowPreviewIfCurrent(rowIndex, sessionId);
  }

  function clearColumnPreview(colIndex: number) {
    columnPreviewSessions.delete(colIndex);
    clearColumnWidth?.(colIndex);
  }

  function clearRowPreview(rowIndex: number) {
    rowPreviewSessions.delete(rowIndex);
    clearRowHeight?.(rowIndex);
  }

  function clearColumnPreviewIfCurrent(colIndex: number, sessionId: number) {
    if (columnPreviewSessions.get(colIndex) !== sessionId) return;
    clearColumnPreview(colIndex);
  }

  function clearRowPreviewIfCurrent(rowIndex: number, sessionId: number) {
    if (rowPreviewSessions.get(rowIndex) !== sessionId) return;
    clearRowPreview(rowIndex);
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

function isPromiseLike(value: unknown): value is Promise<void> {
  return !!value && typeof (value as { then?: unknown }).then === "function";
}

function clampPixelSize(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, Math.round(value)));
}
