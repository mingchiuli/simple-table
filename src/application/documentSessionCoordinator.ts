import type {
  DocumentProjection,
  DocumentSessionLifecycle,
  EditorCommandContext,
  EditorMutationResponse,
  EditorPatch,
  EditorSessionInfo,
  FormulaStatus,
  OpenDocumentResponse,
  SavedDocumentResponse,
  LoadedSheetSlot,
  SheetRegion,
  SheetRegionBlock,
  SheetRegionProjectionResponse,
  U64String,
  WorkbookCapabilities,
} from '@/types';
import { isNextU64 } from '@/utils/u64';
import {
  createDocumentRegionLoadScheduler,
  type RegionLoadPriority,
} from '@/application/documentRegionLoadScheduler';
import {
  loadRegionBlocks,
  tileRegions,
  TILE_COLUMNS,
  TILE_ROWS,
} from '@/application/documentRegionRepository';
import { createDocumentSessionRuntime } from '@/application/documentSessionRuntime';

export type MutationApplyResult = {
  data: DocumentProjection | null;
  resyncRequired: boolean;
  applied: boolean;
};

export type DocumentSessionCoordinatorPorts<
  DocumentSnapshot,
  StatusSnapshot,
  SelectionSnapshot,
  SearchSnapshot,
> = {
  document: {
    readonly data: DocumentProjection | null;
    readonly documentId: U64String | null;
    readonly revision: U64String;
    readonly lifecycle: DocumentSessionLifecycle;
    readonly editorCommandDepth: number;
    readonly projectionStale: boolean;
    beginLifecycle(lifecycle: Exclude<DocumentSessionLifecycle, 'idle'>): boolean;
    endLifecycle(lifecycle: Exclude<DocumentSessionLifecycle, 'idle'>): void;
    beginEditorCommand(): boolean;
    endEditorCommand(): void;
    openDocumentResponse(response: OpenDocumentResponse, path?: string | null): void;
    recoverActiveDocumentResponse(response: OpenDocumentResponse, preferredSheetIndex?: number): boolean;
    applySavedDocumentResponse(
      response: SavedDocumentResponse,
      path?: string | null,
      preferredSheetIndex?: number,
    ): void;
    applySavedDocumentResponseForContext(
      context: EditorCommandContext,
      response: SavedDocumentResponse,
      path?: string | null,
      preferredSheetIndex?: number,
    ): boolean;
    clearDocument(): void;
    applyMutationResponse(
      response: EditorMutationResponse,
      preferredSheetIndex?: number,
    ): MutationApplyResult;
    matchesCommandContext(context: EditorCommandContext): boolean;
    replaceDocumentProjection(response: OpenDocumentResponse, preferredSheetIndex?: number): void;
    markProjectionStaleFromMutationResponse(response: EditorMutationResponse): boolean;
    currentCommandContext(): EditorCommandContext | null;
    commandContextForDocument(documentId: U64String): EditorCommandContext | null;
    activateResidentSheet(sheetIndex: number, protectedSheetIndex?: number): boolean;
    loadedSheet(sheetIndex: number): LoadedSheetSlot | null;
    pinRegionBlocksForLoad(regions: SheetRegion[]): void;
    touchLoadedRegion(region: SheetRegion): boolean;
    commitLoadedRegionBlocks(
      context: EditorCommandContext,
      region: SheetRegion,
      blocks: SheetRegionBlock[],
    ): boolean;
    isSheetRegionLoaded(region: SheetRegion): boolean;
    applyEditorSessionIdentity(info: EditorSessionInfo): {
      applied: boolean;
      revisionAdvanced: boolean;
    };
    captureSessionSnapshot(): DocumentSnapshot;
    restoreSessionSnapshot(snapshot: DocumentSnapshot): void;
  };
  status: {
    clearPendingContentChange(): void;
    reset(): void;
    applyEditorSession(info: EditorSessionInfo | null | undefined): void;
    applyRuntimeStatus(formulaStatus: FormulaStatus, capabilities: WorkbookCapabilities): void;
    applyEditorState(state: EditorMutationResponse['editorState']): void;
    captureSnapshot(): StatusSnapshot;
    restoreSnapshot(snapshot: StatusSnapshot): void;
  };
  selection: {
    reset(): void;
    clearSelection(): void;
    applyEditorPatches(patches: EditorPatch[] | undefined): void;
    clampToSheetData(
      sheetCount: number,
      containsCell: (sheetIndex: number, row: number, col: number) => boolean,
    ): void;
    captureSnapshot(): SelectionSnapshot;
    restoreSnapshot(snapshot: SelectionSnapshot): void;
  };
  pending: { reset(): void };
  search: {
    reset(): void;
    clearSearch(): void;
    captureSnapshot(): SearchSnapshot;
    restoreSnapshot(snapshot: SearchSnapshot): void;
  };
};

