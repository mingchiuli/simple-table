import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ref } from "vue";
import { useGridResize } from "@/table-geometry/useGridResize";

type TestListener = (event: Record<string, unknown>) => void;

function createDocumentMock() {
  const listeners = new Map<string, Set<TestListener>>();
  const addEventListener = vi.fn((type: string, listener: TestListener) => {
    const existing = listeners.get(type) ?? new Set<TestListener>();
    existing.add(listener);
    listeners.set(type, existing);
  });
  const removeEventListener = vi.fn((type: string, listener: TestListener) => {
    listeners.get(type)?.delete(listener);
  });

  return {
    addEventListener,
    removeEventListener,
    dispatch(type: string, event: Record<string, unknown> = {}) {
      for (const listener of listeners.get(type) ?? []) {
        listener({ type, ...event });
      }
    },
    listenerCount(type: string) {
      return listeners.get(type)?.size ?? 0;
    },
  };
}

function mouseEvent(clientX: number, clientY: number = 0) {
  return {
    type: "mousedown",
    clientX,
    clientY,
    preventDefault: vi.fn(),
  } as unknown as MouseEvent;
}

function deferred<T = void>() {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

describe("useGridResize", () => {
  let warnSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    warnSpy = vi.spyOn(console, "warn").mockImplementation(() => undefined);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    warnSpy.mockRestore();
  });

  it("keeps resize listeners idempotent and resize sessions exclusive", () => {
    const documentMock = createDocumentMock();
    vi.stubGlobal("document", documentMock);
    let columnWidth = 120;
    let rowHeight = 72;
    const setColumnWidth = vi.fn((_colIndex: number, width: number) => {
      columnWidth = width;
    });
    const setRowHeight = vi.fn((_rowIndex: number, height: number) => {
      rowHeight = height;
    });
    const commitColumnWidth = vi.fn();
    const commitRowHeight = vi.fn();

    const resize = useGridResize({
      headerHeight: 50,
      minColumnWidth: 56,
      minRowHeight: 36,
      scrollLeft: ref(0),
      scrollTop: ref(0),
      getColumnWidth: () => columnWidth,
      getRowHeight: () => rowHeight,
      getColumnOffset: () => 0,
      getRowOffset: () => 0,
      setColumnWidth,
      setRowHeight,
      commitColumnWidth,
      commitRowHeight,
    });

    resize.startColumnResize(mouseEvent(100), 0, 120);
    documentMock.dispatch("mousemove", { clientX: 180, clientY: 0 });
    resize.startRowResize(mouseEvent(0, 200), 1, 122);

    expect(documentMock.addEventListener).toHaveBeenCalledTimes(5);
    expect(documentMock.listenerCount("mousemove")).toBe(1);
    expect(documentMock.listenerCount("touchmove")).toBe(1);
    expect(documentMock.listenerCount("touchcancel")).toBe(1);
    expect(resize.resizingColumn.value).toBeNull();
    expect(resize.resizingRow.value).toBe(1);
    expect(setColumnWidth).toHaveBeenCalledWith(0, 200);
    expect(commitColumnWidth).toHaveBeenCalledWith(0, 200);

    documentMock.dispatch("mousemove", { clientX: 180, clientY: 250 });

    expect(setRowHeight).toHaveBeenCalledWith(1, 122);

    documentMock.dispatch("mouseup");

    expect(commitColumnWidth).toHaveBeenCalledTimes(1);
    expect(commitRowHeight).toHaveBeenCalledWith(1, 122);
    expect(documentMock.removeEventListener).toHaveBeenCalledTimes(5);
    expect(documentMock.listenerCount("mousemove")).toBe(0);
    expect(documentMock.listenerCount("touchmove")).toBe(0);
    expect(documentMock.listenerCount("touchcancel")).toBe(0);
  });

  it("cleans up an active touch resize when the gesture is cancelled", () => {
    const documentMock = createDocumentMock();
    vi.stubGlobal("document", documentMock);
    let rowHeight = 72;
    const setRowHeight = vi.fn((_rowIndex: number, height: number) => {
      rowHeight = height;
    });
    const commitRowHeight = vi.fn();

    const resize = useGridResize({
      headerHeight: 50,
      minColumnWidth: 56,
      minRowHeight: 36,
      scrollLeft: ref(0),
      scrollTop: ref(0),
      getColumnWidth: () => 120,
      getRowHeight: () => rowHeight,
      getColumnOffset: () => 0,
      getRowOffset: () => 0,
      setColumnWidth: vi.fn(),
      setRowHeight,
      commitColumnWidth: vi.fn(),
      commitRowHeight,
    });

    resize.startRowResize(mouseEvent(0, 100), 2, 122);
    documentMock.dispatch("touchmove", {
      touches: [{ clientX: 0, clientY: 140 }],
      preventDefault: vi.fn(),
    });
    documentMock.dispatch("touchcancel");

    expect(setRowHeight).toHaveBeenCalledWith(2, 112);
    expect(commitRowHeight).toHaveBeenCalledWith(2, 112);
    expect(resize.resizingRow.value).toBeNull();
    expect(documentMock.listenerCount("touchmove")).toBe(0);
    expect(documentMock.listenerCount("touchcancel")).toBe(0);
  });

  it("does not commit a resize when the dimension did not change", () => {
    const documentMock = createDocumentMock();
    vi.stubGlobal("document", documentMock);
    const commitColumnWidth = vi.fn();
    const commitRowHeight = vi.fn();

    const resize = useGridResize({
      headerHeight: 50,
      minColumnWidth: 56,
      minRowHeight: 36,
      scrollLeft: ref(0),
      scrollTop: ref(0),
      getColumnWidth: () => 120,
      getRowHeight: () => 72,
      getColumnOffset: () => 0,
      getRowOffset: () => 0,
      setColumnWidth: vi.fn(),
      setRowHeight: vi.fn(),
      commitColumnWidth,
      commitRowHeight,
    });

    resize.startColumnResize(mouseEvent(100), 0, 120);
    documentMock.dispatch("mouseup");
    resize.startRowResize(mouseEvent(0, 100), 2, 122);
    documentMock.dispatch("mouseup");

    expect(commitColumnWidth).not.toHaveBeenCalled();
    expect(commitRowHeight).not.toHaveBeenCalled();
    expect(resize.resizingColumn.value).toBeNull();
    expect(resize.resizingRow.value).toBeNull();
  });

  it("rounds protocol dimensions and clamps them to the configured maximum", () => {
    const documentMock = createDocumentMock();
    vi.stubGlobal("document", documentMock);
    let columnWidth = 120;
    const commitColumnWidth = vi.fn();
    const resize = useGridResize({
      headerHeight: 50,
      minColumnWidth: 56,
      minRowHeight: 36,
      maxColumnWidth: 150,
      maxRowHeight: 200,
      scrollLeft: ref(0),
      scrollTop: ref(0),
      getColumnWidth: () => columnWidth,
      getRowHeight: () => 72,
      getColumnOffset: () => 0,
      getRowOffset: () => 0,
      setColumnWidth: vi.fn((_index: number, width: number) => {
        columnWidth = width;
      }),
      setRowHeight: vi.fn(),
      commitColumnWidth,
      commitRowHeight: vi.fn(),
    });

    resize.startColumnResize(mouseEvent(100.2), 0, 120);
    documentMock.dispatch("mousemove", { clientX: 111.1 });
    expect(columnWidth).toBe(131);
    documentMock.dispatch("mousemove", { clientX: 1_000.8 });
    documentMock.dispatch("mouseup");

    expect(commitColumnWidth).toHaveBeenCalledWith(0, 150);
  });

  it("clears preview state after a committed resize", () => {
    const documentMock = createDocumentMock();
    vi.stubGlobal("document", documentMock);
    let columnWidth = 120;
    const clearColumnWidth = vi.fn();
    const commitColumnWidth = vi.fn();

    const resize = useGridResize({
      headerHeight: 50,
      minColumnWidth: 56,
      minRowHeight: 36,
      scrollLeft: ref(0),
      scrollTop: ref(0),
      getColumnWidth: () => columnWidth,
      getRowHeight: () => 72,
      getColumnOffset: () => 0,
      getRowOffset: () => 0,
      setColumnWidth: vi.fn((_colIndex: number, width: number) => {
        columnWidth = width;
      }),
      setRowHeight: vi.fn(),
      clearColumnWidth,
      clearRowHeight: vi.fn(),
      commitColumnWidth,
      commitRowHeight: vi.fn(),
    });

    resize.startColumnResize(mouseEvent(100), 0, 120);
    documentMock.dispatch("mousemove", { clientX: 150 });
    documentMock.dispatch("mouseup");

    expect(commitColumnWidth).toHaveBeenCalledWith(0, 170);
    expect(clearColumnWidth).toHaveBeenCalledWith(0);
  });

  it("keeps preview state until an async resize commit settles", async () => {
    const documentMock = createDocumentMock();
    vi.stubGlobal("document", documentMock);
    const commit = deferred();
    let columnWidth = 120;
    const clearColumnWidth = vi.fn();
    const commitColumnWidth = vi.fn(() => commit.promise);

    const resize = useGridResize({
      headerHeight: 50,
      minColumnWidth: 56,
      minRowHeight: 36,
      scrollLeft: ref(0),
      scrollTop: ref(0),
      getColumnWidth: () => columnWidth,
      getRowHeight: () => 72,
      getColumnOffset: () => 0,
      getRowOffset: () => 0,
      setColumnWidth: vi.fn((_colIndex: number, width: number) => {
        columnWidth = width;
      }),
      setRowHeight: vi.fn(),
      clearColumnWidth,
      clearRowHeight: vi.fn(),
      commitColumnWidth,
      commitRowHeight: vi.fn(),
    });

    resize.startColumnResize(mouseEvent(100), 0, 120);
    documentMock.dispatch("mousemove", { clientX: 150 });
    documentMock.dispatch("mouseup");

    expect(commitColumnWidth).toHaveBeenCalledWith(0, 170);
    expect(clearColumnWidth).not.toHaveBeenCalled();
    expect(resize.resizingColumn.value).toBeNull();
    expect(documentMock.listenerCount("mousemove")).toBe(0);

    commit.resolve();
    await commit.promise;
    await Promise.resolve();

    expect(clearColumnWidth).toHaveBeenCalledWith(0);
  });

  it("cancels the active resize without committing when resizing becomes disabled", () => {
    const documentMock = createDocumentMock();
    vi.stubGlobal("document", documentMock);
    const canResize = ref(true);
    let rowHeight = 72;
    const clearRowHeight = vi.fn();
    const commitRowHeight = vi.fn();

    const resize = useGridResize({
      canResize,
      headerHeight: 50,
      minColumnWidth: 56,
      minRowHeight: 36,
      scrollLeft: ref(0),
      scrollTop: ref(0),
      getColumnWidth: () => 120,
      getRowHeight: () => rowHeight,
      getColumnOffset: () => 0,
      getRowOffset: () => 0,
      setColumnWidth: vi.fn(),
      setRowHeight: vi.fn((_rowIndex: number, height: number) => {
        rowHeight = height;
      }),
      clearColumnWidth: vi.fn(),
      clearRowHeight,
      commitColumnWidth: vi.fn(),
      commitRowHeight,
    });

    resize.startRowResize(mouseEvent(0, 100), 2, 122);
    documentMock.dispatch("mousemove", { clientX: 0, clientY: 130 });
    canResize.value = false;

    expect(commitRowHeight).not.toHaveBeenCalled();
    expect(clearRowHeight).toHaveBeenCalledWith(2);
    expect(resize.resizingRow.value).toBeNull();
    expect(documentMock.listenerCount("mousemove")).toBe(0);
  });
});
