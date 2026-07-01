<script setup lang="ts">
import type { CellValue } from '@/types';
import { cellKind, cellToDisplayString } from '@/utils/cellValue';

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
  return cellKind(props.value);
});

const height = computed(() => `${Math.max(36, props.rowHeight)}px`);
</script>

<template>
  <div
    class="cell-view"
    :class="[`kind-${valueKind}`, { selected }]"
    :style="{ height }"
  >
    <span v-if="displayValue" class="cell-content">{{ displayValue }}</span>
  </div>
</template>

<style scoped>
.cell-view {
  width: 100%;
  height: 100%;
  min-width: 0;
  box-sizing: border-box;
  display: flex;
  align-items: center;
  justify-content: center;
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
  text-align: center;
}

.kind-boolean,
.kind-formula {
  color: var(--el-color-primary);
}

.kind-error {
  color: var(--el-color-danger);
}

@media (pointer: coarse) {
  .cell-view {
    padding: 8px;
  }
}
</style>
