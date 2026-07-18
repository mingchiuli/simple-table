import { computed, ref } from "vue";
import type { RecentFile } from "@/types";
import { useDocumentSessionStore } from "@/stores/documentSession";
import { pickOpenFile } from "@/platform";
import { useDocumentLifecycle } from "@/composables/useDocumentLifecycle";
import { useRecentFileUpdates } from "@/composables/useRecentFileUpdates";
import { isAppErrorCode } from "@/utils/appError";
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
  const { runDocumentLifecycle } = useDocumentLifecycle();
  const {
    refreshRecentFiles,
    removeRecentFile,
  } = useRecentFileUpdates();
  const isHomeActionBusy = ref(false);
  const isBusy = computed(
    () => isHomeActionBusy.value || documentSessionStore.isInteractionLocked
  );

  async function runHomeFileAction(errorPrefix: string, action: () => Promise<void>) {
    if (isBusy.value) return;
    isHomeActionBusy.value = true;
    try {
      await runDocumentLifecycle("loading", errorPrefix, action);
    } finally {
      isHomeActionBusy.value = false;
    }
  }

  async function handleOpenFile() {
    await runHomeFileAction("Failed to open file", async () => {
      const selection = await pickOpenFile();
      if (!selection) return;
      const opened = await fileCoordinator.openSelectedFile(selection);
      if (!opened) return;
      await navigateToTableRoute();
    });
  }

  async function handleNewFile() {
    await runHomeFileAction("Failed to create file", async () => {
      if (await fileCoordinator.createNewDocument()) {
        await navigateToTableRoute();
      }
    });
  }

  async function handleOpenRecent(file: RecentFile) {
    await runHomeFileAction("Failed to open file", async () => {
      try {
        if (await fileCoordinator.openRecentDocument(file)) {
          await navigateToTableRoute();
          return;
        }
      } catch (error) {
        if (!isFileNotFoundError(error)) {
          throw error;
        }
      }

      if (await relocateAndOpenRecent(file)) {
        await navigateToTableRoute();
        return;
      }
    });
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
