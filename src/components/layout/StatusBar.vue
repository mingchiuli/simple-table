<script setup lang="ts">
import type { FormulaStatus } from "@/types";

const props = defineProps<{
  fileName: string;
  hasChanges: boolean;
  formulaStatus: FormulaStatus;
}>();

function formulaWarningText(status: FormulaStatus): string | null {
  const diagnostics = status.diagnostics;
  if (status.state === 'degraded') {
    return status.message;
  }
  if (!diagnostics) return null;

  const warnings: string[] = [];
  if (diagnostics.invalidFormulaCount) {
    warnings.push(`${diagnostics.invalidFormulaCount} invalid`);
  }
  if (diagnostics.volatileFormulaCount) {
    warnings.push(`${diagnostics.volatileFormulaCount} volatile`);
  }
  if (diagnostics.unsupportedDependencyCount) {
    warnings.push(`${diagnostics.unsupportedDependencyCount} unsupported`);
  }
  if (diagnostics.largeRangeDependencyCount) {
    warnings.push(`${diagnostics.largeRangeDependencyCount} large range`);
  }
  return warnings.length ? `Formula warnings: ${warnings.join(', ')}` : null;
}

const formulaWarning = computed(() => formulaWarningText(props.formulaStatus));
</script>

<template>
  <footer class="statusbar">
    <span>{{ fileName }}</span>
    <span class="statusbar-right">
      <span
        v-if="formulaWarning"
        class="formula-warning"
        :title="formulaWarning"
      >
        {{ formulaStatus.state === 'degraded' ? 'Formula degraded' : 'Formula warnings' }}
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
