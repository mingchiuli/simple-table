<script setup lang="ts">
import type { CellValue } from '@/types';
import { cellToDisplayString, isFormulaCell } from '@/composables/usePendingCellSave';

const props = withDefaults(defineProps<{
  value: CellValue | undefined;
  draftValue?: string;
  selected?: boolean;
  rowHeight?: number;
}>(), {
  selected: false,
  rowHeight: 72,
});

const displayValue = computed(() => {
  if (props.draftValue !== undefined) return props.draftValue;
  return cellToDisplayString(props.value);
});

const valueKind = computed(() => {
  if (props.value === null || props.value === undefined) return 'blank';
  if (isFormulaCell(props.value)) return props.value.error ? 'error' : 'formula';
  if (typeof props.value === 'number') return 'number';
  if (typeof props.value === 'boolean') return 'boolean';
  return 'text';
});

const minHeight = computed(() => `${Math.max(36, props.rowHeight)}px`);
</script>

<template>
  <div
    class="cell-view"
    :class="[`kind-${valueKind}`, { selected }]"
    :style="{ minHeight }"
  >
    <span v-if="displayValue" class="cell-content">{{ displayValue }}</span>
  </div>
</template>

<style scoped>
.cell-view {
  width: 100%;
  min-width: 0;
  display: flex;
  align-items: flex-start;
  padding: 6px 8px;
  overflow: hidden;
  cursor: cell;
  border: 1px solid transparent;
}

.cell-view.selected {
  border-color: var(--el-color-primary);
  box-shadow: inset 0 0 0 1px var(--el-color-primary);
}

.cell-content {
  min-width: 0;
  max-width: 100%;
  overflow: hidden;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  line-height: 1.35;
}

.kind-number {
  justify-content: flex-end;
}

.kind-boolean,
.kind-formula {
  color: var(--el-color-primary);
}

.kind-error {
  color: var(--el-color-danger);
}
</style>
