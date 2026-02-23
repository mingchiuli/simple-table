<script setup lang="ts">
import { computed } from "vue";

const props = defineProps<{
  modelValue: string;
  cellPosition: { row: number; col: number } | null;
}>();

const emit = defineEmits<{
  (e: "update:modelValue", value: string): void;
  (e: "submit"): void;
}>();

const displayPosition = computed(() => {
  if (!props.cellPosition) return "";
  const { row, col } = props.cellPosition;
  return String.fromCharCode(65 + col) + (row + 1);
});

function handleInput(value: string) {
  emit("update:modelValue", value);
}

function handleEnter() {
  emit("submit");
}
</script>

<template>
  <div v-if="cellPosition" class="cell-editor-bar">
    <span class="cell-position">{{ displayPosition }}</span>
    <el-input
      :model-value="modelValue"
      class="cell-editor-input"
      placeholder="Edit cell value..."
      @input="handleInput"
      @keydown.enter="handleEnter"
      @blur="handleEnter"
    />
  </div>
</template>

<style scoped>
.cell-editor-bar {
  display: flex;
  align-items: center;
  padding: 8px 16px;
  background: #f5f7fa;
  border-bottom: 1px solid #e4e7ed;
  gap: 12px;
}

.cell-position {
  font-weight: bold;
  color: #409eff;
  min-width: 40px;
  font-size: 14px;
}

.cell-editor-input {
  flex: 1;
  max-width: 500px;
}
</style>
