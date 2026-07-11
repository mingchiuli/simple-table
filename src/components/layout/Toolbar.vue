<script setup lang="ts">
import type { DocumentProjection, SearchScope } from '@/types';
import { usePlatform } from '@/composables/usePlatform';
import { isMobile as isMobileOS } from '@/utils/platform';
import {
  CirclePlus,
  Delete,
  Plus,
  Refresh,
  RefreshLeft,
  RefreshRight,
  Search,
} from '@element-plus/icons-vue';
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
  fileData: DocumentProjection | null;
  sheetNames: string[];
  currentSheetIndex: number;
  canUndo: boolean;
  canRedo: boolean;
  capabilities: {
    canInsertDeleteRows: boolean;
    canInsertDeleteColumns: boolean;
    canInsertDeleteSheets: boolean;
  };
  isBusy: boolean;
  isEditorLocked: boolean;
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
  (e: 'search', query: string, scope: SearchScope): void;
  (e: 'clear-search'): void;
}>();
function handleCheckUpdate() {
  updateDialogRef.value?.show();
}
</script>

<template>
  <!-- 桌面端工具栏 -->
  <header v-if="!isMobileOrTablet" class="toolbar desktop-toolbar">
    <el-scrollbar
      class="desktop-toolbar-scrollbar"
      view-class="desktop-toolbar-content"
    >
      <div class="toolbar-left">
        <FileButtons
          :file-data="props.fileData"
          :show-export="canExport"
          :disabled="props.isBusy"
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
          :disabled="props.isEditorLocked"
          @sheet-change="emit('sheet-change', $event)"
        />

        <SheetButtons
          class="sheet-buttons"
          :sheet-count="props.sheetNames.length"
          :can-insert-delete-sheets="props.capabilities.canInsertDeleteSheets"
          :disabled="props.isEditorLocked"
          @add-sheet="emit('add-sheet')"
          @delete-sheet="emit('delete-sheet')"
        />

        <SearchBox
          class="search-box"
          :is-searching="props.isSearching"
          :disabled="props.isEditorLocked"
          @search="(query: string, scope: SearchScope) => emit('search', query, scope)"
          @clear-search="emit('clear-search')"
        />
      </div>

      <EditButtons
        v-if="props.fileData"
        :can-undo="props.canUndo"
        :can-redo="props.canRedo"
        :can-insert-delete-rows="props.capabilities.canInsertDeleteRows"
        :can-insert-delete-columns="props.capabilities.canInsertDeleteColumns"
        :disabled="props.isEditorLocked"
        @undo="emit('undo')"
        @redo="emit('redo')"
        @add-row="emit('add-row')"
        @add-column="emit('add-column')"
      />
    </el-scrollbar>
  </header>

  <!-- 移动端工具栏 -->
  <header v-else class="toolbar mobile-toolbar">
    <div class="mobile-toolbar-row">
      <FileButtons
        :file-data="props.fileData"
        :show-export="canExport"
        :disabled="props.isBusy"
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
          :disabled="props.isEditorLocked"
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
          <el-button size="small" title="Search" :disabled="props.isEditorLocked">
            <el-icon><Search /></el-icon>
          </el-button>
        </template>
        <SearchBox
          :is-searching="props.isSearching"
          :disabled="props.isEditorLocked"
          @search="(query, scope) => { emit('search', query, scope); searchPopoverVisible = false; }"
          @clear-search="emit('clear-search')"
        />
      </el-popover>
      <el-button
        :disabled="props.isEditorLocked || !props.canUndo"
        @click="emit('undo')"
        size="small"
        title="Undo"
      >
        <el-icon><RefreshLeft /></el-icon>
      </el-button>
      <el-button
        :disabled="props.isEditorLocked || !props.canRedo"
        @click="emit('redo')"
        size="small"
        title="Redo"
      >
        <el-icon><RefreshRight /></el-icon>
      </el-button>
      <el-button
        :disabled="props.isEditorLocked || !props.capabilities.canInsertDeleteRows"
        @click="emit('add-row')"
        size="small"
        title="Add Row"
      >
        <el-icon><Plus /></el-icon>
      </el-button>
      <el-button
        :disabled="props.isEditorLocked || !props.capabilities.canInsertDeleteColumns"
        @click="emit('add-column')"
        size="small"
        title="Add Column"
      >
        <el-icon><Plus /></el-icon>
      </el-button>
      <el-button
        :disabled="props.isEditorLocked || !props.capabilities.canInsertDeleteSheets"
        @click="emit('add-sheet')"
        size="small"
        title="Add Sheet"
      >
        <el-icon><CirclePlus /></el-icon>
      </el-button>
      <el-button
        :disabled="props.isEditorLocked || props.sheetNames.length <= 1 || !props.capabilities.canInsertDeleteSheets"
        @click="emit('delete-sheet')"
        size="small"
        title="Delete Sheet"
      >
        <el-icon><Delete /></el-icon>
      </el-button>
    </div>
  </header>

  <UpdateDialog ref="updateDialogRef" />
</template>

<style scoped>
/* ==================== 桌面端工具栏 ==================== */
.desktop-toolbar {
  background: var(--el-bg-color);
  border-bottom: 1px solid var(--el-border-color);
  overflow: hidden;
}

.desktop-toolbar-scrollbar {
  width: 100%;
}

.desktop-toolbar-scrollbar :deep(.el-scrollbar__wrap) {
  overflow-y: hidden;
}

.desktop-toolbar-scrollbar :deep(.desktop-toolbar-content) {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: max-content;
  min-width: 100%;
  padding: 8px 20px 12px;
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
  flex: 0 0 auto;
  padding: 6px 8px;
  gap: 6px;
  border-bottom: 1px solid var(--el-border-color);
  overflow: hidden;
  background: var(--el-bg-color);
}

.mobile-toolbar-row {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: nowrap;
  min-width: 0;
}

.mobile-toolbar :deep(.file-buttons) {
  flex: 1 1 auto;
  min-width: 0;
  gap: 4px;
}

.mobile-toolbar :deep(.file-buttons .el-button) {
  flex: 1 1 0;
  min-width: 0;
  margin-left: 0;
  padding-right: 6px;
  padding-left: 6px;
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
  flex: 0 1 190px;
  min-width: 0;
  gap: 6px;
}

.mobile-right > .el-button {
  flex: 0 0 40px;
  margin-left: 0;
}

.mobile-right :deep(.sheet-selector) {
  flex: 1 1 auto;
  width: auto;
  min-width: 96px;
  max-width: 160px;
}

.mobile-toolbar-actions {
  display: grid;
  grid-template-columns: repeat(7, minmax(36px, 1fr));
  gap: 6px;
  min-width: 0;
}

.mobile-toolbar-actions :deep(.el-button) {
  width: 100%;
  height: 34px;
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
  .mobile-toolbar {
    padding: 5px 6px;
    gap: 5px;
  }

  .mobile-toolbar-row {
    align-items: stretch;
  }

  .mobile-right {
    flex: 0 0 auto;
  }

  .mobile-right :deep(.sheet-selector) {
    width: 104px;
    min-width: 104px;
    max-width: 104px;
  }

  .mobile-toolbar :deep(.file-buttons .el-button) {
    height: 34px;
    padding-right: 4px;
    padding-left: 4px;
  }

  .mobile-toolbar-actions {
    grid-template-columns: repeat(7, minmax(34px, 1fr));
    gap: 4px;
  }
}
</style>
