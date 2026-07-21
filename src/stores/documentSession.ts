import type {
  DocumentIdentityStateInput,
  DocumentMutationStateInput,
  DocumentProjection,
  DocumentSessionLifecycle,
  DocumentSessionStateInput,
  EditorCommandContext,
  LoadedSheetSlot,
  SheetExtent,
  SheetRegion,
  SheetRegionBlock,
  U64String,
} from '@/types/documentRuntime';
import {
  MAX_DOCUMENT_MANIFEST_RESIDENT_BYTES,
  MAX_DOCUMENT_PROJECTION_RESIDENT_BYTES,
  MAX_RESIDENT_REGION_BYTES,
} from '@/protocol/editorResourcePolicy';
import {
  createLoadedSheetSlot,
  estimateDocumentManifestResidentBytes,
  isRegionLoaded,
  regionCoveringBlockKeys,
  regionKey,
  replaceLoadedSheetBlocks,
} from '@/projection/documentProjection';
import { ZERO_U64 } from '@/utils/u64';
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

export type { DocumentSessionLifecycle } from '@/types/documentRuntime';

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
  manifestResidentBytes: number;
  residentSheetOrder: number[];
  regionLru: string[];
  pinnedRegionBlocks: string[];
};

const MAX_RESIDENT_SHEETS = 4;
const MAX_BLOCKS_PER_SHEET = 8;
const MAX_RESIDENT_BLOCKS = 24;
const MAX_RESIDENT_BLOCK_BYTES = MAX_RESIDENT_REGION_BYTES;
export const useDocumentSessionStore = defineStore('documentSession', {
  state: () => ({
    data: null as DocumentProjection | null,
    currentFilePath: null as string | null,
    documentId: null as U64String | null,
    revision: ZERO_U64,
    lifecycle: 'idle' as DocumentSessionLifecycle,
    editorCommandDepth: 0,
    projectionStale: false,
    manifestResidentBytes: 0,
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
    replaceProjection(data: DocumentProjection, protectedSheetIndex = 0) {
      resetRegionState(this);
      const manifestResidentBytes = admittedManifestResidentBytes(data);
      this.data = markProjectionCellIndexesRaw(data);
      this.manifestResidentBytes = manifestResidentBytes;
      this.residentSheetOrder = this.loadedSheetIndexes;
      this.projectionStale = false;
      reconcileRegionBlocks(this, currentRegionBlockKeys(this.data));
      this.enforceResidentSheetBudget(protectedSheetIndex);
      this.enforceRegionBlockBudget(protectedSheetIndex);
    },
    replaceSessionState(state: DocumentSessionStateInput) {
      const residentSheetOrder = state.preserveResidentSheetOrder
        ? [...this.residentSheetOrder]
        : [];
      this.replaceProjection(state.data, state.preferredSheetIndex);
      if (state.preserveResidentSheetOrder) {
        const loaded = new Set(this.loadedSheetIndexes);
        this.residentSheetOrder = residentSheetOrder.filter((index) => loaded.has(index));
      }
      this.currentFilePath = state.currentFilePath;
      this.documentId = state.documentId;
      this.revision = state.revision;
      if (state.resetEditorCommandDepth) this.editorCommandDepth = 0;
      this.projectionStale = false;
      if (state.activatePreferredSheet && this.data?.sheets[state.preferredSheetIndex]) {
        this.activateResidentSheet(state.preferredSheetIndex, state.preferredSheetIndex);
      }
      this.enforceResidentSheetBudget(state.preferredSheetIndex);
      this.enforceRegionBlockBudget(state.preferredSheetIndex);
    },
    updateIdentity(path: string | null, fileName: string) {
      if (this.data) {
        const data = { ...this.data, path: path ?? this.data.path, fileName };
        this.manifestResidentBytes = admittedManifestResidentBytes(data);
        this.data = data;
      }
      this.currentFilePath = path;
      this.enforceRegionBlockBudget(0);
    },
    clearDocument() {
      resetRegionState(this);
      this.data = null;
      this.currentFilePath = null;
      this.documentId = null;
      this.revision = ZERO_U64;
      this.editorCommandDepth = 0;
      this.projectionStale = false;
      this.manifestResidentBytes = 0;
      this.residentSheetOrder = [];
      this.lifecycle = 'idle';
    },
    applyMutationState(
      state: DocumentMutationStateInput,
      protectedSheetIndex = 0
    ): MutationApplyResult {
      const data = markProjectionCellIndexesRaw(state.data);
      const manifestResidentBytes = data ? admittedManifestResidentBytes(data) : 0;
      this.documentId = state.documentId;
      this.revision = state.revision;
      this.data = data;
      this.manifestResidentBytes = manifestResidentBytes;
      reconcileRegionBlocks(this, this.data?.sheets.flatMap((sheet) =>
        sheet.state === 'loaded' ? sheet.blocks.map((block) => block.key) : []
      ) ?? []);
      this.reconcileResidentSheets(protectedSheetIndex);
      this.enforceRegionBlockBudget(protectedSheetIndex);
      if (state.resyncRequired) this.projectionStale = true;
      return { data: this.data, resyncRequired: state.resyncRequired, applied: true };
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
          ? slot.blocks.reduce((bytes, block) => bytes + block.residentBytes, 0)
          : 0),
        0
      );
      while (
        totalBlocks() > MAX_RESIDENT_BLOCKS
        || totalBytes() > MAX_RESIDENT_BLOCK_BYTES
        || this.manifestResidentBytes + totalBytes() > MAX_DOCUMENT_PROJECTION_RESIDENT_BYTES
      ) {
        const blockOwners = new Map<string, number>();
        for (const [sheetIndex, slot] of sheets.entries()) {
          if (slot.state !== 'loaded') continue;
          for (const block of slot.blocks) blockOwners.set(block.key, sheetIndex);
        }
        const candidateKeys = new Set(blockOwners.keys());
        const exceedsHardByteBudget = totalBytes() > MAX_RESIDENT_BLOCK_BYTES
          || this.manifestResidentBytes + totalBytes() > MAX_DOCUMENT_PROJECTION_RESIDENT_BYTES;
        const candidateKey = oldestEvictableRegionBlock(this, candidateKeys)
          ?? (exceedsHardByteBudget
            ? this.regionLru.find((key) => candidateKeys.has(key))
            : undefined);
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
    markProjectionStale(identity: DocumentIdentityStateInput) {
      this.documentId = identity.documentId;
      this.revision = identity.revision;
      this.projectionStale = true;
    },
    applyEditorSessionIdentity(identity: DocumentIdentityStateInput, revisionAdvanced: boolean) {
      this.documentId = identity.documentId;
      this.revision = identity.revision;
      if (revisionAdvanced) this.projectionStale = true;
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
        manifestResidentBytes: this.manifestResidentBytes,
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
      this.manifestResidentBytes = snapshot.manifestResidentBytes;
      this.residentSheetOrder = [...snapshot.residentSheetOrder];
      this.regionLru = [...snapshot.regionLru];
      this.pinnedRegionBlocks = [...snapshot.pinnedRegionBlocks];
    },
  },
});

function admittedManifestResidentBytes(data: DocumentProjection): number {
  const bytes = estimateDocumentManifestResidentBytes(data);
  if (bytes > MAX_DOCUMENT_MANIFEST_RESIDENT_BYTES) {
    throw new Error(
      `Document manifest requires ${bytes} resident bytes; maximum is ${MAX_DOCUMENT_MANIFEST_RESIDENT_BYTES}`,
    );
  }
  return bytes;
}

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
