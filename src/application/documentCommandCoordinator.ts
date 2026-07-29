import { createDocumentMutationProtocol } from '@/application/documentMutationProtocol';
import { runtimeDocumentRegionProjection } from '@/application/documentProjectionProtocol';
import type { RegionLoadPriority } from '@/application/documentRegionLoadScheduler';
import type { FetchRegionProjection } from '@/application/documentRegionCoordinator';
import type { MutationApplyResult } from '@/application/documentSessionCoordinator';
import type { DocumentMutationLease } from '@/application/documentSessionRuntime';
import type {
  DocumentProjection,
  EditorCommandContext,
  MutationCommandContext,
  SheetRegion,
  U64String,
} from '@/types/documentRuntime';
import type {
  EditorMutationResponse,
  EditorSessionInfo,
  MutationResultLookup,
  OpenDocumentResponse,
  SheetRegionProjectionResponse,
} from '@/types/protocol';

type MutationAction = (
  context: MutationCommandContext,
) => Promise<EditorMutationResponse>;

export type InteractiveMutationOptions = {
  action: MutationAction;
  flushPendingChanges: () => Promise<boolean>;
  refreshProjectionOnError?: boolean;
  afterApplied?: () => void;
};

export type InteractiveMutationOutcome =
  | { status: 'completed' }
  | { status: 'skipped' }
  | { status: 'failed'; error: unknown }
  | { status: 'refresh-failed'; error: unknown }
  | { status: 'after-applied-failed'; error: unknown };

export type BackgroundMutationOptions = {
  documentId: U64String;
  action: MutationAction;
};

export type BackgroundMutationOutcome =
  | { status: 'completed' }
  | { status: 'skipped' }
  | { status: 'refresh-failed'; error: unknown };

export type EditorStateRefreshOutcome =
  | { status: 'completed' }
  | { status: 'stale' }
  | { status: 'failed'; error: unknown };

export type ConsistentReadOptions<T> = {
  flushPendingChanges: () => Promise<boolean>;
  action: (context: EditorCommandContext) => Promise<T>;
  lockInteraction?: boolean;
};

export type DocumentCommandStatePort = {
  readonly data: DocumentProjection | null;
  readonly documentId: U64String | null;
  readonly projectionStale: boolean;
  currentCommandContext(): EditorCommandContext | null;
  commandContextForDocument(documentId: U64String): EditorCommandContext | null;
  matchesCommandContext(context: EditorCommandContext): boolean;
};

export type DocumentCommandSessionPort = {
  beginEditorCommand(): (() => void) | null;
  enqueueDocumentMutation<T>(
    documentId: U64String,
    task: (context: EditorCommandContext, lease: DocumentMutationLease) => Promise<T>,
  ): Promise<T | undefined>;
  waitForMutations(): Promise<void>;
  recoverActiveDocumentResponse(
    response: OpenDocumentResponse,
    preferredSheetIndex?: number,
  ): boolean;
  applyMutationResponseWithResync(
    response: EditorMutationResponse,
    fetchProjection: (
      context: EditorCommandContext,
      preferredSheetIndex: number,
    ) => Promise<OpenDocumentResponse>,
    preferredSheetIndex?: number,
  ): Promise<MutationApplyResult>;
  markProjectionStaleFromMutationResponse(response: EditorMutationResponse): boolean;
  refreshAfterMutationFailure(
    fetchEditorSession: (
      context: EditorCommandContext | null,
    ) => Promise<EditorSessionInfo | null | undefined>,
    fetchProjection?: (
      context: EditorCommandContext,
      preferredSheetIndex: number,
    ) => Promise<OpenDocumentResponse>,
    preferredSheetIndex?: number,
  ): Promise<void>;
  applyEditorSessionForContext(
    context: EditorCommandContext | null,
    info: EditorSessionInfo | null | undefined,
  ): void;
  ensureSheetLoaded(
    sheetIndex: number,
    fetchProjection: FetchRegionProjection,
  ): Promise<boolean>;
  ensureSheetRegionLoaded(
    region: SheetRegion,
    fetchProjection: FetchRegionProjection,
    options?: { priority?: RegionLoadPriority },
  ): Promise<boolean>;
};

