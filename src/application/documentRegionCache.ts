import {
  MAX_DOCUMENT_PROJECTION_RESIDENT_BYTES,
  MAX_RESIDENT_REGION_BYTES,
} from '@/resourcePolicy/editorMemoryPolicy';
import {
  createLoadedSheetSlot,
  isRegionLoaded,
  regionCoveringBlockKeys,
  regionKey,
  replaceLoadedSheetBlocks,
} from '@/projection/documentProjection';
import type {
  DocumentProjection,
  EditorCommandContext,
  LoadedSheetSlot,
  SheetRegion,
  SheetRegionBlock,
} from '@/types/documentRuntime';

const MAX_RESIDENT_SHEETS = 4;
const MAX_BLOCKS_PER_SHEET = 8;
const MAX_RESIDENT_BLOCKS = 24;
const MAX_PINNED_REGION_BLOCKS = 8;

export type DocumentRegionCacheDocumentPort = {
  readonly data: DocumentProjection | null;
  readonly manifestResidentBytes: number;
  currentCommandContext(): EditorCommandContext | null;
  matchesCommandContext(context: EditorCommandContext): boolean;
  replaceCachedProjection(data: DocumentProjection): void;
};

export type DocumentRegionCacheSnapshot = {
  residentSheetOrder: number[];
  regionLru: string[];
  pinnedRegionBlocks: string[];
};

