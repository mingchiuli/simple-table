<script setup lang="ts">
defineProps<{
  sheetNames: string[];
  currentSheetIndex: number;
  disabled?: boolean;
}>();

const emit = defineEmits<{
  (e: 'sheet-change', value: number): void;
}>();
</script>

<template>
  <el-select
    :model-value="currentSheetIndex"
    :disabled="disabled"
    @update:model-value="(val: number) => emit('sheet-change', val)"
    placeholder="Select sheet"
    class="sheet-selector"
  >
    <el-option
      v-for="(name, index) in sheetNames"
      :key="index"
      :label="name"
      :value="index"
    />
  </el-select>
</template>

<style scoped>
.sheet-selector {
  width: 150px;
}
</style>
