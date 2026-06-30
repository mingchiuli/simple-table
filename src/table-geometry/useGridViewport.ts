type CellPosition = { row: number; col: number };

export function useGridViewport() {
  const containerRef = ref<HTMLElement | null>(null);
  const scrollViewportRef = ref<HTMLElement | null>(null);
  const tableSize = ref({ width: 800, height: 600 });
  const scrollLeft = ref(0);
  const scrollTop = ref(0);
  let resizeObserver: ResizeObserver | null = null;

  function setContainerRef(element: unknown) {
    containerRef.value = element instanceof HTMLElement ? element : null;
  }

  function setScrollViewportRef(element: unknown) {
    scrollViewportRef.value = element instanceof HTMLElement ? element : null;
  }

  function handleViewportScroll() {
    const viewport = scrollViewportRef.value;
    if (!viewport) return;
    scrollLeft.value = viewport.scrollLeft;
    scrollTop.value = viewport.scrollTop;
  }

  function scrollCellIntoView(
    cell: CellPosition | null | undefined,
    enabled: boolean | undefined,
    geometry: {
      getRowOffset: (rowIndex: number) => number;
      getColumnOffset: (colIndex: number) => number;
      getRowHeight: (rowIndex: number) => number;
      getColumnWidth: (colIndex: number) => number;
      viewportWidth: number;
      viewportHeight: number;
    }
  ) {
    if (!enabled || !cell || !scrollViewportRef.value) return;
    const targetTop = geometry.getRowOffset(cell.row)
      - geometry.viewportHeight / 2
      + geometry.getRowHeight(cell.row) / 2;
    const targetLeft = geometry.getColumnOffset(cell.col)
      - geometry.viewportWidth / 2
      + geometry.getColumnWidth(cell.col) / 2;
    scrollViewportRef.value.scrollTo({
      top: Math.max(0, targetTop),
      left: Math.max(0, targetLeft),
    });
  }

  onMounted(() => {
    if (!containerRef.value) return;

    const updateSize = () => {
      tableSize.value = {
        width: containerRef.value!.clientWidth,
        height: containerRef.value!.clientHeight,
      };
    };

    updateSize();
    resizeObserver = new ResizeObserver(updateSize);
    resizeObserver.observe(containerRef.value);
  });

  onUnmounted(() => {
    resizeObserver?.disconnect();
  });

  return {
    containerRef,
    scrollViewportRef,
    setContainerRef,
    setScrollViewportRef,
    tableSize,
    scrollLeft,
    scrollTop,
    handleViewportScroll,
    scrollCellIntoView,
  };
}
