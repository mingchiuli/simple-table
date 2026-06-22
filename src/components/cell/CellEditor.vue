<script setup lang="ts">
import { Close } from '@element-plus/icons-vue';
import { toCellPosition } from '@/utils/excel';

const modelValue = defineModel<string>({ required: true });

const props = defineProps<{
  cellPosition: { row: number; col: number } | null;
}>();

const emit = defineEmits<{
  (e: "submit"): void;
  (e: "close"): void;
}>();

const displayPosition = computed(() => {
  if (!props.cellPosition) return "";
  return toCellPosition(props.cellPosition.row, props.cellPosition.col);
});

function handleEnter() {
  emit("submit");
}

function handleClose() {
  emit("close");
}
</script>

<template>
  <div v-if="cellPosition" class="cell-editor-bar">
    <span class="cell-position">{{ displayPosition }}</span>
    <el-input
      v-model="modelValue"
      type="textarea"
      :autosize="{ minRows: 2, maxRows: 4 }"
      class="cell-editor-input"
      placeholder="Edit cell value..."
      @keydown.enter="handleEnter"
      @blur="handleEnter"
    />
    <el-button
      class="close-btn"
      circle
      size="small"
      @click="handleClose"
    >
      <el-icon><Close /></el-icon>
    </el-button>
  </div>
</template>

<style scoped>
.cell-editor-bar {
  display: flex;
  align-items: center;
  padding: 8px 16px;
  background: var(--el-bg-color-page);
  border-bottom: 1px solid var(--el-border-color);
  gap: 12px;
  overflow: visible;
}

.cell-position {
  font-weight: bold;
  color: var(--el-color-primary);
  min-width: 40px;
  font-size: 14px;
}

.cell-editor-input {
  flex: 1;
  max-width: min(100vw - 80px, 500px);
  font-size: 16px;
}

.close-btn {
  margin-left: 8px;
}
</style>
