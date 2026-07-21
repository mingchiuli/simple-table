import { ElMessage } from 'element-plus';
import * as api from '@/api';
import { useDocumentSessionStore } from '@/stores/documentSession';
import { useEditorSelectionStore } from '@/stores/editorSelection';
import { createDocumentMutationProtocol } from '@/application/documentMutationProtocol';
import { useDocumentSessionCoordinator } from '@/composables/useDocumentSessionCoordinator';
import type {
  EditorCommandContext,
  MutationCommandContext,
  SheetRegion,
  U64String,
} from '@/types/documentRuntime';
import type { EditorMutationResponse } from '@/types/protocol';
import { appErrorMessage } from '@/utils/appError';
import type { RegionLoadPriority } from '@/application/documentRegionLoadScheduler';

type InteractiveMutationOptions = {
  action: (context: MutationCommandContext) => Promise<EditorMutationResponse>;
  flushPendingChanges: () => Promise<boolean>;
  errorMessage: string;
  refreshProjectionOnError?: boolean;
  afterApplied?: () => void;
};

type BackgroundMutationOptions = {
  documentId: U64String;
  action: (context: MutationCommandContext) => Promise<EditorMutationResponse>;
  onRefreshFailed?: (error: unknown) => void;
};

type ConsistentReadOptions<T> = {
  flushPendingChanges: () => Promise<boolean>;
  action: (context: EditorCommandContext) => Promise<T>;
  lockInteraction?: boolean;
};

export function useDocumentCommandBus() {
  const documentSessionStore = useDocumentSessionStore();
  const documentSessionCoordinator = useDocumentSessionCoordinator();
  const editorSelectionStore = useEditorSelectionStore();
  const mutationProtocol = createDocumentMutationProtocol({
    transport: {
      getMutationResult: (documentId, commandId) =>
        api.getMutationResult(documentId, commandId),
      getActiveDocument: () => api.getActiveDocument(),
      getCurrentDocumentProjection: (context, preferredSheetIndex) =>
        api.getCurrentDocumentProjection(context, preferredSheetIndex),
    },
    recovery: {
      preferredSheetIndex: () => editorSelectionStore.currentSheetIndex,
      recoverProjection: (response, preferredSheetIndex) =>
        documentSessionCoordinator.recoverActiveDocumentResponse(response, preferredSheetIndex),
    },
    reportError: (message, error) => console.error(`${message}:`, error),
  });

  async function runInteractiveMutation({
    action,
    flushPendingChanges,
    errorMessage,
    refreshProjectionOnError = false,
    afterApplied,
  }: InteractiveMutationOptions): Promise<void> {
    const releaseEditorCommand = documentSessionCoordinator.beginEditorCommand();
    if (!releaseEditorCommand) return;
    const initialContext = documentSessionStore.currentCommandContext();
    if (!initialContext) {
      releaseEditorCommand();
      return;
    }

    try {
      if (!(await flushPendingChanges())) return;
      await documentSessionCoordinator.enqueueDocumentMutation(initialContext.documentId, async (context, lease) => {
        const execution = await mutationProtocol.execute(action, context);
        if (!lease.isCurrent()) return;
        if (execution.status === 'recovered') {
          runAfterApplied(afterApplied);
          return;
        }
        const response = execution.response;
        try {
          const result = await applyMutationResponse(response);
          if (lease.isCurrent() && result.applied) runAfterApplied(afterApplied);
        } catch (error) {
          if (!lease.isCurrent()) return;
          if (!documentSessionCoordinator.markProjectionStaleFromMutationResponse(response)) return;
          if (await refreshAfterMutationError(true)) {
            if (lease.isCurrent()) runAfterApplied(afterApplied);
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
    await documentSessionCoordinator.enqueueDocumentMutation(documentId, async (context, lease) => {
      const execution = await mutationProtocol.execute(action, context);
      if (!lease.isCurrent()) return;
      if (execution.status === 'recovered') return;
      const response = execution.response;
      try {
        const result = await applyMutationResponse(response);
        if (!result.applied && documentSessionStore.documentId === documentId) {
          throw new Error('Mutation response was not applied to the active document');
        }
      } catch (error) {
        if (!lease.isCurrent()) return;
        if (!documentSessionCoordinator.markProjectionStaleFromMutationResponse(response)) return;
        if (!(await refreshAfterMutationError(true))) onRefreshFailed?.(error);
      }
    });
  }

  async function refreshAfterMutationError(refreshProjection: boolean): Promise<boolean> {
    try {
      await documentSessionCoordinator.refreshAfterMutationFailure(
        api.getEditorState,
        refreshProjection && documentSessionStore.data
          ? api.getCurrentDocumentProjection
          : undefined,
        editorSelectionStore.currentSheetIndex
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
      ? documentSessionCoordinator.beginEditorCommand()
      : () => undefined;
    if (!releaseEditorCommand) return undefined;
    const initialContext = documentSessionStore.currentCommandContext();
    if (!initialContext) {
      releaseEditorCommand();
      return undefined;
    }
    try {
      if (!(await flushPendingChanges())) return undefined;
      await documentSessionCoordinator.waitForMutations();
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
    await documentSessionCoordinator.waitForMutations();
    return documentSessionStore.commandContextForDocument(initialContext.documentId) ?? undefined;
  }

  function applyMutationResponse(response: EditorMutationResponse) {
    return documentSessionCoordinator.applyMutationResponseWithResync(
      response,
      api.getCurrentDocumentProjection,
      editorSelectionStore.currentSheetIndex
    );
  }

  async function ensureSheetLoaded(
    sheetIndex: number,
    flushPendingChanges: () => Promise<boolean>
  ): Promise<boolean> {
    const releaseEditorCommand = documentSessionCoordinator.beginEditorCommand();
    if (!releaseEditorCommand) return false;
    try {
      if (!(await flushPendingChanges())) return false;
      await documentSessionCoordinator.waitForMutations();
      return await documentSessionCoordinator.ensureSheetLoaded(
        sheetIndex,
        api.getSheetRegionProjection
      );
    } catch (error) {
      ElMessage.error(`Failed to load sheet: ${appErrorMessage(error)}`);
      return false;
    } finally {
      releaseEditorCommand();
    }
  }

  async function ensureSheetRegionLoaded(
    region: SheetRegion,
    options: { priority?: RegionLoadPriority } = {}
  ): Promise<boolean> {
    try {
      await documentSessionCoordinator.waitForMutations();
      return await documentSessionCoordinator.ensureSheetRegionLoaded(
        region,
        api.getSheetRegionProjection,
        options
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
