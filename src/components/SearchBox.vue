<script setup lang="ts">
import { ref } from 'vue';

defineProps<{
  isSearching: boolean;
}>();

const emit = defineEmits<{
  (e: 'search', query: string, scope: 'currentSheet' | 'allSheets'): void;
  (e: 'clear-search'): void;
}>();

const searchQuery = ref('');
const searchScope = ref<'currentSheet' | 'allSheets'>('currentSheet');

function handleSearch() {
  if (searchQuery.value.trim()) {
    emit('search', searchQuery.value, searchScope.value);
  }
}

function clearSearch() {
  searchQuery.value = '';
  emit('clear-search');
}
</script>

<template>
  <div class="search-box">
    <el-input
      v-model="searchQuery"
      placeholder="Search cells..."
      @keyup.enter="handleSearch"
      clearable
      @clear="clearSearch"
      style="width: 300px"
    >
      <template #prepend>
        <el-select v-model="searchScope" style="width: 120px">
          <el-option label="Current Sheet" value="currentSheet" />
          <el-option label="All Sheets" value="allSheets" />
        </el-select>
      </template>
      <template #append>
        <el-button :loading="isSearching" @click="handleSearch">
          <el-icon><Search /></el-icon>
        </el-button>
      </template>
    </el-input>
  </div>
</template>

<style scoped>
.search-box {
  position: relative;
}
</style>
