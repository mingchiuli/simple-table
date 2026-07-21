import type { ComputedRef } from 'vue';
import { ElMessage } from 'element-plus';

import { useDocumentFileCoordinator } from '@/composables/useDocumentFileCoordinator';
import type { DocumentProjection } from '@/types/documentRuntime';
import { appErrorMessage } from '@/utils/appError';

type UseFileActionsOptions = {
  fileData: ComputedRef<DocumentProjection | null>;
  flushPendingCellChanges: () => Promise<boolean>;
};

export function useFileActions({
  fileData,
  flushPendingCellChanges,
}: UseFileActionsOptions) {
  const router = useRouter();
  const fileCoordinator = useDocumentFileCoordinator({
    fileData,
    flushPendingCellChanges,
  });

  async function handleOpenFile() {
    await fileCoordinator.openPickedFile();
  }

  async function handleSaveFile() {
    const outcome = await fileCoordinator.saveCurrentFile();
    if (outcome.status === 'saved') {
      ElMessage.success('File saved successfully');
    } else if (outcome.status === 'saved-stale') {
      ElMessage.warning(
        'File was saved, but the active document changed before the editor could refresh.'
      );
    } else if (outcome.status === 'blocked') {
      ElMessage.error(outcome.message);
    }
  }

  async function handleExportFile() {
    if (await fileCoordinator.exportCurrentFile() === 'exported') {
      ElMessage.success('File exported successfully');
    }
  }

  async function handleBack() {
    try {
      await router.push({ name: 'home' });
    } catch (error) {
      ElMessage.error(`Failed to return home: ${appErrorMessage(error)}`);
    }
  }

  return {
    loadFileFromPath: fileCoordinator.loadFileFromPath,
    handleOpenFile,
    handleSaveFile,
    handleExportFile,
    closeCurrentDocument: fileCoordinator.closeCurrentDocument,
    confirmApplicationExit: fileCoordinator.confirmApplicationExit,
    handleBack,
  };
}
