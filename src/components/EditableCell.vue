<script setup lang="ts">
import { nextTick, ref, watch } from 'vue';

const props = withDefaults(defineProps<{
  modelValue: string;
  autoFocus?: boolean;
}>(), {
  autoFocus: true
});

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void;
  (e: 'blur'): void;
}>();

const inputRef = ref<InstanceType<typeof import('element-plus').ElInput> | null>(null);

watch(() => props.autoFocus, (newVal) => {
  if (newVal) {
    nextTick(() => {
      inputRef.value?.focus();
    });
  }
});
</script>

<template>
  <el-input
    ref="inputRef"
    :model-value="modelValue"
    size="small"
    class="cell-input"
    @input="emit('update:modelValue', $event)"
    @blur="emit('blur')"
  />
</template>

<style scoped>
.cell-input {
  width: 100%;
}

:deep(.cell-input .el-input__wrapper) {
  box-shadow: none;
}

:deep(.cell-input .el-input__wrapper:hover) {
  box-shadow: 0 0 0 1px #409eff inset;
}
</style>
