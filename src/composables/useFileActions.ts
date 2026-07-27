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
    try {
      await fileCoordinator.openPickedFile();
    } catch (error) {
      ElMessage.error(`Failed to open file: ${appErrorMessage(error)}`);
    }
  }

  async function handleSaveFile() {
    try {
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
    } catch (error) {
      ElMessage.error(`Failed to save file: ${appErrorMessage(error)}`);
    }
  }

  async function handleExportFile() {
    try {
      if (await fileCoordinator.exportCurrentFile() === 'exported') {
        ElMessage.success('File exported successfully');
      }
    } catch (error) {
      ElMessage.error(`Failed to export file: ${appErrorMessage(error)}`);
    }
  }

  async function closeCurrentDocument(
    options: Parameters<typeof fileCoordinator.closeCurrentDocument>[0] = {},
  ): Promise<boolean> {
    try {
      return await fileCoordinator.closeCurrentDocument(options);
    } catch (error) {
      ElMessage.error(`Failed to close file: ${appErrorMessage(error)}`);
      return false;
    }
  }

  async function prepareApplicationExit(
    options: Parameters<typeof fileCoordinator.prepareApplicationExit>[0] = {},
  ) {
    try {
      return await fileCoordinator.prepareApplicationExit(options);
    } catch (error) {
      ElMessage.error(`Failed to prepare application exit: ${appErrorMessage(error)}`);
      return null;
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
    closeCurrentDocument,
    prepareApplicationExit,
    handleBack,
  };
}
