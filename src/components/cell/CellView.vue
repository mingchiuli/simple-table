<script setup lang="ts">
import type { CellFormatProjection, CellStyleProjection, CellValue } from '@/types';
import { cellDisplayText, cellKind } from '@/utils/cellValue';

const props = withDefaults(defineProps<{
  value: CellValue | undefined;
  format?: CellFormatProjection;
  cellStyle?: CellStyleProjection;
  draftValue?: string;
  selected?: boolean;
}>(), {
  selected: false,
});

const displayValue = computed(() => {
  if (props.draftValue !== undefined) return props.draftValue;
  return cellDisplayText(props.value);
});

const valueKind = computed(() => {
  return cellKind(props.value);
});

const cellViewStyle = computed(() => ({
  height: '100%',
  color: normalizeColor(props.cellStyle?.fontColor),
  backgroundColor: normalizeColor(props.cellStyle?.backgroundColor),
  fontWeight: props.cellStyle?.bold ? "700" : undefined,
  fontStyle: props.cellStyle?.italic ? "italic" : undefined,
  justifyContent: horizontalAlign(props.cellStyle?.horizontalAlign),
  alignItems: verticalAlign(props.cellStyle?.verticalAlign),
}));

function normalizeColor(value: string | null | undefined): string | undefined {
  if (!value) return undefined;
  const hex = value.replace(/^#/, "");
  if (/^[0-9a-fA-F]{8}$/.test(hex)) {
    return `#${hex.slice(2)}`;
  }
  if (/^[0-9a-fA-F]{6}$/.test(hex)) {
    return `#${hex}`;
  }
  return undefined;
}

function horizontalAlign(value: string | null | undefined): string | undefined {
  const normalized = value?.toLowerCase();
  if (normalized?.includes("left")) return "flex-start";
  if (normalized?.includes("right")) return "flex-end";
  if (normalized?.includes("center")) return "center";
  return undefined;
}

function verticalAlign(value: string | null | undefined): string | undefined {
  const normalized = value?.toLowerCase();
  if (normalized?.includes("top")) return "flex-start";
  if (normalized?.includes("bottom")) return "flex-end";
  if (normalized?.includes("center")) return "center";
  return undefined;
}
</script>

<template>
  <div
    class="cell-view"
    :class="[`kind-${valueKind}`, { selected }]"
    :style="cellViewStyle"
    :title="format?.numberFormat ?? undefined"
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
