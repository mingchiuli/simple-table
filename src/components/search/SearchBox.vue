<script setup lang="ts">
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
      class="search-input"
    >
      <template #prepend>
        <el-select v-model="searchScope" class="scope-select">
          <el-option label="Current" value="currentSheet" />
          <el-option label="All" value="allSheets" />
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
