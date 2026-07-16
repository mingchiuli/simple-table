import type {
  DocumentProjection,
  EditorCommandContext,
  EditorMutationResponse,
  EditorSessionInfo,
  LoadedSheetSlot,
  OpenDocumentResponse,
  SavedDocumentResponse,
  SheetExtent,
  SheetRegion,
  SheetRegionProjectionResponse,
  U64String,
} from '@/types';
import {
  applyProjectionPatches,
  createDocumentProjection,
  isRegionLoaded,
  regionCoveringBlockKeys,
  regionBlock,
  regionKey,
} from '@/stores/documentProjection';
import { isAppErrorCode } from '@/utils/appError';
import { compareU64, isNextU64, maxU64, ZERO_U64 } from '@/utils/u64';
import { useEditorSelectionStore } from '@/stores/editorSelection';
import {
  deleteRegionCache,
  beginViewportRegionLoad,
  oldestEvictableRegionBlock,
  pinRegionBlocks,
  reconcileRegionBlocks,
  removeRegionBlocks,
  replacePinnedRegionBlock,
  resetRegionCache,
  scheduleRegionLoad,
  touchRegionBlock,
  type RegionLoadPriority,
} from '@/stores/documentRegionCache';
import {
  applyEditorSessionStatus,
  applyResponseStatus,
  applySelectionPatches,
  beginSessionEditorCommand,
  beginSessionLifecycle,
  captureMutationSnapshot,
  clampSelectionToCurrentSheet,
  clearSearchSession,
  endSessionLifecycle,
  enqueueMutation,
  mutationInvalidatesSearch,
  replaceProjection,
  resetDocumentStatus,
  resetSearchSession,
  resetSessionEditorCommands,
  resetSessionLifecycle,
  resetSessionUi,
  resetTransientDocumentWork,
  restoreMutationSnapshot,
  waitForIdleSessionInteraction,
  waitForQueuedMutations,
  type DocumentSessionLifecycle,
} from '@/stores/documentSessionRuntime';

export type { DocumentSessionLifecycle } from '@/stores/documentSessionRuntime';

export type MutationApplyResult = {
  data: DocumentProjection | null;
  resyncRequired: boolean;
  applied: boolean;
};

const MAX_RESIDENT_SHEETS = 4;
const MAX_BLOCKS_PER_SHEET = 8;
const MAX_RESIDENT_BLOCKS = 24;
const MAX_RESIDENT_BLOCK_BYTES = 16 * 1024 * 1024;
const MAX_REGION_BLOCK_BYTES = 16 * 1024 * 1024;
const TILE_ROWS = 128;
const TILE_COLUMNS = 32;

