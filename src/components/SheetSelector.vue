<script setup lang="ts">
defineProps<{
  sheetNames: string[];
  currentSheetIndex: number;
}>();

const emit = defineEmits<{
  (e: 'sheet-change', value: number): void;
}>();
</script>

<template>
  <el-select
    :model-value="currentSheetIndex"
    @update:model-value="(val: number) => emit('sheet-change', val)"
    placeholder="Select sheet"
    style="width: 150px"
  >
    <el-option
      v-for="(name, index) in sheetNames"
      :key="index"
      :label="name"
      :value="index"
    />
  </el-select>
</template>
