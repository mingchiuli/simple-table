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
  regionBlock,
  regionKey,
} from '@/stores/documentProjection';
import { compareU64, isNextU64, maxU64, ZERO_U64 } from '@/utils/u64';
import { useEditorSelectionStore } from '@/stores/editorSelection';
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
const TILE_ROWS = 128;
const TILE_COLUMNS = 32;
const regionLoads = new WeakMap<object, Map<string, Promise<boolean>>>();

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
      regionLoads.set(this, new Map());
      this.data = createDocumentProjection(response.document, response.initialRegion);
      this.residentSheetOrder = this.loadedSheetIndexes;
      this.currentFilePath = path !== null ? path : response.document.path || null;
      this.documentId = response.editorSession.documentId;
      this.revision = response.editorSession.revision;
      resetSessionEditorCommands(this);
      this.projectionStale = false;
      resetSessionUi();
      this.enforceResidentSheetBudget();
      resetDocumentStatus();
      applyEditorSessionStatus(response.editorSession);
    },
    applySavedDocumentResponse(response: SavedDocumentResponse, path: string | null = null) {
      if (!response.document && (!response.identity || !this.data)) {
        throw new Error('Saved document response did not include manifest or identity data');
      }
      resetTransientDocumentWork(this);
      regionLoads.set(this, new Map());
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
      regionLoads.delete(this);
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
      if (response.protocolVersion !== 2) {
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
      regionLoads.set(this, new Map());
      try {
        const result = applyProjectionPatches(this.data, response.patches, response.sheetExtents);
        this.data = result.data;
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
      while (this.residentSheetOrder.length > MAX_RESIDENT_SHEETS) {
        const position = this.residentSheetOrder.findIndex((index) => index !== protectedSheet);
        if (position < 0) break;
        const [evicted] = this.residentSheetOrder.splice(position, 1);
        const slot = sheets[evicted];
        if (slot?.state === 'loaded') {
          sheets[evicted] = { state: 'unloaded', name: slot.name, extent: slot.extent };
        }
      }
      this.residentSheetOrder = [...this.residentSheetOrder];
      this.data = { ...this.data, sheets };
    },
    enforceRegionBlockBudget(protectedSheet: number) {
      if (!this.data) return;
      const sheets = [...this.data.sheets];
      const protectedSlot = sheets[protectedSheet];
      if (protectedSlot?.state === 'loaded' && protectedSlot.blocks.length > MAX_BLOCKS_PER_SHEET) {
        sheets[protectedSheet] = {
          ...protectedSlot,
          blocks: protectedSlot.blocks.slice(-MAX_BLOCKS_PER_SHEET),
        };
      }
      const totalBlocks = () => sheets.reduce(
        (total, slot) => total + (slot.state === 'loaded' ? slot.blocks.length : 0),
        0
      );
      while (totalBlocks() > MAX_RESIDENT_BLOCKS) {
        const candidate = this.residentSheetOrder.find((index) => {
          const slot = sheets[index];
          return index !== protectedSheet && slot?.state === 'loaded' && slot.blocks.length > 0;
        }) ?? protectedSheet;
        const slot = sheets[candidate];
        if (!slot || slot.state !== 'loaded' || !slot.blocks.length) break;
        sheets[candidate] = { ...slot, blocks: slot.blocks.slice(1) };
      }
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
      ) => Promise<SheetRegionProjectionResponse>
    ): Promise<boolean> {
      if (!this.activateResidentSheet(region.sheetIndex)) return false;
      const slot = this.data?.sheets[region.sheetIndex];
      if (!slot) return false;
      const tiles = tileRegions(region, slot.extent);
      if (!tiles.length) return true;
      const results = await Promise.all(tiles.map((tile) => this.loadRegionBlock(tile, fetchProjection)));
      return results.every(Boolean);
    },
    async loadRegionBlock(
      region: SheetRegion,
      fetchProjection: (
        context: EditorCommandContext,
        region: SheetRegion
      ) => Promise<SheetRegionProjectionResponse>
    ): Promise<boolean> {
      const slot = this.data?.sheets[region.sheetIndex];
      if (isRegionLoaded(slot, region)) return true;
      const context = this.currentCommandContext();
      if (!context) return false;
      const key = `${context.documentId}:${context.baseRevision}:${regionKey(region)}`;
      const loads = regionLoadsFor(this);
      const existing = loads.get(key);
      if (existing) return existing;
      const load = (async () => {
        const response = await fetchProjection(context, region);
        if (!this.matchesCommandContext(context)
          || response.documentId !== context.documentId
          || response.revision !== context.baseRevision
          || regionKey(response.region) !== regionKey(region)) return false;
        const data = this.data;
        const current = data?.sheets[region.sheetIndex];
        if (!data || !current || current.state !== 'loaded') return false;
        const block = regionBlock(response);
        const blocks = [...current.blocks.filter((entry) => entry.key !== block.key), block];
        const sheets = [...data.sheets];
        sheets[region.sheetIndex] = { ...current, blocks };
        this.data = { ...data, sheets };
        this.touchResidentSheet(region.sheetIndex);
        this.enforceRegionBlockBudget(region.sheetIndex);
        return true;
      })().finally(() => loads.delete(key));
      loads.set(key, load);
      return load;
    },
    markProjectionStaleFromMutationResponse(response: EditorMutationResponse): boolean {
      if (this.documentId !== null && response.documentId !== this.documentId) return false;
      if (this.documentId === null && this.data === null) return false;
      if (compareU64(response.revision, this.revision) < 0) return false;
      if (this.documentId === null) this.documentId = response.documentId;
      this.revision = response.revision;
      if (response.protocolVersion === 2) applyResponseStatus(response);
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

function regionLoadsFor(store: object): Map<string, Promise<boolean>> {
  let loads = regionLoads.get(store);
  if (!loads) {
    loads = new Map();
    regionLoads.set(store, loads);
  }
  return loads;
}
