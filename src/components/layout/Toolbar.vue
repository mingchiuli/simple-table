<script setup lang="ts">
import type { FileData } from '@/types';
import { ref } from 'vue';
import { usePlatform } from '@/composables/usePlatform';
import { isMobile as isMobileOS } from '@/utils/platform';
import { Search, Refresh } from '@element-plus/icons-vue';
import { FileButtons } from '@/components/file';
import { SheetSelector, SheetButtons } from '@/components/sheet';
import { SearchBox } from '@/components/search';
import { EditButtons } from '@/components/edit';
import UpdateDialog from '@/components/UpdateDialog.vue';

const { isMobileOrTablet } = usePlatform();
const canExport = isMobileOS();
const searchPopoverVisible = ref(false);
const updateDialogRef = ref<InstanceType<typeof UpdateDialog> | null>(null);

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
  (e: 'export-file'): void;
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
function handleCheckUpdate() {
  updateDialogRef.value?.show();
}
</script>

<template>
  <!-- 桌面端工具栏 -->
  <header v-if="!isMobileOrTablet" class="toolbar desktop-toolbar">
    <div class="toolbar-left">
      <FileButtons
        :file-data="props.fileData"
        :show-export="canExport"
        @open-file="emit('open-file')"
        @save-file="emit('save-file')"
        @export-file="emit('export-file')"
      />
      <el-button
        size="small"
        :icon="Refresh"
        @click="handleCheckUpdate"
        title="Check for Updates"
      >
        Update
      </el-button>
    </div>

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
        :show-export="canExport"
        @open-file="emit('open-file')"
        @save-file="emit('save-file')"
        @export-file="emit('export-file')"
      />

      <div class="mobile-right">
        <el-button
          size="small"
          :icon="Refresh"
          @click="handleCheckUpdate"
          title="Check for Updates"
        />
        <SheetSelector
          v-if="props.fileData"
          :sheet-names="props.sheetNames"
          :current-sheet-index="props.currentSheetIndex"
          @sheet-change="emit('sheet-change', $event)"
        />
      </div>
    </div>

    <div class="mobile-toolbar-actions" v-if="props.fileData">
      <el-popover
        :visible="searchPopoverVisible"
        placement="bottom"
        :width="280"
        trigger="click"
        @update:visible="searchPopoverVisible = $event"
      >
        <template #reference>
          <el-button size="small" title="Search">
            <el-icon><Search /></el-icon>
          </el-button>
        </template>
        <SearchBox
          :is-searching="props.isSearching"
          @search="(query, scope) => { emit('search', query, scope); searchPopoverVisible = false; }"
          @clear-search="emit('clear-search')"
        />
      </el-popover>
      <el-button
        :disabled="!props.canUndo"
        @click="emit('undo')"
        size="small"
        title="Undo"
      >
        Undo
      </el-button>
      <el-button
        :disabled="!props.canRedo"
        @click="emit('redo')"
        size="small"
        title="Redo"
      >
        Redo
      </el-button>
      <el-button @click="emit('add-row')" size="small" title="Add Row">
        +R
      </el-button>
      <el-button @click="emit('add-column')" size="small" title="Add Column">
        +C
      </el-button>
      <el-button @click="emit('add-sheet')" size="small" title="Add Sheet">
        +S
      </el-button>
      <el-button
        :disabled="props.sheetNames.length <= 1"
        @click="emit('delete-sheet')"
        size="small"
        title="Delete Sheet"
      >
        -S
      </el-button>
    </div>
  </header>

  <UpdateDialog ref="updateDialogRef" />
</template>

<style scoped>
/* ==================== 桌面端工具栏 ==================== */
.desktop-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: var(--el-bg-color);
  border-bottom: 1px solid var(--el-border-color);
  overflow-x: auto;
  padding: 8px 20px;
  gap: 16px;
}

.toolbar-left {
  display: flex;
  align-items: center;
  gap: 8px;
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
  width: 100%;
  max-width: 100%;
  min-width: 0;
  padding: 8px 12px;
  gap: 8px;
  border-bottom: 1px solid var(--el-border-color);
  overflow: hidden;
}

.mobile-toolbar-row {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
  min-width: 0;
}

.mobile-toolbar :deep(.file-buttons) {
  flex: 1 1 260px;
  min-width: 0;
}

.mobile-toolbar :deep(.file-buttons .el-button) {
  flex: 1 1 88px;
  min-width: 0;
  margin-left: 0;
  padding-right: 10px;
  padding-left: 10px;
}

.mobile-toolbar :deep(.file-buttons .el-button > span) {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.mobile-right {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  flex: 1 1 180px;
  min-width: 0;
  gap: 8px;
}

.mobile-right > .el-button {
  flex: 0 0 40px;
  margin-left: 0;
}

.mobile-right :deep(.sheet-selector) {
  flex: 1 1 auto;
  width: auto;
  min-width: 120px;
  max-width: 180px;
}

.mobile-toolbar-actions {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(44px, 1fr));
  gap: 6px;
  min-width: 0;
}

.mobile-toolbar-actions :deep(.el-button) {
  width: 100%;
  min-width: 0;
  margin-left: 0;
  padding-right: 4px;
  padding-left: 4px;
}

.mobile-toolbar-actions :deep(.el-button > span) {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

@media (max-width: 480px) {
  .mobile-right {
    flex-basis: 100%;
  }

  .mobile-right :deep(.sheet-selector) {
    max-width: none;
  }
}
</style>