export type DocumentCommandTransport = {
  getMutationResult(
    documentId: U64String,
    commandId: string,
  ): Promise<MutationResultLookup>;
  getActiveDocument(): Promise<OpenDocumentResponse | null>;
  getCurrentDocumentProjection(
    context: EditorCommandContext,
    preferredSheetIndex: number,
  ): Promise<OpenDocumentResponse>;
  getEditorState(
    context: EditorCommandContext | null,
  ): Promise<EditorSessionInfo | null>;
  getSheetRegionProjection(
    context: EditorCommandContext,
    region: SheetRegion,
  ): Promise<SheetRegionProjectionResponse>;
};

type DocumentCommandCoordinatorOptions = {
  document: DocumentCommandStatePort;
  session: DocumentCommandSessionPort;
  transport: DocumentCommandTransport;
  preferredSheetIndex: () => number;
  reportDiagnostic?: (message: string, error: unknown) => void;
};

export function createDocumentCommandCoordinator({
  document,
  session,
  transport,
  preferredSheetIndex,
  reportDiagnostic = () => undefined,
}: DocumentCommandCoordinatorOptions) {
  const mutationProtocol = createDocumentMutationProtocol({
    transport,
    recovery: {
      preferredSheetIndex,
      recoverProjection: (response, sheetIndex) =>
        session.recoverActiveDocumentResponse(response, sheetIndex),
    },
    reportError: reportDiagnostic,
  });

  async function runInteractiveMutation({
    action,
    flushPendingChanges,
    refreshProjectionOnError = false,
    afterApplied,
  }: InteractiveMutationOptions): Promise<InteractiveMutationOutcome> {
    const releaseEditorCommand = session.beginEditorCommand();
    if (!releaseEditorCommand) return { status: 'skipped' };
    const initialContext = document.currentCommandContext();
    if (!initialContext) {
      releaseEditorCommand();
      return { status: 'skipped' };
    }

    try {
      if (!(await flushPendingChanges())) return { status: 'skipped' };
      return await session.enqueueDocumentMutation(
        initialContext.documentId,
        async (context, lease): Promise<InteractiveMutationOutcome> => {
          const execution = await mutationProtocol.execute(action, context);
          if (!lease.isCurrent()) return { status: 'skipped' };
          const response = execution.response;
          try {
            const result = await applyMutationResponse(response);
            return lease.isCurrent() && result.applied
              ? runAfterApplied(afterApplied)
              : { status: 'skipped' };
          } catch (error) {
            if (!lease.isCurrent()) return { status: 'skipped' };
            if (!session.markProjectionStaleFromMutationResponse(response)) {
              return { status: 'skipped' };
            }
            if (await refreshAfterMutationError(true)) {
              return lease.isCurrent()
                ? runAfterApplied(afterApplied)
                : { status: 'skipped' };
            }
            return { status: 'refresh-failed', error };
          }
        },
      ) ?? { status: 'skipped' };
    } catch (error) {
      await refreshAfterMutationError(refreshProjectionOnError || document.projectionStale);
      return { status: 'failed', error };
    } finally {
      releaseEditorCommand();
    }
  }

  async function runBackgroundMutation({
    documentId,
    action,
  }: BackgroundMutationOptions): Promise<BackgroundMutationOutcome> {
    return await session.enqueueDocumentMutation(
      documentId,
      async (context, lease): Promise<BackgroundMutationOutcome> => {
        const execution = await mutationProtocol.execute(action, context);
        if (!lease.isCurrent()) return { status: 'skipped' };
        const response = execution.response;
        try {
          const result = await applyMutationResponse(response);
          if (!result.applied && document.documentId === documentId) {
            throw new Error('Mutation response was not applied to the active document');
          }
          return result.applied ? { status: 'completed' } : { status: 'skipped' };
        } catch (error) {
          if (!lease.isCurrent()) return { status: 'skipped' };
          if (!session.markProjectionStaleFromMutationResponse(response)) {
            return { status: 'skipped' };
          }
          return await refreshAfterMutationError(true)
            ? { status: 'completed' }
            : { status: 'refresh-failed', error };
        }
      },
    ) ?? { status: 'skipped' };
  }

  async function refreshAfterMutationError(refreshProjection: boolean): Promise<boolean> {
    try {
      await session.refreshAfterMutationFailure(
        transport.getEditorState,
        refreshProjection && document.data
          ? transport.getCurrentDocumentProjection
          : undefined,
        preferredSheetIndex(),
      );
      return true;
    } catch (error) {
      reportDiagnostic('Failed to refresh document session after mutation error', error);
      return false;
    }
  }

  async function refreshEditorState(): Promise<EditorStateRefreshOutcome> {
    const context = document.currentCommandContext();
    try {
      const info = await transport.getEditorState(context);
      if (!refreshContextIsCurrent(context)) return { status: 'stale' };
      session.applyEditorSessionForContext(context, info);
      return { status: 'completed' };
    } catch (error) {
      return refreshContextIsCurrent(context)
        ? { status: 'failed', error }
        : { status: 'stale' };
    }
  }

  function refreshContextIsCurrent(context: EditorCommandContext | null): boolean {
    return context
      ? document.matchesCommandContext(context)
      : document.documentId === null;
  }

  async function runConsistentRead<T>({
    flushPendingChanges,
    action,
    lockInteraction = false,
  }: ConsistentReadOptions<T>): Promise<T | undefined> {
    const releaseEditorCommand = lockInteraction
      ? session.beginEditorCommand()
      : () => undefined;
    if (!releaseEditorCommand) return undefined;
    const initialContext = document.currentCommandContext();
    if (!initialContext) {
      releaseEditorCommand();
      return undefined;
    }
    try {
      if (!(await flushPendingChanges())) return undefined;
      await session.waitForMutations();
      const context = document.commandContextForDocument(initialContext.documentId);
      if (!context) return undefined;
      const result = await action(context);
      return document.matchesCommandContext(context) ? result : undefined;
    } finally {
      releaseEditorCommand();
    }
  }

  async function prepareConsistentContext(
    flushPendingChanges: () => Promise<boolean>,
  ): Promise<EditorCommandContext | undefined> {
    const initialContext = document.currentCommandContext();
    if (!initialContext) return undefined;
    if (!(await flushPendingChanges())) return undefined;
    await session.waitForMutations();
    return document.commandContextForDocument(initialContext.documentId) ?? undefined;
  }

  async function ensureSheetLoaded(
    sheetIndex: number,
    flushPendingChanges: () => Promise<boolean>,
  ): Promise<boolean> {
    const releaseEditorCommand = session.beginEditorCommand();
    if (!releaseEditorCommand) return false;
    try {
      if (!(await flushPendingChanges())) return false;
      await session.waitForMutations();
      return await session.ensureSheetLoaded(sheetIndex, fetchRegionProjection);
    } finally {
      releaseEditorCommand();
    }
  }

  async function ensureSheetRegionLoaded(
    region: SheetRegion,
    options: { priority?: RegionLoadPriority } = {},
  ): Promise<boolean> {
    await session.waitForMutations();
    return session.ensureSheetRegionLoaded(region, fetchRegionProjection, options);
  }

  function applyMutationResponse(response: EditorMutationResponse) {
    return session.applyMutationResponseWithResync(
      response,
      transport.getCurrentDocumentProjection,
      preferredSheetIndex(),
    );
  }

  function runAfterApplied(afterApplied: (() => void) | undefined): InteractiveMutationOutcome {
    try {
      afterApplied?.();
      return { status: 'completed' };
    } catch (error) {
      return { status: 'after-applied-failed', error };
    }
  }

  async function fetchRegionProjection(context: EditorCommandContext, region: SheetRegion) {
    return runtimeDocumentRegionProjection(
      await transport.getSheetRegionProjection(context, region),
    );
  }

  return {
    runInteractiveMutation,
    runBackgroundMutation,
    refreshAfterMutationError,
    refreshEditorState,
    ensureSheetLoaded,
    ensureSheetRegionLoaded,
    runConsistentRead,
    prepareConsistentContext,
  };
}
