<script setup lang="ts">
import { Check, Close } from '@element-plus/icons-vue';
import { toCellPosition } from '@/utils/excel';

const modelValue = defineModel<string>({ required: true });

const props = defineProps<{
  cellPosition: { row: number; col: number } | null;
}>();

const emit = defineEmits<{
  (e: 'submit'): void;
  (e: 'close'): void;
}>();

const displayPosition = computed(() => {
  if (!props.cellPosition) return '';
  return toCellPosition(props.cellPosition.row, props.cellPosition.col);
});

const isFormula = computed(() => modelValue.value.trimStart().startsWith('='));

function handleKeydown(event: KeyboardEvent) {
  if (event.key === 'Enter' && !event.altKey && !event.shiftKey && !event.metaKey && !event.ctrlKey) {
    event.preventDefault();
    emit('submit');
    return;
  }

  if (event.key === 'Escape') {
    event.preventDefault();
    emit('close');
  }
}
</script>

<template>
  <div v-if="cellPosition" class="formula-bar">
    <div class="name-box">{{ displayPosition }}</div>
    <div class="formula-token" :class="{ active: isFormula }">fx</div>
    <textarea
      v-model="modelValue"
      class="formula-input"
      spellcheck="false"
      placeholder="Value or formula"
      @keydown="handleKeydown"
      @blur="emit('submit')"
    />
    <button class="formula-action" type="button" title="Apply" @click="emit('submit')">
      <el-icon><Check /></el-icon>
    </button>
    <button class="formula-action" type="button" title="Close" @click="emit('close')">
      <el-icon><Close /></el-icon>
    </button>
  </div>
</template>

<style scoped>
.formula-bar {
  display: grid;
  grid-template-columns: 72px 36px minmax(0, 1fr) 32px 32px;
  align-items: stretch;
  gap: 8px;
  padding: 8px 12px;
  background: var(--el-bg-color-page);
  border-bottom: 1px solid var(--el-border-color);
  min-height: 52px;
}

.name-box,
.formula-token,
.formula-action {
  display: flex;
  align-items: center;
  justify-content: center;
  border: 1px solid var(--el-border-color);
  background: var(--el-bg-color);
  color: var(--el-text-color-regular);
  font-size: 13px;
}

.name-box {
  font-weight: 600;
  color: var(--el-color-primary);
}

.formula-token {
  font-family: Georgia, serif;
  font-style: italic;
}

.formula-token.active {
  color: var(--el-color-primary);
  border-color: var(--el-color-primary-light-5);
}

.formula-input {
  width: 100%;
  min-height: 34px;
  max-height: 160px;
  resize: vertical;
  border: 1px solid var(--el-border-color);
  background: var(--el-bg-color);
  color: var(--el-text-color-primary);
  font: 14px/1.45 ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", monospace;
  padding: 7px 9px;
  outline: none;
  white-space: pre-wrap;
}

.formula-input:focus {
  border-color: var(--el-color-primary);
}

.formula-action {
  padding: 0;
  cursor: pointer;
}

.formula-action:hover {
  color: var(--el-color-primary);
  border-color: var(--el-color-primary-light-5);
}

@media (max-width: 640px), (pointer: coarse) {
  .formula-bar {
    grid-template-columns: 56px 32px minmax(0, 1fr) 36px 36px;
    gap: 6px;
    padding: 6px;
    min-height: 50px;
  }

  .name-box,
  .formula-token,
  .formula-action {
    min-height: 36px;
  }

  .formula-input {
    min-height: 36px;
    font-size: 16px;
    resize: none;
  }
}

@media (max-width: 420px) {
  .formula-bar {
    grid-template-columns: 56px 32px minmax(0, 1fr);
  }

  .formula-action {
    display: none;
  }
}
</style>
