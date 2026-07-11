import { ElMessage } from 'element-plus';
import * as api from '@/api';
import { useDocumentSessionStore } from '@/stores/documentSession';
import type { EditorCommandContext, EditorMutationResponse, SheetRegion, U64String } from '@/types';
import { appErrorMessage } from '@/utils/appError';

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

type ConsistentReadOptions<T> = {
  flushPendingChanges: () => Promise<boolean>;
  action: (context: EditorCommandContext) => Promise<T>;
  lockInteraction?: boolean;
};

export function useDocumentCommandBus() {
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
        refreshProjection && documentSessionStore.data
          ? api.getCurrentDocumentProjection
          : undefined
      );
      return true;
    } catch (error) {
      console.error('Failed to refresh document session after mutation error:', error);
      return false;
    }
  }

  async function runConsistentRead<T>({
    flushPendingChanges,
    action,
    lockInteraction = false,
  }: ConsistentReadOptions<T>): Promise<T | undefined> {
    const releaseEditorCommand = lockInteraction
      ? documentSessionStore.beginEditorCommand()
      : () => undefined;
    if (!releaseEditorCommand) return undefined;
    const initialContext = documentSessionStore.currentCommandContext();
    if (!initialContext) {
      releaseEditorCommand();
      return undefined;
    }
    try {
      if (!(await flushPendingChanges())) return undefined;
      await documentSessionStore.waitForMutations();
      const context = documentSessionStore.commandContextForDocument(initialContext.documentId);
      if (!context) return undefined;
      const result = await action(context);
      return documentSessionStore.matchesCommandContext(context) ? result : undefined;
    } finally {
      releaseEditorCommand();
    }
  }

  async function prepareConsistentContext(
    flushPendingChanges: () => Promise<boolean>
  ): Promise<EditorCommandContext | undefined> {
    const initialContext = documentSessionStore.currentCommandContext();
    if (!initialContext) return undefined;
    if (!(await flushPendingChanges())) return undefined;
    await documentSessionStore.waitForMutations();
    return documentSessionStore.commandContextForDocument(initialContext.documentId) ?? undefined;
  }

  function applyMutationResponse(response: EditorMutationResponse) {
    return documentSessionStore.applyMutationResponseWithResync(
      response,
      api.getCurrentDocumentProjection
    );
  }

  async function ensureSheetLoaded(
    sheetIndex: number,
    flushPendingChanges: () => Promise<boolean>
  ): Promise<boolean> {
    const releaseEditorCommand = documentSessionStore.beginEditorCommand();
    if (!releaseEditorCommand) return false;
    try {
      if (!(await flushPendingChanges())) return false;
      await documentSessionStore.waitForMutations();
      return await documentSessionStore.ensureSheetLoaded(sheetIndex, api.getSheetRegionProjection);
    } catch (error) {
      ElMessage.error(`Failed to load sheet: ${appErrorMessage(error)}`);
      return false;
    } finally {
      releaseEditorCommand();
    }
  }

  async function ensureSheetRegionLoaded(region: SheetRegion): Promise<boolean> {
    try {
      await documentSessionStore.waitForMutations();
      return await documentSessionStore.ensureSheetRegionLoaded(
        region,
        api.getSheetRegionProjection
      );
    } catch (error) {
      console.error('Failed to load sheet viewport:', error);
      return false;
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
    ensureSheetRegionLoaded,
    runConsistentRead,
    prepareConsistentContext,
  };
}
