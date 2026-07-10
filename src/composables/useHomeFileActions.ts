import { computed, ref } from "vue";
import { defaultRichProjection, type FileData, type RecentFile } from "@/types";
import { useDocumentSessionStore } from "@/stores/documentSession";
import * as api from "@/api";
import { pickOpenFile, readRecentFile } from "@/platform";
import { blankCell } from "@/utils/cellValue";
import { warnRecentFileTrackingFailure } from "@/utils/recentFileTracking";
import { defaultSpreadsheetExtension } from "@/utils/spreadsheetFormats";
import { useDocumentLifecycle } from "@/composables/useDocumentLifecycle";
import { useDocumentReplacementGuard } from "@/composables/useDocumentReplacementGuard";
import { useOpenFileSelection } from "@/composables/useOpenFileSelection";
import { useRecentFileUpdates } from "@/composables/useRecentFileUpdates";

type UseHomeFileActionsOptions = {
  navigateToTable?: () => Promise<void> | void;
};

export function useHomeFileActions({
  navigateToTable,
}: UseHomeFileActionsOptions = {}) {
  const router = useRouter();
  const documentSessionStore = useDocumentSessionStore();
  const { beginDocumentReplacement } = useDocumentReplacementGuard();
  const { openSelectedFileOrDiscard } = useOpenFileSelection({
    beginDocumentReplacement,
  });
  const { runDocumentLifecycle } = useDocumentLifecycle();
  const { queueRecentFileEntryUpdate, refreshRecentFiles } = useRecentFileUpdates();
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
      const opened = await openSelectedFileOrDiscard(selection);
      if (!opened) return;

      queueRecentFileEntryUpdate(selection.originalPath);
      await navigateToTableRoute();
    });
  }

  async function handleNewFile() {
    await runHomeFileAction("Failed to create file", async () => {
      const replacement = await beginDocumentReplacement();
      if (!replacement) return;
      try {
        const defaultExtension = await defaultSpreadsheetExtension();
        const newFileData = newUntitledFileData(defaultExtension);
        const opened = await api.initFile(newFileData);
        replacement.commit();
        documentSessionStore.openDocumentResponse(opened, null);
        await navigateToTableRoute();
      } catch (error) {
        replacement.cancel();
        throw error;
      }
    });
  }

  async function handleOpenRecent(file: RecentFile) {
    await runHomeFileAction("Failed to open file", async () => {
      try {
        if (await openRecentPath(file)) {
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

  async function openRecentPath(file: RecentFile): Promise<boolean> {
    const replacement = await beginDocumentReplacement();
    if (!replacement) return false;
    try {
      const opened = await readRecentFile(file);
      replacement.commit();
      documentSessionStore.openDocumentResponse(opened, file.path);
      queueRecentFileEntryUpdate(file.originalPath);
      return true;
    } catch (error) {
      replacement.cancel();
      throw error;
    }
  }

  async function relocateAndOpenRecent(file: RecentFile): Promise<boolean> {
    const selection = await pickOpenFile();
    if (!selection) return false;
    const opened = await openSelectedFileOrDiscard(selection);
    if (!opened) return false;
    queueRecentFileEntryUpdate(selection.originalPath);

    if (file.path !== selection.path) {
      try {
        await api.removeRecentFile(file.id);
      } catch (error) {
        warnRecentFileTrackingFailure(error);
      }
    }
    void refreshRecentFiles();
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
  return String(error).includes("File not found:");
}

function newUntitledFileData(defaultExtension: string): FileData {
  return {
    path: "",
    fileName: `untitled.${defaultExtension}`,
    sheets: [
      {
        name: "Sheet1",
        rows: Array.from({ length: 5 }, () =>
          Array.from({ length: 5 }, () => blankCell())
        ),
        merges: [],
        rich: defaultRichProjection(),
      },
    ],
  };
}
