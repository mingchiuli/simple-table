<script setup lang="ts">
defineProps<{
  modelValue: string;
}>();

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void;
  (e: 'blur'): void;
}>();
</script>

<template>
  <el-input
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
