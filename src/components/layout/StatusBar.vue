<script setup lang="ts">
import type { FormulaStatus, HistoryStatus } from "@/types";

const props = defineProps<{
  fileName: string;
  hasChanges: boolean;
  formulaStatus: FormulaStatus;
  historyStatus: HistoryStatus;
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
  if (diagnostics.skippedReferenceRewriteCount) {
    warnings.push(`${diagnostics.skippedReferenceRewriteCount} unshifted ref`);
  }
  const issueDetails = (diagnostics.issues ?? [])
    .slice(0, 5)
    .map((issue) => `S${issue.sheetIndex + 1}!${issue.row + 1}:${issue.col + 1} ${issue.kind}`);
  const detailText = issueDetails.length ? ` (${issueDetails.join('; ')})` : '';
  return warnings.length ? `Formula warnings: ${warnings.join(', ')}${detailText}` : null;
}

const formulaWarning = computed(() => formulaWarningText(props.formulaStatus));

const historyWarning = computed(() => {
  const status = props.historyStatus;
  if (!status.isTruncated) return null;
  const reason = status.reason ? `${status.reason}. ` : '';
  return `${reason}Undo entries: ${status.undoEntries}, redo entries: ${status.redoEntries}`;
});
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
      <span
        v-if="historyWarning"
        class="history-warning"
        :title="historyWarning"
      >
        Undo history limited
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

.history-warning {
  color: var(--el-color-warning);
}

@media (max-width: 640px), (pointer: coarse) {
  .statusbar {
    padding: 6px 10px max(6px, env(safe-area-inset-bottom));
    gap: 8px;
  }

  .statusbar > span:first-child {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .statusbar-right {
    flex-shrink: 0;
    gap: 8px;
  }
}
</style>
