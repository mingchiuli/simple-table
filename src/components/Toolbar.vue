<script setup lang="ts">
import type { FileData } from '@/types';
import { usePlatform } from '@/composables/usePlatform';
import FileButtons from './FileButtons.vue';
import SheetSelector from './SheetSelector.vue';
import SheetButtons from './SheetButtons.vue';
import SearchBox from './SearchBox.vue';
import EditButtons from './EditButtons.vue';

const { isMobileOrTablet } = usePlatform();

const props = defineProps<{
  fileData: FileData | null;
  sheetNames: string[];
  currentSheetIndex: number;
  canUndo: boolean;
  canRedo: boolean;
  isSearching: boolean;
}>();

const emit = defineEmits<{
  (e: 'open-file'): void;
  (e: 'save-file'): void;
  (e: 'sheet-change', value: number): void;
  (e: 'add-sheet'): void;
  (e: 'delete-sheet'): void;
  (e: 'add-row'): void;
  (e: 'add-column'): void;
  (e: 'undo'): void;
  (e: 'redo'): void;
  (e: 'search', query: string, scope: 'currentSheet' | 'allSheets'): void;
  (e: 'clear-search'): void;
}>();
</script>

<template>
  <!-- 桌面端工具栏 -->
  <header v-if="!isMobileOrTablet" class="toolbar desktop-toolbar">
    <FileButtons
      :file-data="props.fileData"
      @open-file="emit('open-file')"
      @save-file="emit('save-file')"
    />

    <div class="toolbar-center" v-if="props.fileData">
      <SheetSelector
        :sheet-names="props.sheetNames"
        :current-sheet-index="props.currentSheetIndex"
        @sheet-change="emit('sheet-change', $event)"
      />

      <SheetButtons
        class="sheet-buttons"
        :sheet-count="props.sheetNames.length"
        @add-sheet="emit('add-sheet')"
        @delete-sheet="emit('delete-sheet')"
      />

      <SearchBox
        class="search-box"
        :is-searching="props.isSearching"
        @search="(query: string, scope: 'currentSheet' | 'allSheets') => emit('search', query, scope)"
        @clear-search="emit('clear-search')"
      />
    </div>

    <EditButtons
      v-if="props.fileData"
      :can-undo="props.canUndo"
      :can-redo="props.canRedo"
      @undo="emit('undo')"
      @redo="emit('redo')"
      @add-row="emit('add-row')"
      @add-column="emit('add-column')"
    />
  </header>

  <!-- 移动端工具栏 -->
  <header v-else class="toolbar mobile-toolbar">
    <div class="mobile-toolbar-row">
      <FileButtons
        :file-data="props.fileData"
        @open-file="emit('open-file')"
        @save-file="emit('save-file')"
      />

      <SheetSelector
        v-if="props.fileData"
        :sheet-names="props.sheetNames"
        :current-sheet-index="props.currentSheetIndex"
        @sheet-change="emit('sheet-change', $event)"
      />
    </div>

    <div class="mobile-toolbar-actions" v-if="props.fileData">
      <el-button
        :disabled="!props.canUndo"
        @click="emit('undo')"
        size="small"
        circle
        title="Undo"
      >
        ↶
      </el-button>
      <el-button
        :disabled="!props.canRedo"
        @click="emit('redo')"
        size="small"
        circle
        title="Redo"
      >
        ↷
      </el-button>
      <el-button @click="emit('add-row')" size="small" circle title="Add Row">
        +R
      </el-button>
      <el-button @click="emit('add-column')" size="small" circle title="Add Column">
        +C
      </el-button>
      <el-button @click="emit('add-sheet')" size="small" circle title="Add Sheet">
        +S
      </el-button>
      <el-button
        :disabled="props.sheetNames.length <= 1"
        @click="emit('delete-sheet')"
        size="small"
        circle
        title="Delete Sheet"
      >
        -S
      </el-button>
    </div>
  </header>
</template>

<style scoped>
/* ==================== 桌面端工具栏 ==================== */
.desktop-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: #fff;
  border-bottom: 1px solid #e4e7ed;
  overflow-x: auto;
  padding: 8px 20px;
  gap: 16px;
}

.desktop-toolbar .toolbar-center {
  flex: 1;
  display: flex;
  justify-content: center;
  align-items: center;
  gap: 16px;
  flex-shrink: 0;
}

/* ==================== 移动端工具栏 ==================== */
.mobile-toolbar {
  display: flex;
  flex-direction: column;
  padding: 8px 12px;
  padding-top: max(8px, env(safe-area-inset-top));
  gap: 8px;
  border-bottom: 1px solid #e4e7ed;
}

.mobile-toolbar-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.mobile-toolbar-actions {
  display: flex;
  justify-content: center;
  gap: 4px;
  flex-wrap: wrap;
}
</style>
