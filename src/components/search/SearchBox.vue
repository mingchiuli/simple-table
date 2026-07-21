<script setup lang="ts">
import { Search } from '@element-plus/icons-vue';
import { MAX_SEARCH_QUERY_BYTES } from '@/protocol/editorResourcePolicy';
import type { SearchScope } from '@/types';
import { truncateUtf8 } from '@/utils/utf8';

const MAX_SEARCH_QUERY_CHARACTERS = MAX_SEARCH_QUERY_BYTES;

const props = defineProps<{
  isSearching: boolean;
  disabled?: boolean;
}>();

const emit = defineEmits<{
  (e: 'search', query: string, scope: SearchScope): void;
  (e: 'clear-search'): void;
}>();

const searchQuery = ref('');
const searchScope = ref<SearchScope>('currentSheet');

function handleSearch() {
  if (props.disabled) return;
  if (searchQuery.value.trim()) {
    emit('search', searchQuery.value, searchScope.value);
  }
}

function handleInput(value: string) {
  searchQuery.value = truncateUtf8(value, MAX_SEARCH_QUERY_BYTES);
}

function clearSearch() {
  if (props.disabled) return;
  searchQuery.value = '';
  emit('clear-search');
}
</script>

<template>
  <div class="search-box">
    <el-input
      v-model="searchQuery"
      :maxlength="MAX_SEARCH_QUERY_CHARACTERS"
      placeholder="Search cells..."
      :disabled="props.disabled"
      @keyup.enter="handleSearch"
      @input="handleInput"
      clearable
      @clear="clearSearch"
      class="search-input"
    >
      <template #prepend>
        <el-select v-model="searchScope" class="scope-select" :disabled="props.disabled">
          <el-option label="Current" value="currentSheet" />
          <el-option label="All" value="allSheets" />
        </el-select>
      </template>
      <template #append>
        <el-button :loading="props.isSearching" :disabled="props.disabled" @click="handleSearch">
          <el-icon><Search /></el-icon>
        </el-button>
      </template>
    </el-input>
  </div>
</template>

<style scoped>
.search-box {
  position: relative;
  width: 100%;
}

.search-input {
  width: 100%;
  min-width: 0;
}

.scope-select {
  width: auto;
  min-width: 80px;
}
</style>
