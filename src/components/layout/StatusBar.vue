<script setup lang="ts">
import type { FormulaStatus } from "@/types";

defineProps<{
  fileName: string;
  hasChanges: boolean;
  formulaStatus: FormulaStatus;
}>();
</script>

<template>
  <footer class="statusbar">
    <span>{{ fileName }}</span>
    <span class="statusbar-right">
      <span
        v-if="formulaStatus.state === 'degraded'"
        class="formula-warning"
        :title="formulaStatus.message"
      >
        Formula degraded
      </span>
      <span v-if="hasChanges" class="unsaved">Unsaved changes</span>
    </span>
  </footer>
</template>

<style scoped>
.statusbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 20px;
  background: var(--el-bg-color-page);
  border-top: 1px solid var(--el-border-color);
  font-size: 12px;
  color: var(--el-text-color-regular);
}

.unsaved {
  color: var(--el-color-warning);
}

.statusbar-right {
  display: inline-flex;
  align-items: center;
  gap: 12px;
}

.formula-warning {
  color: var(--el-color-danger);
}
</style>
