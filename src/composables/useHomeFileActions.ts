import { computed } from "vue";
import { ElMessage } from "element-plus";
import type { RecentFile } from "@/types";
import { useDocumentSessionStore } from "@/stores/documentSession";
import { pickOpenFile } from "@/platform";
import { useRecentFileUpdates } from "@/composables/useRecentFileUpdates";
import { appErrorMessage, isAppErrorCode } from "@/utils/appError";
import { useDocumentFileCoordinator } from "@/composables/useDocumentFileCoordinator";

type UseHomeFileActionsOptions = {
  navigateToTable?: () => Promise<void> | void;
};

export function useHomeFileActions({
  navigateToTable,
}: UseHomeFileActionsOptions = {}) {
  const router = useRouter();
  const documentSessionStore = useDocumentSessionStore();
  const fileCoordinator = useDocumentFileCoordinator();
  const {
    refreshRecentFiles,
    removeRecentFile,
  } = useRecentFileUpdates();
  const isBusy = computed(() => documentSessionStore.isInteractionLocked);

  async function handleOpenFile() {
    try {
      if (await fileCoordinator.openPickedFile()) {
        await navigateToTableRoute();
      }
    } catch (error) {
      ElMessage.error(`Failed to open file: ${appErrorMessage(error)}`);
    }
  }

  async function handleNewFile() {
    try {
      if (await fileCoordinator.createNewDocument()) {
        await navigateToTableRoute();
      }
    } catch (error) {
      ElMessage.error(`Failed to create file: ${appErrorMessage(error)}`);
    }
  }

  async function handleOpenRecent(file: RecentFile) {
    try {
      if (await fileCoordinator.openRecentDocument(file)) {
        await navigateToTableRoute();
        return;
      }
    } catch (error) {
      if (!isFileNotFoundError(error)) {
        ElMessage.error(`Failed to open file: ${appErrorMessage(error)}`);
        return;
      }
    }

    try {
      if (await relocateAndOpenRecent(file)) {
        await navigateToTableRoute();
      }
    } catch (error) {
      ElMessage.error(`Failed to open file: ${appErrorMessage(error)}`);
    }
  }

  async function relocateAndOpenRecent(file: RecentFile): Promise<boolean> {
    const selection = await pickOpenFile();
    if (!selection) return false;
    const opened = await fileCoordinator.openSelectedFile(selection);
    if (!opened) return false;

    if (file.path !== selection.path) {
      try {
        await removeRecentFile(file.id);
      } catch (error) {
        console.warn("Failed to update recent file metadata", error);
      }
    }
    return true;
  }

  async function navigateToTableRoute() {
    if (navigateToTable) {
      await navigateToTable();
      return;
    }
    await router.push({ name: "table" });
  }

  return {
    isBusy,
    refreshRecentFiles,
    handleOpenFile,
    handleNewFile,
    handleOpenRecent,
  };
}

function isFileNotFoundError(error: unknown): boolean {
  return isAppErrorCode(error, "file_not_found") || String(error).includes("File not found:");
}
