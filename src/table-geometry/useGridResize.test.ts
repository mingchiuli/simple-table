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
    const setColumnWidth = vi.fn();
    const setRowHeight = vi.fn();
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
      setColumnWidth,
      setRowHeight,
      commitColumnWidth,
      commitRowHeight,
    });

    resize.startColumnResize(mouseEvent(100), 0, 120);
    resize.startRowResize(mouseEvent(0, 200), 1, 122);

    expect(documentMock.addEventListener).toHaveBeenCalledTimes(4);
    expect(documentMock.listenerCount("mousemove")).toBe(1);
    expect(documentMock.listenerCount("touchmove")).toBe(1);
    expect(resize.resizingColumn.value).toBeNull();
    expect(resize.resizingRow.value).toBe(1);

    documentMock.dispatch("mousemove", { clientX: 180, clientY: 250 });

    expect(setColumnWidth).not.toHaveBeenCalled();
    expect(setRowHeight).toHaveBeenCalledWith(1, 122);

    documentMock.dispatch("mouseup");

    expect(commitColumnWidth).not.toHaveBeenCalled();
    expect(commitRowHeight).toHaveBeenCalledWith(1, 72);
    expect(documentMock.removeEventListener).toHaveBeenCalledTimes(4);
    expect(documentMock.listenerCount("mousemove")).toBe(0);
    expect(documentMock.listenerCount("touchmove")).toBe(0);
  });
});
