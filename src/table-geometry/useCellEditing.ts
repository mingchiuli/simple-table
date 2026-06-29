type UseCellEditingOptions = {
  getCellKey: (rowIndex: number, colIndex: number) => string;
  getInitialValue: (rowIndex: number, colIndex: number) => string;
  emitEditing: (rowIndex: number, colIndex: number, value: string) => void;
  emitChange: (rowIndex: number, colIndex: number, value: string) => void;
  emitCancel: (rowIndex: number, colIndex: number) => void;
};

export function useCellEditing({
  getCellKey,
  getInitialValue,
  emitEditing,
  emitChange,
  emitCancel,
}: UseCellEditingOptions) {
  const editingValue = ref<Record<string, string>>({});
  const editingCell = ref<string | null>(null);
  const isManualClick = ref(false);

  function isEditing(rowIndex: number, colIndex: number): boolean {
    return editingCell.value === getCellKey(rowIndex, colIndex);
  }

  function beginEdit(rowIndex: number, colIndex: number) {
    const key = getCellKey(rowIndex, colIndex);
    editingCell.value = key;
    editingValue.value = {};
    editingValue.value[key] = getInitialValue(rowIndex, colIndex);
    isManualClick.value = true;
  }

  function resetEditing() {
    editingCell.value = null;
    editingValue.value = {};
    isManualClick.value = false;
  }

  function handleInput(rowIndex: number, colIndex: number, value: string) {
    editingValue.value[getCellKey(rowIndex, colIndex)] = value;
    emitEditing(rowIndex, colIndex, value);
  }

  function commit(rowIndex: number, colIndex: number, value: string) {
    delete editingValue.value[getCellKey(rowIndex, colIndex)];
    editingCell.value = null;
    emitChange(rowIndex, colIndex, value);
  }

  function cancel(rowIndex: number, colIndex: number) {
    delete editingValue.value[getCellKey(rowIndex, colIndex)];
    editingCell.value = null;
    emitCancel(rowIndex, colIndex);
  }

  function syncSelectedCell(newKey: string | null) {
    if (!newKey) {
      resetEditing();
      return;
    }
    if (editingCell.value === newKey) return;
    resetEditing();
  }

  return {
    editingValue,
    editingCell,
    isManualClick,
    isEditing,
    beginEdit,
    resetEditing,
    handleInput,
    commit,
    cancel,
    syncSelectedCell,
  };
}
