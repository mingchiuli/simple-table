import { ElMessage } from 'element-plus';
import * as api from '@/api';
import {
  useDocumentSessionStore,
  type MutationApplyResult,
} from '@/stores/documentSession';
import type { EditorCommandContext, EditorMutationResponse, U64String } from '@/types';
import { appErrorMessage } from '@/utils/appError';

type DocumentCommandBusOptions = {
  applyMutationResponse: (response: EditorMutationResponse) => Promise<MutationApplyResult>;
};

type InteractiveMutationOptions = {
  action: (context: EditorCommandContext) => Promise<EditorMutationResponse>;
  flushPendingChanges: () => Promise<boolean>;
  errorMessage: string;
  refreshProjectionOnError?: boolean;
  afterApplied?: () => void;
};

type BackgroundMutationOptions = {
  documentId: U64String;
  action: (context: EditorCommandContext) => Promise<EditorMutationResponse>;
  onRefreshFailed?: (error: unknown) => void;
};

export function useDocumentCommandBus({
  applyMutationResponse,
}: DocumentCommandBusOptions) {
  const documentSessionStore = useDocumentSessionStore();

  async function runInteractiveMutation({
    action,
    flushPendingChanges,
    errorMessage,
    refreshProjectionOnError = false,
    afterApplied,
  }: InteractiveMutationOptions): Promise<void> {
    const releaseEditorCommand = documentSessionStore.beginEditorCommand();
    if (!releaseEditorCommand) return;
    const initialContext = documentSessionStore.currentCommandContext();
    if (!initialContext) {
      releaseEditorCommand();
      return;
    }

    try {
      if (!(await flushPendingChanges())) return;
      await documentSessionStore.enqueueDocumentMutation(initialContext.documentId, async (context) => {
        const response = await action(context);
        try {
          const result = await applyMutationResponse(response);
          if (result.applied) runAfterApplied(afterApplied);
        } catch (error) {
          if (!documentSessionStore.markProjectionStaleFromMutationResponse(response)) return;
          if (await refreshAfterMutationError(true)) {
            runAfterApplied(afterApplied);
          } else {
            ElMessage.error(
              `Change was applied, but the editor could not refresh: ${appErrorMessage(error)}`
            );
          }
        }
      });
    } catch (error) {
      await refreshAfterMutationError(
        refreshProjectionOnError || documentSessionStore.projectionStale
      );
      ElMessage.error(`${errorMessage}: ${appErrorMessage(error)}`);
    } finally {
      releaseEditorCommand();
    }
  }

  async function runBackgroundMutation({
    documentId,
    action,
    onRefreshFailed,
  }: BackgroundMutationOptions): Promise<void> {
    await documentSessionStore.enqueueDocumentMutation(documentId, async (context) => {
      const response = await action(context);
      try {
        const result = await applyMutationResponse(response);
        if (!result.applied && documentSessionStore.documentId === documentId) {
          throw new Error('Mutation response was not applied to the active document');
        }
      } catch (error) {
        if (!documentSessionStore.markProjectionStaleFromMutationResponse(response)) return;
        if (!(await refreshAfterMutationError(true))) onRefreshFailed?.(error);
      }
    });
  }

  async function refreshAfterMutationError(refreshProjection: boolean): Promise<boolean> {
    try {
      await documentSessionStore.refreshAfterMutationFailure(
        api.getEditorState,
        refreshProjection && documentSessionStore.data ? api.getCurrentFileData : undefined
      );
      return true;
    } catch (error) {
      console.error('Failed to refresh document session after mutation error:', error);
      return false;
    }
  }

  async function ensureSheetLoaded(
    sheetIndex: number,
    flushPendingChanges: () => Promise<boolean>
  ): Promise<boolean> {
    if (documentSessionStore.isSheetLoaded(sheetIndex)) return true;
    const releaseEditorCommand = documentSessionStore.beginEditorCommand();
    if (!releaseEditorCommand) return false;
    try {
      if (!(await flushPendingChanges())) return false;
      await documentSessionStore.waitForMutations();
      return await documentSessionStore.ensureSheetLoaded(sheetIndex, api.getSheetProjection);
    } catch (error) {
      ElMessage.error(`Failed to load sheet: ${appErrorMessage(error)}`);
      return false;
    } finally {
      releaseEditorCommand();
    }
  }

  function runAfterApplied(afterApplied: (() => void) | undefined) {
    try {
      afterApplied?.();
    } catch (error) {
      console.error('Post-mutation UI update failed:', error);
      ElMessage.error(
        `Change was applied, but the editor UI could not update: ${appErrorMessage(error)}`
      );
    }
  }

  return {
    runInteractiveMutation,
    runBackgroundMutation,
    refreshAfterMutationError,
    ensureSheetLoaded,
  };
}
