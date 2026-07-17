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
  createLoadedSheetSlot,
  createDocumentProjection,
  isRegionLoaded,
  regionCoveringBlockKeys,
  regionKey,
  replaceLoadedSheetBlocks,
} from '@/stores/documentProjection';
import { compareU64, isNextU64, maxU64, ZERO_U64 } from '@/utils/u64';
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
  loadRegionBlocks,
  tileRegions,
  TILE_COLUMNS,
  TILE_ROWS,
} from '@/stores/documentRegionRepository';
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
    replaceDocumentProjection(response: OpenDocumentResponse, protectedSheetIndex = 0) {
      resetRegionCache(this);
      replaceProjection(this, response);
      reconcileRegionBlocks(this, currentRegionBlockKeys(this.data));
      this.enforceResidentSheetBudget(protectedSheetIndex);
      this.enforceRegionBlockBudget(response.initialRegion?.region.sheetIndex ?? 0);
    },
    openDocumentResponse(response: OpenDocumentResponse, path: string | null = null) {
      resetTransientDocumentWork(this);
      this.replaceDocumentProjection(response);
      this.currentFilePath = path !== null ? path : response.document.path || null;
      this.documentId = response.editorSession.documentId;
      this.revision = response.editorSession.revision;
      resetSessionEditorCommands(this);
      this.projectionStale = false;
      resetSessionUi();
      resetDocumentStatus();
      applyEditorSessionStatus(response.editorSession);
    },
    recoverActiveDocumentResponse(response: OpenDocumentResponse, preferredSheetIndex = 0): boolean {
      if (
        this.documentId !== response.editorSession.documentId
        || compareU64(response.editorSession.revision, this.revision) < 0
      ) return false;
      this.replaceDocumentProjection(response, preferredSheetIndex);
      this.revision = response.editorSession.revision;
      this.currentFilePath = response.document.path || this.currentFilePath;
      applyEditorSessionStatus(response.editorSession);
      return true;
    },
    applySavedDocumentResponse(
      response: SavedDocumentResponse,
      path: string | null = null,
      preferredSheetIndex = 0
    ) {
      if (!response.document && (!response.identity || !this.data)) {
        throw new Error('Saved document response did not include manifest or identity data');
      }
      resetTransientDocumentWork(this);
      resetRegionCache(this);
      const selected = response.document
        ? Math.min(preferredSheetIndex, Math.max(0, response.document.sheets.length - 1))
        : preferredSheetIndex;
      if (response.document) {
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
      this.enforceResidentSheetBudget(selected);
      clampSelectionToCurrentSheet(this);
      resetSearchSession();
      applyEditorSessionStatus(response.editorSession);
    },
    applySavedDocumentResponseForContext(
      context: EditorCommandContext,
      response: SavedDocumentResponse,
      path: string | null = null,
      preferredSheetIndex = 0
    ): boolean {
      if (
        response.editorSession.documentId !== context.documentId
        || compareU64(response.editorSession.revision, context.baseRevision) < 0
        || !this.matchesCommandContext(context)
      ) return false;
      this.applySavedDocumentResponse(response, path, preferredSheetIndex);
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
    applyMutationResponse(
      response: EditorMutationResponse,
      protectedSheetIndex = 0
    ): MutationApplyResult {
      if (response.protocolVersion !== 4) {
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
          response.sheetExtents
        );
        this.data = result.data;
        reconcileRegionBlocks(this, this.data?.sheets.flatMap((sheet) =>
          sheet.state === 'loaded' ? sheet.blocks.map((block) => block.key) : []
        ) ?? []);
        this.reconcileResidentSheets(protectedSheetIndex);
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
    activateResidentSheet(sheetIndex: number, protectedSheetIndex = 0): boolean {
      const slot = this.data?.sheets[sheetIndex];
      if (!slot || !this.data) return false;
      if (slot.state === 'unloaded') {
        const sheets = [...this.data.sheets];
        sheets[sheetIndex] = createLoadedSheetSlot(
          slot.name,
          slot.extent,
          slot.layout,
          []
        );
        this.data = { ...this.data, sheets };
      }
      this.touchResidentSheet(sheetIndex, protectedSheetIndex);
      return true;
    },
    touchResidentSheet(sheetIndex: number, protectedSheetIndex = 0) {
      if (this.residentSheetOrder.at(-1) === sheetIndex) return;
      this.residentSheetOrder = [
        ...this.residentSheetOrder.filter((index) => index !== sheetIndex),
        sheetIndex,
      ];
      this.enforceResidentSheetBudget(protectedSheetIndex);
    },
    reconcileResidentSheets(protectedSheetIndex?: number) {
      const loaded = new Set(this.loadedSheetIndexes);
      const retained = this.residentSheetOrder.filter((index) => loaded.delete(index));
      this.residentSheetOrder = [...retained, ...loaded];
      this.enforceResidentSheetBudget(protectedSheetIndex ?? 0);
    },
    enforceResidentSheetBudget(protectedSheet: number) {
      if (!this.data) return;
      const sheets = [...this.data.sheets];
      const removedBlockKeys: string[] = [];
      let evictedSheet = false;
      while (this.residentSheetOrder.length > MAX_RESIDENT_SHEETS) {
        const position = this.residentSheetOrder.findIndex((index) => index !== protectedSheet);
        if (position < 0) break;
        const [evicted] = this.residentSheetOrder.splice(position, 1);
        const slot = sheets[evicted];
        if (slot?.state === 'loaded') {
          evictedSheet = true;
          removedBlockKeys.push(...slot.blocks.map((block) => block.key));
          sheets[evicted] = {
            state: 'unloaded', name: slot.name, extent: slot.extent, layout: slot.layout,
          };
        }
      }
      this.residentSheetOrder = [...this.residentSheetOrder];
      if (!evictedSheet) return;
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
          sheets[protectedSheet] = replaceLoadedSheetBlocks(
            protectedSlot,
            protectedSlot.blocks.filter((block) => !removed.has(block.key))
          );
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
        sheets[candidateSheet] = replaceLoadedSheetBlocks(
          slot,
          slot.blocks.filter((block) => block.key !== candidateKey)
        );
        removedKeys.push(candidateKey);
      }
      if (!removedKeys.length) return;
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
      if (!this.activateResidentSheet(sheetIndex, sheetIndex)) return false;
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
      if (!this.activateResidentSheet(region.sheetIndex, region.sheetIndex)) return false;
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
      return scheduleRegionLoad(this, key, async (isCurrent) => {
        const newBlocks = await loadRegionBlocks(
          context,
          region,
          fetchProjection,
          () => isCurrent() && this.matchesCommandContext(context)
        );
        if (!isCurrent() || !this.matchesCommandContext(context)) return false;
        const data = this.data;
        const current = data?.sheets[region.sheetIndex];
        if (!data || !current || current.state !== 'loaded') return false;
        const newKeys = new Set(newBlocks.map((block) => block.key));
        const blocks = [
          ...current.blocks.filter((entry) => !newKeys.has(entry.key)),
          ...newBlocks,
        ];
        const sheets = [...data.sheets];
        sheets[region.sheetIndex] = replaceLoadedSheetBlocks(current, blocks);
        this.data = { ...data, sheets };
        replacePinnedRegionBlock(
          this,
          regionKey(region),
          newBlocks.map((block) => block.key)
        );
        this.touchResidentSheet(region.sheetIndex, region.sheetIndex);
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
      if (response.protocolVersion === 4) applyResponseStatus(response);
      this.projectionStale = true;
      clearSearchSession();
      return true;
    },
    async applyMutationResponseWithResync(
      response: EditorMutationResponse,
      fetchProjection: (
        context: EditorCommandContext,
        preferredSheetIndex: number
      ) => Promise<OpenDocumentResponse>,
      preferredSheetIndex = 0
    ): Promise<MutationApplyResult> {
      const snapshot = captureMutationSnapshot(this);
      const result = this.applyMutationResponse(response, preferredSheetIndex);
      if (!result.applied || !result.resyncRequired) return result;
      const resyncContext = { documentId: response.documentId, baseRevision: response.revision };
      try {
        const projection = await fetchProjection(
          resyncContext,
          preferredSheetIndex
        );
        if (!this.matchesCommandContext(resyncContext)) {
          return { data: this.data, resyncRequired: true, applied: false };
        }
        this.replaceDocumentProjection(projection, preferredSheetIndex);
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
      ) => Promise<OpenDocumentResponse>,
      preferredSheetIndex = 0
    ) {
      const context = this.currentCommandContext();
      if (!fetchProjection || !context) {
        this.applyEditorSessionForContext(context, await fetchEditorSession(context));
        return;
      }
      const snapshot = captureMutationSnapshot(this);
      try {
        const [projection, session] = await Promise.all([
          fetchProjection(context, preferredSheetIndex),
          fetchEditorSession(context),
        ]);
        if (!this.matchesCommandContext(context)) return;
        this.replaceDocumentProjection(projection, preferredSheetIndex);
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

function currentRegionBlockKeys(data: DocumentProjection | null): string[] {
  return data?.sheets.flatMap((slot) => slot.state === 'loaded'
    ? slot.blocks.map((block) => block.key)
    : []) ?? [];
}