type FetchProjection = (
  context: EditorCommandContext,
  preferredSheetIndex: number
) => Promise<OpenDocumentResponse>;

type FetchEditorSession = (
  context: EditorCommandContext | null
) => Promise<EditorSessionInfo | null | undefined>;

type FetchRegionProjection = (
  context: EditorCommandContext,
  region: SheetRegion,
) => Promise<SheetRegionProjectionResponse>;

export function createDocumentSessionCoordinator<
  DocumentSnapshot,
  StatusSnapshot,
  SelectionSnapshot,
  SearchSnapshot,
>({
  document,
  status,
  selection,
  pending,
  search,
}: DocumentSessionCoordinatorPorts<
  DocumentSnapshot,
  StatusSnapshot,
  SelectionSnapshot,
  SearchSnapshot
>) {
  const sessionRuntime = createDocumentSessionRuntime(
    document,
    () => document.beginEditorCommand(),
    () => document.endEditorCommand(),
  );
  const regionLoads = createDocumentRegionLoadScheduler();

  function discardPendingLocalWork() {
    sessionRuntime.reset();
    regionLoads.reset();
    pending.reset();
    status.clearPendingContentChange();
  }

  function openDocumentResponse(response: OpenDocumentResponse, path: string | null = null) {
    sessionRuntime.reset();
    regionLoads.reset();
    document.openDocumentResponse(response, path);
    pending.reset();
    selection.reset();
    search.reset();
    status.reset();
    status.applyEditorSession(response.editorSession);
  }

  function recoverActiveDocumentResponse(
    response: OpenDocumentResponse,
    preferredSheetIndex = 0
  ): boolean {
    if (!document.recoverActiveDocumentResponse(response, preferredSheetIndex)) return false;
    regionLoads.reset();
    status.applyEditorSession(response.editorSession);
    clampSelectionToProjection();
    search.clearSearch();
    return true;
  }

  function applySavedDocumentResponse(
    response: SavedDocumentResponse,
    path: string | null = null,
    preferredSheetIndex = 0
  ) {
    sessionRuntime.reset();
    regionLoads.reset();
    document.applySavedDocumentResponse(response, path, preferredSheetIndex);
    pending.reset();
    status.clearPendingContentChange();
    status.applyEditorSession(response.editorSession);
    clampSelectionToProjection();
    search.reset();
  }

  function applySavedDocumentResponseForContext(
    context: EditorCommandContext,
    response: SavedDocumentResponse,
    path: string | null = null,
    preferredSheetIndex = 0
  ): boolean {
    if (!document.applySavedDocumentResponseForContext(
      context,
      response,
      path,
      preferredSheetIndex
    )) return false;
    sessionRuntime.reset();
    regionLoads.reset();
    pending.reset();
    status.clearPendingContentChange();
    status.applyEditorSession(response.editorSession);
    clampSelectionToProjection();
    search.reset();
    return true;
  }

  function clearDocument() {
    sessionRuntime.reset();
    regionLoads.reset();
    document.clearDocument();
    pending.reset();
    selection.reset();
    search.reset();
    status.reset();
  }

  async function applyMutationResponseWithResync(
    response: EditorMutationResponse,
    fetchProjection: FetchProjection,
    preferredSheetIndex = 0
  ): Promise<MutationApplyResult> {
    const snapshot = captureSnapshot();
    const previousRevision = document.revision;
    const result = document.applyMutationResponse(response, preferredSheetIndex);
    if (!result.applied) return result;

    applyResponseStatus(response);
    const projectionAdvanced = isNextU64(response.revision, previousRevision);
    if (projectionAdvanced) regionLoads.reset();
    if (projectionAdvanced) {
      selection.applyEditorPatches(response.patches);
      clampSelectionToProjection();
    }
    if (result.resyncRequired || mutationInvalidatesSearch(response)) {
      search.clearSearch();
    }
    if (!result.resyncRequired) return result;

    const resyncContext = { documentId: response.documentId, baseRevision: response.revision };
    try {
      const projection = await fetchProjection(resyncContext, preferredSheetIndex);
      if (!document.matchesCommandContext(resyncContext)) {
        return { data: document.data, resyncRequired: true, applied: false };
      }
      document.replaceDocumentProjection(projection, preferredSheetIndex);
      regionLoads.reset();
      status.applyEditorSession(projection.editorSession);
      clampSelectionToProjection();
    } catch (error) {
      if (document.matchesCommandContext(resyncContext)) {
        restoreSnapshot(snapshot);
        regionLoads.reset();
        document.markProjectionStaleFromMutationResponse(response);
        applyResponseStatus(response);
        search.clearSearch();
      }
      throw error;
    }
    return { data: document.data, resyncRequired: true, applied: true };
  }

  function markProjectionStaleFromMutationResponse(response: EditorMutationResponse): boolean {
    if (!document.markProjectionStaleFromMutationResponse(response)) return false;
    regionLoads.reset();
    if (response.protocolVersion === 4) applyResponseStatus(response);
    search.clearSearch();
    return true;
  }

  async function refreshAfterMutationFailure(
    fetchEditorSession: FetchEditorSession,
    fetchProjection?: FetchProjection,
    preferredSheetIndex = 0
  ) {
    const context = document.currentCommandContext();
    if (!fetchProjection || !context) {
      applyEditorSessionForContext(context, await fetchEditorSession(context));
      return;
    }

    const snapshot = captureSnapshot();
    try {
      const [projection, session] = await Promise.all([
        fetchProjection(context, preferredSheetIndex),
        fetchEditorSession(context),
      ]);
      if (!document.matchesCommandContext(context)) return;
      document.replaceDocumentProjection(projection, preferredSheetIndex);
      regionLoads.reset();
      status.applyEditorSession(projection.editorSession);
      clampSelectionToProjection();
      search.clearSearch();
      applyEditorSessionForContext(context, session);
    } catch (error) {
      if (document.matchesCommandContext(context)) restoreSnapshot(snapshot);
      throw error;
    }
  }

  function applyEditorSessionForContext(
    context: EditorCommandContext | null,
    info: EditorSessionInfo | null | undefined
  ) {
    if (context) {
      if (document.matchesCommandContext(context)) applyEditorSession(info);
      return;
    }
    if (document.documentId !== null) return;
    if (!info) {
      clearDocument();
    } else if (document.data !== null) {
      applyEditorSession(info);
    }
  }

  function applyEditorSession(info: EditorSessionInfo | null | undefined) {
    if (!info) {
      clearDocument();
      return;
    }
    const result = document.applyEditorSessionIdentity(info);
    if (!result.applied) return;
    status.applyEditorSession(info);
    if (result.revisionAdvanced) {
      regionLoads.reset();
      search.clearSearch();
    }
  }

  function applyResponseStatus(response: EditorMutationResponse) {
    status.applyRuntimeStatus(response.formulaStatus, response.capabilities);
    status.applyEditorState(response.editorState);
  }

  function clampSelectionToProjection() {
    if (!document.data) {
      selection.clearSelection();
      return;
    }
    selection.clampToSheetData(
      document.data.sheets.length,
      (sheetIndex, row, col) => {
        const sheet = document.data?.sheets[sheetIndex];
        if (!sheet) return false;
        const extent = sheet.extent;
        return row >= 0 && col >= 0 && row < extent.rowCount && col < extent.columnCount;
      }
    );
  }

  function captureSnapshot() {
    return {
      document: document.captureSessionSnapshot(),
      status: status.captureSnapshot(),
      selection: selection.captureSnapshot(),
      search: search.captureSnapshot(),
    };
  }

  function restoreSnapshot(snapshot: ReturnType<typeof captureSnapshot>) {
    document.restoreSessionSnapshot(snapshot.document);
    status.restoreSnapshot(snapshot.status);
    selection.restoreSnapshot(snapshot.selection);
    search.restoreSnapshot(snapshot.search);
    regionLoads.reset();
  }

  function beginLifecycle(lifecycle: Exclude<DocumentSessionLifecycle, 'idle'>): boolean {
    return document.beginLifecycle(lifecycle);
  }

  function endLifecycle(lifecycle: Exclude<DocumentSessionLifecycle, 'idle'>) {
    document.endLifecycle(lifecycle);
    sessionRuntime.notifyInteractionChanged();
  }

  function waitForInteractionIdle() {
    return sessionRuntime.waitForInteractionIdle();
  }

  function beginEditorCommand() {
    return sessionRuntime.beginEditorCommandLease();
  }

  function enqueueDocumentMutation<T>(
    documentId: U64String,
    task: (context: EditorCommandContext) => Promise<T>,
  ): Promise<T | undefined> {
    return sessionRuntime.enqueueMutation(async () => {
      if (document.projectionStale) {
        throw new Error('Document projection is stale; refresh the document before editing.');
      }
      const context = document.commandContextForDocument(documentId);
      return context ? task(context) : undefined;
    });
  }

  function waitForMutations() {
    return sessionRuntime.waitForMutations();
  }

  async function ensureSheetLoaded(
    sheetIndex: number,
    fetchProjection: FetchRegionProjection,
  ): Promise<boolean> {
    if (!document.activateResidentSheet(sheetIndex, sheetIndex)) return false;
    const slot = document.loadedSheet(sheetIndex);
    if (!slot) return false;
    if (slot.extent.rowCount === 0 || slot.extent.columnCount === 0) return true;
    return ensureSheetRegionLoaded({
      sheetIndex,
      rowStart: 0,
      rowEnd: Math.min(TILE_ROWS, slot.extent.rowCount),
      colStart: 0,
      colEnd: Math.min(TILE_COLUMNS, slot.extent.columnCount),
    }, fetchProjection);
  }

  async function ensureSheetRegionLoaded(
    region: SheetRegion,
    fetchProjection: FetchRegionProjection,
    options: { priority?: RegionLoadPriority } = {},
  ): Promise<boolean> {
    if (!document.activateResidentSheet(region.sheetIndex, region.sheetIndex)) return false;
    const slot = document.loadedSheet(region.sheetIndex);
    if (!slot) return false;
    const tiles = tileRegions(region, slot.extent);
    if (!tiles.length) return true;
    const context = document.currentCommandContext();
    if (!context) return false;
    const priority = options.priority ?? 'required';
    const viewportGeneration = priority === 'viewport'
      ? regionLoads.beginViewportRegionLoad(tiles.map((tile) => regionLoadKey(context, tile)))
      : undefined;
    document.pinRegionBlocksForLoad(tiles);
    const results = await Promise.all(tiles.map((tile) => loadRegionBlock(
      context,
      tile,
      fetchProjection,
      { priority, viewportGeneration },
    )));
    return results.every(Boolean) && document.isSheetRegionLoaded(region);
  }

  function loadRegionBlock(
    context: EditorCommandContext,
    region: SheetRegion,
    fetchProjection: FetchRegionProjection,
    options: { priority: RegionLoadPriority; viewportGeneration?: number },
  ): Promise<boolean> {
    if (document.touchLoadedRegion(region)) return Promise.resolve(true);
    return regionLoads.scheduleRegionLoad(
      regionLoadKey(context, region),
      async (isCurrent) => {
        let blocks: SheetRegionBlock[];
        try {
          blocks = await loadRegionBlocks(
            context,
            region,
            fetchProjection,
            () => isCurrent() && document.matchesCommandContext(context),
          );
        } catch (error) {
          if (!document.matchesCommandContext(context)) return false;
          throw error;
        }
        if (!isCurrent() || !document.matchesCommandContext(context)) return false;
        return document.commitLoadedRegionBlocks(context, region, blocks);
      },
      options,
    );
  }

  return {
    discardPendingLocalWork,
    openDocumentResponse,
    recoverActiveDocumentResponse,
    applySavedDocumentResponse,
    applySavedDocumentResponseForContext,
    clearDocument,
    applyMutationResponseWithResync,
    markProjectionStaleFromMutationResponse,
    refreshAfterMutationFailure,
    applyEditorSessionForContext,
    beginLifecycle,
    endLifecycle,
    waitForInteractionIdle,
    beginEditorCommand,
    enqueueDocumentMutation,
    waitForMutations,
    ensureSheetLoaded,
    ensureSheetRegionLoaded,
  };
}

function regionLoadKey(context: EditorCommandContext, region: SheetRegion) {
  return `${context.documentId}:${context.baseRevision}:${region.sheetIndex}:${region.rowStart}:${region.rowEnd}:${region.colStart}:${region.colEnd}`;
}

function mutationInvalidatesSearch(response: EditorMutationResponse): boolean {
  return (response.patches ?? []).some((patch) => patch.type !== 'Layout');
}
