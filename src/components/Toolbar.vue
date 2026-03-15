<script setup lang="ts">
import type { FileData } from '@/types';
import { usePlatform } from '@/composables/usePlatform';
import FileButtons from './FileButtons.vue';
import SheetSelector from './SheetSelector.vue';
import SheetButtons from './SheetButtons.vue';
import SearchBox from './SearchBox.vue';
import EditButtons from './EditButtons.vue';

const { isMobile } = usePlatform();

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
  <header class="toolbar">
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
        v-if="!isMobile"
        class="search-box"
        :is-searching="props.isSearching"
        @search="(query: string, scope: Function) => emit('search', query, scope)"
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
</template>

<style scoped>
.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: #fff;
  border-bottom: 1px solid #e4e7ed;
  overflow-x: auto;
}

.toolbar-center {
  flex: 1;
  display: flex;
  justify-content: center;
  align-items: center;
  flex-shrink: 0;
}
</style>