export const useDocumentSessionStore = defineStore('documentSession', {
  state: () => ({
    data: null as DocumentProjection | null,
    currentFilePath: null as string | null,
    documentId: null as U64String | null,
    revision: ZERO_U64,
    lifecycle: 'idle' as DocumentSessionLifecycle,
    editorCommandDepth: 0,
    projectionStale: false,
    residentSheetOrder: [] as number[],
  }),
  getters: {
    isInteractionLocked: (state) => state.lifecycle !== 'idle' || state.editorCommandDepth > 0,
    isEditorInteractionLocked: (state) =>
      state.lifecycle !== 'idle' || state.projectionStale || state.editorCommandDepth > 0,
    sheetExtents: (state): SheetExtent[] => state.data?.sheets.map((slot) => slot.extent) ?? [],
    loadedSheetIndexes: (state): number[] => state.data?.sheets
      .map((slot, index) => slot.state === 'loaded' ? index : -1)
      .filter((index) => index >= 0) ?? [],
  },
  actions: {
    beginLifecycle(lifecycle: Exclude<DocumentSessionLifecycle, 'idle'>): boolean {
      return beginSessionLifecycle(this, lifecycle);
    },
    endLifecycle(lifecycle: Exclude<DocumentSessionLifecycle, 'idle'>) {
      endSessionLifecycle(this, lifecycle);
    },
    waitForInteractionIdle(): Promise<void> {
      return waitForIdleSessionInteraction(this);
    },
    enqueueDocumentMutation<T>(
      documentId: U64String,
      task: (context: EditorCommandContext) => Promise<T>
    ): Promise<T | undefined> {
      return enqueueMutation(this, async () => {
        if (this.projectionStale) {
          throw new Error('Document projection is stale; refresh the document before editing.');
        }
        const context = this.commandContextForDocument(documentId);
        return context ? task(context) : undefined;
      });
    },
    waitForMutations(): Promise<void> {
      return waitForQueuedMutations(this);
    },
    beginEditorCommand(): (() => void) | null {
      return beginSessionEditorCommand(this);
    },
    currentCommandContext(): EditorCommandContext | null {
      if (this.documentId === null) return null;
      return { documentId: this.documentId, baseRevision: this.revision };
    },
    commandContextForDocument(documentId: U64String): EditorCommandContext | null {
      const context = this.currentCommandContext();
      return context?.documentId === documentId ? context : null;
    },
    requireCommandContext(): EditorCommandContext {
      const context = this.currentCommandContext();
      if (!context) throw new Error('No active editor document');
      return context;
    },
    matchesCommandContext(context: EditorCommandContext): boolean {
      return this.documentId === context.documentId && this.revision === context.baseRevision;
    },
    discardPendingLocalWork() {
      resetTransientDocumentWork(this);
    },
    openDocumentResponse(response: OpenDocumentResponse, path: string | null = null) {
      resetTransientDocumentWork(this);
      resetRegionCache(this);
      this.data = createDocumentProjection(response.document, response.initialRegion);
      reconcileRegionBlocks(this, currentRegionBlockKeys(this.data));
      this.residentSheetOrder = this.loadedSheetIndexes;
      this.currentFilePath = path !== null ? path : response.document.path || null;
      this.documentId = response.editorSession.documentId;
      this.revision = response.editorSession.revision;
      resetSessionEditorCommands(this);
      this.projectionStale = false;
      resetSessionUi();
      this.enforceResidentSheetBudget();
      this.enforceRegionBlockBudget(response.initialRegion?.region.sheetIndex ?? 0);
      resetDocumentStatus();
      applyEditorSessionStatus(response.editorSession);
    },
    recoverActiveDocumentResponse(response: OpenDocumentResponse): boolean {
      if (
        this.documentId !== response.editorSession.documentId
        || compareU64(response.editorSession.revision, this.revision) < 0
      ) return false;
      resetRegionCache(this);
      replaceProjection(this, response);
      reconcileRegionBlocks(this, currentRegionBlockKeys(this.data));
      this.revision = response.editorSession.revision;
      this.currentFilePath = response.document.path || this.currentFilePath;
      this.residentSheetOrder = this.loadedSheetIndexes;
      this.enforceResidentSheetBudget();
      this.enforceRegionBlockBudget(response.initialRegion?.region.sheetIndex ?? 0);
      applyEditorSessionStatus(response.editorSession);
      return true;
    },
    applySavedDocumentResponse(response: SavedDocumentResponse, path: string | null = null) {
      if (!response.document && (!response.identity || !this.data)) {
        throw new Error('Saved document response did not include manifest or identity data');
      }
      resetTransientDocumentWork(this);
      resetRegionCache(this);
      if (response.document) {
        const selected = Math.min(
          useEditorSelectionStore().currentSheetIndex,
          Math.max(0, response.document.sheets.length - 1)
        );
        this.data = createDocumentProjection(response.document);
        this.activateResidentSheet(selected);
      } else if (this.data && response.identity) {
        this.data = {
          ...this.data,
          path: response.identity.path,
          fileName: response.identity.fileName,
        };
      }
      const responsePath = response.document?.path ?? response.identity?.path;
      this.currentFilePath = path !== null ? path : responsePath || null;
      this.documentId = response.editorSession.documentId;
      this.revision = response.editorSession.revision;
      this.projectionStale = false;
      this.enforceResidentSheetBudget();
      clampSelectionToCurrentSheet(this);
      resetSearchSession();
      applyEditorSessionStatus(response.editorSession);
    },
    applySavedDocumentResponseForContext(
      context: EditorCommandContext,
      response: SavedDocumentResponse,
      path: string | null = null
    ): boolean {
      if (
        response.editorSession.documentId !== context.documentId
        || compareU64(response.editorSession.revision, context.baseRevision) < 0
        || !this.matchesCommandContext(context)
      ) return false;
      this.applySavedDocumentResponse(response, path);
      return true;
    },
    updateIdentity(path: string | null, fileName: string) {
      if (this.data) {
        this.data = { ...this.data, path: path ?? this.data.path, fileName };
      }
      this.currentFilePath = path;
    },
    clearDocument() {
      resetTransientDocumentWork(this);
      deleteRegionCache(this);
      this.data = null;
      this.currentFilePath = null;
      this.documentId = null;
      this.revision = ZERO_U64;
      resetSessionEditorCommands(this);
      this.projectionStale = false;
      this.residentSheetOrder = [];
      resetSessionLifecycle(this);
      resetSessionUi();
      resetDocumentStatus();
    },
    applyMutationResponse(response: EditorMutationResponse): MutationApplyResult {
      if (response.protocolVersion !== 3) {
        throw new Error(`Unsupported editor mutation protocol: ${response.protocolVersion}`);
      }
      if (this.documentId !== null && response.documentId !== this.documentId) {
        return { data: this.data, resyncRequired: false, applied: false };
      }
      if (this.documentId === null && this.data === null) {
        return { data: this.data, resyncRequired: false, applied: false };
      }
      if (this.documentId === null) this.documentId = response.documentId;
      if (compareU64(response.revision, this.revision) < 0) {
        return { data: this.data, resyncRequired: false, applied: false };
      }
      if (compareU64(response.revision, this.revision) > 0
          && !isNextU64(response.revision, this.revision)) {
        this.revision = response.revision;
        applyResponseStatus(response);
        this.projectionStale = true;
        clearSearchSession();
        return { data: this.data, resyncRequired: true, applied: true };
      }
      if (response.revision === this.revision && response.patches?.length) {
        applyResponseStatus(response);
        this.projectionStale = true;
        clearSearchSession();
        return { data: this.data, resyncRequired: true, applied: true };
      }
      if (response.revision === this.revision) {
        applyResponseStatus(response);
        return { data: this.data, resyncRequired: false, applied: true };
      }

      applyResponseStatus(response);
      this.revision = response.revision;
      resetRegionCache(this, true);
      try {
        const result = applyProjectionPatches(
          this.data,
          response.patches,
          response.sheetExtents,
          response.sheetLayouts
        );
        this.data = result.data;
        reconcileRegionBlocks(this, this.data?.sheets.flatMap((sheet) =>
          sheet.state === 'loaded' ? sheet.blocks.map((block) => block.key) : []
        ) ?? []);
        this.reconcileResidentSheets();
        applySelectionPatches(response.patches);
        if (mutationInvalidatesSearch(response.patches)) clearSearchSession();
        clampSelectionToCurrentSheet(this);
        if (result.resyncRequired) {
          this.projectionStale = true;
          clearSearchSession();
        }
        return { ...result, applied: true };
      } catch (error) {
        this.projectionStale = true;
        clearSearchSession();
        throw error;
      }
    },
    isSheetLoaded(sheetIndex: number): boolean {
      return this.data?.sheets[sheetIndex]?.state === 'loaded';
    },
    loadedSheet(sheetIndex: number): LoadedSheetSlot | null {
      const slot = this.data?.sheets[sheetIndex];
      return slot?.state === 'loaded' ? slot : null;
    },
    activateResidentSheet(sheetIndex: number): boolean {
      const slot = this.data?.sheets[sheetIndex];
      if (!slot || !this.data) return false;
      if (slot.state === 'unloaded') {
        const sheets = [...this.data.sheets];
        sheets[sheetIndex] = { ...slot, state: 'loaded', blocks: [] };
        this.data = { ...this.data, sheets };
      }
      this.touchResidentSheet(sheetIndex);
      return true;
    },
    touchResidentSheet(sheetIndex: number) {
      this.residentSheetOrder = [
        ...this.residentSheetOrder.filter((index) => index !== sheetIndex),
        sheetIndex,
      ];
      this.enforceResidentSheetBudget();
    },
    reconcileResidentSheets() {
      this.residentSheetOrder = this.loadedSheetIndexes;
      this.enforceResidentSheetBudget();
    },
    enforceResidentSheetBudget() {
      if (!this.data) return;
      const protectedSheet = useEditorSelectionStore().currentSheetIndex;
      const sheets = [...this.data.sheets];
      const removedBlockKeys: string[] = [];
      while (this.residentSheetOrder.length > MAX_RESIDENT_SHEETS) {
        const position = this.residentSheetOrder.findIndex((index) => index !== protectedSheet);
        if (position < 0) break;
        const [evicted] = this.residentSheetOrder.splice(position, 1);
        const slot = sheets[evicted];
        if (slot?.state === 'loaded') {
          removedBlockKeys.push(...slot.blocks.map((block) => block.key));
          sheets[evicted] = {
            state: 'unloaded', name: slot.name, extent: slot.extent, layout: slot.layout,
          };
        }
      }
      this.residentSheetOrder = [...this.residentSheetOrder];
      removeRegionBlocks(this, removedBlockKeys);
      this.data = { ...this.data, sheets };
    },
    enforceRegionBlockBudget(protectedSheet: number) {
      if (!this.data) return;
      const sheets = [...this.data.sheets];
      const protectedSlot = sheets[protectedSheet];
      const removedKeys: string[] = [];
      if (protectedSlot?.state === 'loaded') {
        while (protectedSlot.blocks.length - removedKeys.length > MAX_BLOCKS_PER_SHEET) {
          const remaining = new Set(
            protectedSlot.blocks
              .filter((block) => !removedKeys.includes(block.key))
              .map((block) => block.key)
          );
          const candidate = oldestEvictableRegionBlock(this, remaining);
          if (!candidate) break;
          removedKeys.push(candidate);
        }
        if (removedKeys.length) {
          const removed = new Set(removedKeys);
          sheets[protectedSheet] = {
            ...protectedSlot,
            blocks: protectedSlot.blocks.filter((block) => !removed.has(block.key)),
          };
        }
      }
      const totalBlocks = () => sheets.reduce(
        (total, slot) => total + (slot.state === 'loaded' ? slot.blocks.length : 0),
        0
      );
      const totalBytes = () => sheets.reduce(
        (total, slot) => total + (slot.state === 'loaded'
          ? slot.blocks.reduce((bytes, block) => bytes + block.estimatedBytes, 0)
          : 0),
        0
      );
      while (totalBlocks() > MAX_RESIDENT_BLOCKS || totalBytes() > MAX_RESIDENT_BLOCK_BYTES) {
        const blockOwners = new Map<string, number>();
        for (const [sheetIndex, slot] of sheets.entries()) {
          if (slot.state !== 'loaded') continue;
          for (const block of slot.blocks) blockOwners.set(block.key, sheetIndex);
        }
        const candidateKey = oldestEvictableRegionBlock(this, new Set(blockOwners.keys()));
        const candidateSheet = candidateKey === undefined ? undefined : blockOwners.get(candidateKey);
        if (candidateKey === undefined || candidateSheet === undefined) break;
        const slot = sheets[candidateSheet];
        if (!slot || slot.state !== 'loaded') break;
        sheets[candidateSheet] = {
          ...slot,
          blocks: slot.blocks.filter((block) => block.key !== candidateKey),
        };
        removedKeys.push(candidateKey);
      }
      removeRegionBlocks(this, removedKeys);
      this.data = { ...this.data, sheets };
    },
    async ensureSheetLoaded(
      sheetIndex: number,
      fetchProjection: (
        context: EditorCommandContext,
        region: SheetRegion
      ) => Promise<SheetRegionProjectionResponse>
    ): Promise<boolean> {
      if (!this.activateResidentSheet(sheetIndex)) return false;
      const slot = this.data?.sheets[sheetIndex];
      if (!slot) return false;
      if (slot.extent.rowCount === 0 || slot.extent.columnCount === 0) return true;
      return this.ensureSheetRegionLoaded({
        sheetIndex,
        rowStart: 0,
        rowEnd: Math.min(TILE_ROWS, slot.extent.rowCount),
        colStart: 0,
        colEnd: Math.min(TILE_COLUMNS, slot.extent.columnCount),
      }, fetchProjection);
    },
    async ensureSheetRegionLoaded(
      region: SheetRegion,
      fetchProjection: (
        context: EditorCommandContext,
        region: SheetRegion
      ) => Promise<SheetRegionProjectionResponse>,
      options: { priority?: RegionLoadPriority } = {}
    ): Promise<boolean> {
      if (!this.activateResidentSheet(region.sheetIndex)) return false;
      const slot = this.data?.sheets[region.sheetIndex];
      if (!slot) return false;
      const tiles = tileRegions(region, slot.extent);
      if (!tiles.length) return true;
      const priority = options.priority ?? 'required';
      const context = this.currentCommandContext();
      if (!context) return false;
      const viewportGeneration = priority === 'viewport'
        ? beginViewportRegionLoad(this, tiles.map((tile) =>
          `${context.documentId}:${context.baseRevision}:${regionKey(tile)}`
        ))
        : undefined;
      pinRegionBlocks(this, tiles.map(regionKey));
      const results = await Promise.all(tiles.map((tile) => this.loadRegionBlock(
        tile,
        fetchProjection,
        { priority, viewportGeneration }
      )));
      return results.every(Boolean) && isRegionLoaded(this.data?.sheets[region.sheetIndex], region);
    },
    async loadRegionBlock(
      region: SheetRegion,
      fetchProjection: (
        context: EditorCommandContext,
        region: SheetRegion
      ) => Promise<SheetRegionProjectionResponse>,
      options: { priority?: RegionLoadPriority; viewportGeneration?: number } = {}
    ): Promise<boolean> {
      const slot = this.data?.sheets[region.sheetIndex];
      const coveringKeys = regionCoveringBlockKeys(slot, region);
      if (coveringKeys !== null) {
        for (const blockKey of coveringKeys) touchRegionBlock(this, blockKey);
        return true;
      }
      const context = this.currentCommandContext();
      if (!context) return false;
      const key = `${context.documentId}:${context.baseRevision}:${regionKey(region)}`;
      return scheduleRegionLoad(this, key, async () => {
        const responses = await fetchRegionResponses(context, region, fetchProjection);
        if (!this.matchesCommandContext(context) || responses.some((response) =>
          response.documentId !== context.documentId
          || response.revision !== context.baseRevision
        )) return false;
        const data = this.data;
        const current = data?.sheets[region.sheetIndex];
        if (!data || !current || current.state !== 'loaded') return false;
        const newBlocks = responses.map(regionBlock);
        if (newBlocks.some((block) => block.estimatedBytes > MAX_REGION_BLOCK_BYTES)) return false;
        const newKeys = new Set(newBlocks.map((block) => block.key));
        const blocks = [
          ...current.blocks.filter((entry) => !newKeys.has(entry.key)),
          ...newBlocks,
        ];
        const sheets = [...data.sheets];
        sheets[region.sheetIndex] = { ...current, blocks };
        this.data = { ...data, sheets };
        replacePinnedRegionBlock(
          this,
          regionKey(region),
          newBlocks.map((block) => block.key)
        );
        this.touchResidentSheet(region.sheetIndex);
        this.enforceRegionBlockBudget(region.sheetIndex);
        return isRegionLoaded(this.data?.sheets[region.sheetIndex], region);
      }, options);
    },
    markProjectionStaleFromMutationResponse(response: EditorMutationResponse): boolean {
      if (this.documentId !== null && response.documentId !== this.documentId) return false;
      if (this.documentId === null && this.data === null) return false;
      if (compareU64(response.revision, this.revision) < 0) return false;
      if (this.documentId === null) this.documentId = response.documentId;
      this.revision = response.revision;
      if (response.protocolVersion === 3) applyResponseStatus(response);
      this.projectionStale = true;
      clearSearchSession();
      return true;
    },
    async applyMutationResponseWithResync(
      response: EditorMutationResponse,
      fetchProjection: (
        context: EditorCommandContext,
        preferredSheetIndex: number
      ) => Promise<OpenDocumentResponse>
    ): Promise<MutationApplyResult> {
      const snapshot = captureMutationSnapshot(this);
      const result = this.applyMutationResponse(response);
      if (!result.applied || !result.resyncRequired) return result;
      const resyncContext = { documentId: response.documentId, baseRevision: response.revision };
      try {
        const projection = await fetchProjection(
          resyncContext,
          useEditorSelectionStore().currentSheetIndex
        );
        if (!this.matchesCommandContext(resyncContext)) {
          return { data: this.data, resyncRequired: true, applied: false };
        }
        replaceProjection(this, projection);
      } catch (error) {
        if (this.matchesCommandContext(resyncContext)) {
          restoreMutationSnapshot(this, snapshot);
          this.documentId = response.documentId;
          this.revision = response.revision;
          applyResponseStatus(response);
          this.projectionStale = true;
        }
        throw error;
      }
      return { data: this.data, resyncRequired: true, applied: true };
    },
    async refreshAfterMutationFailure(
      fetchEditorSession: (
        context: EditorCommandContext | null
      ) => Promise<EditorSessionInfo | null | undefined>,
      fetchProjection?: (
        context: EditorCommandContext,
        preferredSheetIndex: number
      ) => Promise<OpenDocumentResponse>
    ) {
      const context = this.currentCommandContext();
      if (!fetchProjection || !context) {
        this.applyEditorSessionForContext(context, await fetchEditorSession(context));
        return;
      }
      const snapshot = captureMutationSnapshot(this);
      try {
        const [projection, session] = await Promise.all([
          fetchProjection(context, useEditorSelectionStore().currentSheetIndex),
          fetchEditorSession(context),
        ]);
        if (!this.matchesCommandContext(context)) return;
        replaceProjection(this, projection);
        this.applyEditorSessionForContext(context, session);
      } catch (error) {
        if (this.matchesCommandContext(context)) restoreMutationSnapshot(this, snapshot);
        throw error;
      }
    },
    applyEditorSessionForContext(
      context: EditorCommandContext | null,
      info: EditorSessionInfo | null | undefined
    ) {
      if (context) {
        if (this.matchesCommandContext(context)) this.applyEditorSession(info);
        return;
      }
      if (this.documentId !== null) return;
      if (!info) {
        this.clearDocument();
      } else if (this.data !== null) {
        this.applyEditorSession(info);
      }
    },
    applyEditorSession(info: EditorSessionInfo | null | undefined) {
      if (!info) {
        this.clearDocument();
        return;
      }
      if (this.data === null) return;
      if (this.documentId !== null && info.documentId !== this.documentId) return;
      const revisionAdvancedWithoutProjection = compareU64(info.revision, this.revision) > 0;
      this.documentId = info.documentId;
      this.revision = maxU64(this.revision, info.revision);
      applyEditorSessionStatus(info);
      if (revisionAdvancedWithoutProjection) {
        this.projectionStale = true;
        clearSearchSession();
      }
    },
  },
});