export function createDocumentRegionCache(document: DocumentRegionCacheDocumentPort) {
  let residentSheetOrder: number[] = [];
  let regionLru: string[] = [];
  let pinnedRegionBlocks: string[] = [];

  function reset() {
    residentSheetOrder = [];
    regionLru = [];
    pinnedRegionBlocks = [];
  }

  function reconcileProjection(protectedSheetIndex = 0) {
    const loaded = new Set(loadedSheetIndexes());
    const retained = residentSheetOrder.filter((index) => loaded.delete(index));
    residentSheetOrder = [...retained, ...loaded];
    reconcileRegionBlocks(currentRegionBlockKeys());
    enforceResidentSheetBudget(protectedSheetIndex);
    enforceRegionBlockBudget(protectedSheetIndex);
  }

  function activateResidentSheet(sheetIndex: number, protectedSheetIndex = 0): boolean {
    const data = document.data;
    const slot = data?.sheets[sheetIndex];
    if (!slot || !data) return false;
    if (slot.state === 'unloaded') {
      const sheets = [...data.sheets];
      sheets[sheetIndex] = createLoadedSheetSlot(slot.name, slot.extent, slot.layout, []);
      document.replaceCachedProjection({ ...data, sheets });
    }
    touchResidentSheet(sheetIndex, protectedSheetIndex);
    return true;
  }

  function loadedSheet(sheetIndex: number): LoadedSheetSlot | null {
    const slot = document.data?.sheets[sheetIndex];
    return slot?.state === 'loaded' ? slot : null;
  }

  function touchResidentSheet(sheetIndex: number, protectedSheetIndex = 0) {
    if (residentSheetOrder.at(-1) !== sheetIndex) {
      residentSheetOrder = [
        ...residentSheetOrder.filter((index) => index !== sheetIndex),
        sheetIndex,
      ];
    }
    enforceResidentSheetBudget(protectedSheetIndex);
  }

  function enforceResidentSheetBudget(protectedSheet: number) {
    const data = document.data;
    if (!data) return;
    const sheets = [...data.sheets];
    const removedBlockKeys: string[] = [];
    let evictedSheet = false;
    while (residentSheetOrder.length > MAX_RESIDENT_SHEETS) {
      const position = residentSheetOrder.findIndex((index) => index !== protectedSheet);
      if (position < 0) break;
      const [evicted] = residentSheetOrder.splice(position, 1);
      const slot = sheets[evicted];
      if (slot?.state !== 'loaded') continue;
      evictedSheet = true;
      removedBlockKeys.push(...slot.blocks.map((block) => block.key));
      sheets[evicted] = {
        state: 'unloaded',
        name: slot.name,
        extent: slot.extent,
        layout: slot.layout,
      };
    }
    if (!evictedSheet) return;
    removeRegionBlocks(removedBlockKeys);
    document.replaceCachedProjection({ ...data, sheets });
  }

  function enforceRegionBlockBudget(protectedSheet: number) {
    const data = document.data;
    if (!data) return;
    const sheets = [...data.sheets];
    const protectedSlot = sheets[protectedSheet];
    const removedKeys: string[] = [];
    if (protectedSlot?.state === 'loaded') {
      while (protectedSlot.blocks.length - removedKeys.length > MAX_BLOCKS_PER_SHEET) {
        const remaining = new Set(
          protectedSlot.blocks
            .filter((block) => !removedKeys.includes(block.key))
            .map((block) => block.key),
        );
        const candidate = oldestEvictableRegionBlock(remaining);
        if (!candidate) break;
        removedKeys.push(candidate);
      }
      if (removedKeys.length) {
        const removed = new Set(removedKeys);
        sheets[protectedSheet] = replaceLoadedSheetBlocks(
          protectedSlot,
          protectedSlot.blocks.filter((block) => !removed.has(block.key)),
        );
      }
    }
    const totalBlocks = () => sheets.reduce(
      (total, slot) => total + (slot.state === 'loaded' ? slot.blocks.length : 0),
      0,
    );
    const totalBytes = () => sheets.reduce(
      (total, slot) => total + (slot.state === 'loaded'
        ? slot.blocks.reduce((bytes, block) => bytes + block.residentBytes, 0)
        : 0),
      0,
    );
    while (
      totalBlocks() > MAX_RESIDENT_BLOCKS
      || totalBytes() > MAX_RESIDENT_REGION_BYTES
      || document.manifestResidentBytes + totalBytes() > MAX_DOCUMENT_PROJECTION_RESIDENT_BYTES
    ) {
      const blockOwners = new Map<string, number>();
      for (const [sheetIndex, slot] of sheets.entries()) {
        if (slot.state !== 'loaded') continue;
        for (const block of slot.blocks) blockOwners.set(block.key, sheetIndex);
      }
      const candidateKeys = new Set(blockOwners.keys());
      const exceedsHardByteBudget = totalBytes() > MAX_RESIDENT_REGION_BYTES
        || document.manifestResidentBytes + totalBytes() > MAX_DOCUMENT_PROJECTION_RESIDENT_BYTES;
      const candidateKey = oldestEvictableRegionBlock(candidateKeys)
        ?? (exceedsHardByteBudget
          ? regionLru.find((key) => candidateKeys.has(key))
          : undefined);
      const candidateSheet = candidateKey === undefined ? undefined : blockOwners.get(candidateKey);
      if (candidateKey === undefined || candidateSheet === undefined) break;
      const slot = sheets[candidateSheet];
      if (!slot || slot.state !== 'loaded') break;
      sheets[candidateSheet] = replaceLoadedSheetBlocks(
        slot,
        slot.blocks.filter((block) => block.key !== candidateKey),
      );
      removedKeys.push(candidateKey);
    }
    if (!removedKeys.length) return;
    removeRegionBlocks(removedKeys);
    document.replaceCachedProjection({ ...data, sheets });
  }

  function pinRegionBlocksForLoad(regions: SheetRegion[]) {
    pinnedRegionBlocks = regions.map(regionKey).slice(-MAX_PINNED_REGION_BLOCKS);
    for (const key of pinnedRegionBlocks) touchRegionBlock(key);
  }

  function touchLoadedRegion(region: SheetRegion): boolean {
    const coveringKeys = regionCoveringBlockKeys(
      document.data?.sheets[region.sheetIndex],
      region,
    );
    if (coveringKeys === null) return false;
    for (const blockKey of coveringKeys) touchRegionBlock(blockKey);
    return true;
  }

  function commitLoadedRegionBlocks(
    context: EditorCommandContext,
    region: SheetRegion,
    newBlocks: SheetRegionBlock[],
  ): boolean {
    if (!document.matchesCommandContext(context)) return false;
    const data = document.data;
    const current = data?.sheets[region.sheetIndex];
    if (!data || !current || current.state !== 'loaded') return false;
    const newKeys = new Set(newBlocks.map((block) => block.key));
    const blocks = [
      ...current.blocks.filter((entry) => !newKeys.has(entry.key)),
      ...newBlocks,
    ];
    const sheets = [...data.sheets];
    sheets[region.sheetIndex] = replaceLoadedSheetBlocks(current, blocks);
    document.replaceCachedProjection({ ...data, sheets });
    replacePinnedRegionBlock(regionKey(region), newBlocks.map((block) => block.key));
    touchResidentSheet(region.sheetIndex, region.sheetIndex);
    enforceRegionBlockBudget(region.sheetIndex);
    return isSheetRegionLoaded(region);
  }

  function isSheetRegionLoaded(region: SheetRegion): boolean {
    return isRegionLoaded(document.data?.sheets[region.sheetIndex], region);
  }

  function captureSnapshot(): DocumentRegionCacheSnapshot {
    return {
      residentSheetOrder: [...residentSheetOrder],
      regionLru: [...regionLru],
      pinnedRegionBlocks: [...pinnedRegionBlocks],
    };
  }

  function restoreSnapshot(snapshot: DocumentRegionCacheSnapshot) {
    residentSheetOrder = [...snapshot.residentSheetOrder];
    regionLru = [...snapshot.regionLru];
    pinnedRegionBlocks = [...snapshot.pinnedRegionBlocks];
    reconcileRegionBlocks(currentRegionBlockKeys());
  }

  function loadedSheetIndexes(): number[] {
    return document.data?.sheets
      .map((slot, index) => slot.state === 'loaded' ? index : -1)
      .filter((index) => index >= 0) ?? [];
  }

  function currentRegionBlockKeys(): string[] {
    return document.data?.sheets.flatMap((slot) => slot.state === 'loaded'
      ? slot.blocks.map((block) => block.key)
      : []) ?? [];
  }

  function touchRegionBlock(key: string) {
    regionLru = [...regionLru.filter((entry) => entry !== key), key];
  }

  function removeRegionBlocks(keys: Iterable<string>) {
    const removed = new Set(keys);
    regionLru = regionLru.filter((key) => !removed.has(key));
    pinnedRegionBlocks = pinnedRegionBlocks.filter((key) => !removed.has(key));
  }

  function reconcileRegionBlocks(keys: Iterable<string>) {
    const current = new Set(keys);
    regionLru = regionLru.filter((key) => current.has(key));
    pinnedRegionBlocks = pinnedRegionBlocks.filter((key) => current.has(key));
    for (const key of current) {
      if (!regionLru.includes(key)) regionLru.push(key);
    }
  }

  function replacePinnedRegionBlock(key: string, replacements: string[]) {
    const wasPinned = pinnedRegionBlocks.includes(key);
    const pinned = pinnedRegionBlocks.filter((entry) => entry !== key);
    pinnedRegionBlocks = (wasPinned ? [...pinned, ...replacements] : pinned)
      .slice(-MAX_PINNED_REGION_BLOCKS);
    regionLru = regionLru.filter((entry) => entry !== key);
    for (const replacement of replacements) touchRegionBlock(replacement);
  }

  function oldestEvictableRegionBlock(candidates: ReadonlySet<string>): string | undefined {
    const pinned = new Set(pinnedRegionBlocks);
    return regionLru.find((key) => candidates.has(key) && !pinned.has(key));
  }

  return {
    reset,
    reconcileProjection,
    activateResidentSheet,
    loadedSheet,
    touchResidentSheet,
    pinRegionBlocksForLoad,
    touchLoadedRegion,
    commitLoadedRegionBlocks,
    isSheetRegionLoaded,
    currentCommandContext: () => document.currentCommandContext(),
    matchesCommandContext: (context: EditorCommandContext) => document.matchesCommandContext(context),
    captureSnapshot,
    restoreSnapshot,
  };
}
