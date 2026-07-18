import type {
  DocumentProjection,
  DocumentSessionLifecycle,
  EditorCommandContext,
  EditorMutationResponse,
  EditorSessionInfo,
  LoadedSheetSlot,
  OpenDocumentResponse,
  SavedDocumentResponse,
  SheetExtent,
  SheetRegion,
  SheetRegionBlock,
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
} from '@/projection/documentProjection';
import { compareU64, isNextU64, maxU64, ZERO_U64 } from '@/utils/u64';
import {
  oldestEvictableRegionBlock,
  pinRegionBlocks,
  reconcileRegionBlocks,
  removeRegionBlocks,
  replacePinnedRegionBlock,
  resetRegionState,
  touchRegionBlock,
} from '@/stores/documentRegionState';
import { markRaw } from 'vue';

export type { DocumentSessionLifecycle } from '@/types';

export type MutationApplyResult = {
  data: DocumentProjection | null;
  resyncRequired: boolean;
  applied: boolean;
};

export type DocumentSessionSnapshot = {
  data: DocumentProjection | null;
  currentFilePath: string | null;
  documentId: U64String | null;
  revision: U64String;
  lifecycle: DocumentSessionLifecycle;
  editorCommandDepth: number;
  projectionStale: boolean;
  residentSheetOrder: number[];
  regionLru: string[];
  pinnedRegionBlocks: string[];
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
    regionLru: [] as string[],
    pinnedRegionBlocks: [] as string[],
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
      if (this.lifecycle !== 'idle' || this.editorCommandDepth > 0) return false;
      this.lifecycle = lifecycle;
      return true;
    },
    endLifecycle(lifecycle: Exclude<DocumentSessionLifecycle, 'idle'>) {
      if (this.lifecycle === lifecycle) this.lifecycle = 'idle';
    },
    beginEditorCommand(): boolean {
      if (this.lifecycle !== 'idle' || this.projectionStale || this.editorCommandDepth > 0) {
        return false;
      }
      this.editorCommandDepth += 1;
      return true;
    },
    endEditorCommand() {
      this.editorCommandDepth = Math.max(0, this.editorCommandDepth - 1);
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
    replaceDocumentProjection(response: OpenDocumentResponse, protectedSheetIndex = 0) {
      resetRegionState(this);
      this.data = markProjectionCellIndexesRaw(
        createDocumentProjection(response.document, response.initialRegion)
      );
      this.residentSheetOrder = response.initialRegion
        ? [response.initialRegion.region.sheetIndex]
        : [];
      this.projectionStale = false;
      reconcileRegionBlocks(this, currentRegionBlockKeys(this.data));
      this.enforceResidentSheetBudget(protectedSheetIndex);
      this.enforceRegionBlockBudget(response.initialRegion?.region.sheetIndex ?? 0);
    },
    openDocumentResponse(response: OpenDocumentResponse, path: string | null = null) {
      this.replaceDocumentProjection(response);
      this.currentFilePath = path !== null ? path : response.document.path || null;
      this.documentId = response.editorSession.documentId;
      this.revision = response.editorSession.revision;
      this.editorCommandDepth = 0;
      this.projectionStale = false;
    },
    recoverActiveDocumentResponse(response: OpenDocumentResponse, preferredSheetIndex = 0): boolean {
      if (
        this.documentId !== response.editorSession.documentId
        || compareU64(response.editorSession.revision, this.revision) < 0
      ) return false;
      this.replaceDocumentProjection(response, preferredSheetIndex);
      this.revision = response.editorSession.revision;
      this.currentFilePath = response.document.path || this.currentFilePath;
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
      resetRegionState(this);
      const selected = response.document
        ? Math.min(preferredSheetIndex, Math.max(0, response.document.sheets.length - 1))
        : preferredSheetIndex;
      if (response.document) {
        this.data = markProjectionCellIndexesRaw(createDocumentProjection(response.document));
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
      resetRegionState(this);
      this.data = null;
      this.currentFilePath = null;
      this.documentId = null;
      this.revision = ZERO_U64;
      this.editorCommandDepth = 0;
      this.projectionStale = false;
      this.residentSheetOrder = [];
      this.lifecycle = 'idle';
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
        this.projectionStale = true;
        return { data: this.data, resyncRequired: true, applied: true };
      }
      if (response.revision === this.revision && response.patches?.length) {
        this.projectionStale = true;
        return { data: this.data, resyncRequired: true, applied: true };
      }
      if (response.revision === this.revision) {
        return { data: this.data, resyncRequired: false, applied: true };
      }

      this.revision = response.revision;
      try {
        const result = applyProjectionPatches(
          this.data,
          response.patches,
          response.sheetExtents
        );
        this.data = markProjectionCellIndexesRaw(result.data);
        reconcileRegionBlocks(this, this.data?.sheets.flatMap((sheet) =>
          sheet.state === 'loaded' ? sheet.blocks.map((block) => block.key) : []
        ) ?? []);
        this.reconcileResidentSheets(protectedSheetIndex);
        if (result.resyncRequired) {
          this.projectionStale = true;
        }
        return { ...result, applied: true };
      } catch (error) {
        this.projectionStale = true;
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
    pinRegionBlocksForLoad(regions: SheetRegion[]) {
      pinRegionBlocks(this, regions.map(regionKey));
    },
    touchLoadedRegion(region: SheetRegion): boolean {
      const slot = this.data?.sheets[region.sheetIndex];
      const coveringKeys = regionCoveringBlockKeys(slot, region);
      if (coveringKeys === null) return false;
      for (const blockKey of coveringKeys) touchRegionBlock(this, blockKey);
      return true;
    },
    commitLoadedRegionBlocks(
      context: EditorCommandContext,
      region: SheetRegion,
      newBlocks: SheetRegionBlock[]
    ): boolean {
      if (!this.matchesCommandContext(context)) return false;
      const data = this.data;
      const current = data?.sheets[region.sheetIndex];
      if (!data || !current || current.state !== 'loaded') return false;
      markRegionCellIndexesRaw(newBlocks);
      const newKeys = new Set(newBlocks.map((block) => block.key));
      const blocks = [
        ...current.blocks.filter((entry) => !newKeys.has(entry.key)),
        ...newBlocks,
      ];
      const sheets = [...data.sheets];
      sheets[region.sheetIndex] = replaceLoadedSheetBlocks(current, blocks);
      this.data = { ...data, sheets };
      replacePinnedRegionBlock(this, regionKey(region), newBlocks.map((block) => block.key));
      this.touchResidentSheet(region.sheetIndex, region.sheetIndex);
      this.enforceRegionBlockBudget(region.sheetIndex);
      return isRegionLoaded(this.data?.sheets[region.sheetIndex], region);
    },
    isSheetRegionLoaded(region: SheetRegion): boolean {
      return isRegionLoaded(this.data?.sheets[region.sheetIndex], region);
    },
    markProjectionStaleFromMutationResponse(response: EditorMutationResponse): boolean {
      if (this.documentId !== null && response.documentId !== this.documentId) return false;
      if (this.documentId === null && this.data === null) return false;
      if (compareU64(response.revision, this.revision) < 0) return false;
      if (this.documentId === null) this.documentId = response.documentId;
      this.revision = response.revision;
      this.projectionStale = true;
      return true;
    },
    applyEditorSessionIdentity(info: EditorSessionInfo) {
      if (this.data === null) return { applied: false, revisionAdvanced: false };
      if (this.documentId !== null && info.documentId !== this.documentId) {
        return { applied: false, revisionAdvanced: false };
      }
      const revisionAdvancedWithoutProjection = compareU64(info.revision, this.revision) > 0;
      this.documentId = info.documentId;
      this.revision = maxU64(this.revision, info.revision);
      if (revisionAdvancedWithoutProjection) {
        this.projectionStale = true;
      }
      return { applied: true, revisionAdvanced: revisionAdvancedWithoutProjection };
    },
    captureSessionSnapshot(): DocumentSessionSnapshot {
      return {
        data: this.data,
        currentFilePath: this.currentFilePath,
        documentId: this.documentId,
        revision: this.revision,
        lifecycle: this.lifecycle,
        editorCommandDepth: this.editorCommandDepth,
        projectionStale: this.projectionStale,
        residentSheetOrder: [...this.residentSheetOrder],
        regionLru: [...this.regionLru],
        pinnedRegionBlocks: [...this.pinnedRegionBlocks],
      };
    },
    restoreSessionSnapshot(snapshot: DocumentSessionSnapshot) {
      this.data = snapshot.data;
      this.currentFilePath = snapshot.currentFilePath;
      this.documentId = snapshot.documentId;
      this.revision = snapshot.revision;
      this.lifecycle = snapshot.lifecycle;
      this.editorCommandDepth = snapshot.editorCommandDepth;
      this.projectionStale = snapshot.projectionStale;
      this.residentSheetOrder = [...snapshot.residentSheetOrder];
      this.regionLru = [...snapshot.regionLru];
      this.pinnedRegionBlocks = [...snapshot.pinnedRegionBlocks];
    },
  },
});

function currentRegionBlockKeys(data: DocumentProjection | null): string[] {
  return data?.sheets.flatMap((slot) => slot.state === 'loaded'
    ? slot.blocks.map((block) => block.key)
    : []) ?? [];
}

function markProjectionCellIndexesRaw(data: DocumentProjection | null): DocumentProjection | null {
  for (const sheet of data?.sheets ?? []) {
    if (sheet.state === 'loaded') markRegionCellIndexesRaw(sheet.blocks);
  }
  return data;
}

function markRegionCellIndexesRaw(blocks: SheetRegionBlock[]) {
  for (const block of blocks) {
    markRaw(block.cells);
    markRaw(block.mergeAnchorCells);
  }
}