function tileRegions(region: SheetRegion, extent: SheetExtent): SheetRegion[] {
  const rowStart = Math.max(0, Math.min(region.rowStart, extent.rowCount));
  const rowEnd = Math.max(rowStart, Math.min(region.rowEnd, extent.rowCount));
  const colStart = Math.max(0, Math.min(region.colStart, extent.columnCount));
  const colEnd = Math.max(colStart, Math.min(region.colEnd, extent.columnCount));
  if (rowStart === rowEnd || colStart === colEnd) return [];
  const tiles: SheetRegion[] = [];
  for (let row = Math.floor(rowStart / TILE_ROWS) * TILE_ROWS; row < rowEnd; row += TILE_ROWS) {
    for (let col = Math.floor(colStart / TILE_COLUMNS) * TILE_COLUMNS; col < colEnd; col += TILE_COLUMNS) {
      tiles.push({
        sheetIndex: region.sheetIndex,
        rowStart: row,
        rowEnd: Math.min(row + TILE_ROWS, extent.rowCount),
        colStart: col,
        colEnd: Math.min(col + TILE_COLUMNS, extent.columnCount),
      });
    }
  }
  return tiles;
}

async function fetchRegionResponses(
  context: EditorCommandContext,
  region: SheetRegion,
  fetchProjection: (
    context: EditorCommandContext,
    region: SheetRegion
  ) => Promise<SheetRegionProjectionResponse>
): Promise<SheetRegionProjectionResponse[]> {
  try {
    const response = await fetchProjection(context, region);
    if (regionKey(response.region) !== regionKey(region)) return [];
    return [response];
  } catch (error) {
    if (!isAppErrorCode(error, 'region_response_too_large')) throw error;
    const split = splitRegion(region);
    if (!split) throw error;
    const first = await fetchRegionResponses(context, split[0], fetchProjection);
    const second = await fetchRegionResponses(context, split[1], fetchProjection);
    return [...first, ...second];
  }
}

function splitRegion(region: SheetRegion): [SheetRegion, SheetRegion] | null {
  const rows = region.rowEnd - region.rowStart;
  const columns = region.colEnd - region.colStart;
  if (rows <= 1 && columns <= 1) return null;
  if (rows >= columns && rows > 1) {
    const middle = region.rowStart + Math.floor(rows / 2);
    return [
      { ...region, rowEnd: middle },
      { ...region, rowStart: middle },
    ];
  }
  const middle = region.colStart + Math.floor(columns / 2);
  return [
    { ...region, colEnd: middle },
    { ...region, colStart: middle },
  ];
}

function currentRegionBlockKeys(data: DocumentProjection | null): string[] {
  return data?.sheets.flatMap((slot) => slot.state === 'loaded'
    ? slot.blocks.map((block) => block.key)
    : []) ?? [];
}
