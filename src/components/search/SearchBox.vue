<script setup lang="ts">
import type { SearchScope } from '@/types';

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
      placeholder="Search cells..."
      :disabled="props.disabled"
      @keyup.enter="handleSearch"
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
