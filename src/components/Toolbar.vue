<script setup lang="ts">
import type { FileData } from '@/types';

const props = defineProps<{
  fileData: FileData | null;
  sheetNames: string[];
  currentSheetIndex: number;
  columnCount: number;
  canUndo: boolean;
  canRedo: boolean;
}>();

const emit = defineEmits<{
  (e: 'open-file'): void;
  (e: 'save-file'): void;
  (e: 'sheet-change', value: number): void;
  (e: 'add-row'): void;
  (e: 'add-column'): void;
  (e: 'delete-column'): void;
  (e: 'undo'): void;
  (e: 'redo'): void;
}>();
</script>

<template>
  <header class="toolbar">
    <div class="toolbar-left">
      <el-button type="primary" @click="emit('open-file')">
        Open File
      </el-button>
      <el-button
        type="success"
        @click="emit('save-file')"
        :disabled="!props.fileData"
      >
        Save
      </el-button>
    </div>
    <div class="toolbar-center" v-if="props.fileData">
      <el-select
        :model-value="props.currentSheetIndex"
        @update:model-value="(val: number) => emit('sheet-change', val)"
        placeholder="Select sheet"
        style="width: 150px"
      >
        <el-option
          v-for="(name, index) in props.sheetNames"
          :key="index"
          :label="name"
          :value="index"
        />
      </el-select>
    </div>
    <div class="toolbar-right" v-if="props.fileData">
      <el-button @click="emit('undo')" :disabled="!props.canUndo">
        Undo
      </el-button>
      <el-button @click="emit('redo')" :disabled="!props.canRedo">
        Redo
      </el-button>
      <el-button @click="emit('add-row')">+ Row</el-button>
      <el-button @click="emit('add-column')">+ Column</el-button>
      <el-button @click="emit('delete-column')" :disabled="props.columnCount <= 1">
        - Column
      </el-button>
    </div>
  </header>
</template>

<style scoped>
.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 20px;
  background: #fff;
  border-bottom: 1px solid #e4e7ed;
  gap: 16px;
  overflow-x: auto;
}

.toolbar-left,
.toolbar-right {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}

.toolbar-center {
  flex: 1;
  display: flex;
  justify-content: center;
  flex-shrink: 0;
}
</style>
