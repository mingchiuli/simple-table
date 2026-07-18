const MAX_PINNED_REGION_BLOCKS = 8;

export type DocumentRegionState = {
  regionLru: string[];
  pinnedRegionBlocks: string[];
};

export function resetRegionState(state: DocumentRegionState, preserveBlocks = false) {
  if (preserveBlocks) return;
  state.regionLru = [];
  state.pinnedRegionBlocks = [];
}

export function pinRegionBlocks(state: DocumentRegionState, keys: string[]) {
  state.pinnedRegionBlocks = keys.slice(-MAX_PINNED_REGION_BLOCKS);
  for (const key of keys) touchRegionBlock(state, key);
}

export function replacePinnedRegionBlock(
  state: DocumentRegionState,
  key: string,
  replacements: string[]
) {
  const wasPinned = state.pinnedRegionBlocks.includes(key);
  const pinned = state.pinnedRegionBlocks.filter((entry) => entry !== key);
  state.pinnedRegionBlocks = (wasPinned ? [...pinned, ...replacements] : pinned)
    .slice(-MAX_PINNED_REGION_BLOCKS);
  state.regionLru = state.regionLru.filter((entry) => entry !== key);
  for (const replacement of replacements) touchRegionBlock(state, replacement);
}

export function touchRegionBlock(state: DocumentRegionState, key: string) {
  state.regionLru = [...state.regionLru.filter((entry) => entry !== key), key];
}

export function removeRegionBlocks(state: DocumentRegionState, keys: Iterable<string>) {
  const removed = new Set(keys);
  state.regionLru = state.regionLru.filter((key) => !removed.has(key));
  state.pinnedRegionBlocks = state.pinnedRegionBlocks.filter((key) => !removed.has(key));
}

export function reconcileRegionBlocks(state: DocumentRegionState, keys: Iterable<string>) {
  const current = new Set(keys);
  state.regionLru = state.regionLru.filter((key) => current.has(key));
  state.pinnedRegionBlocks = state.pinnedRegionBlocks.filter((key) => current.has(key));
  for (const key of current) {
    if (!state.regionLru.includes(key)) state.regionLru.push(key);
  }
}

export function oldestEvictableRegionBlock(
  state: DocumentRegionState,
  candidates: ReadonlySet<string>
): string | undefined {
  const pinned = new Set(state.pinnedRegionBlocks);
  return state.regionLru.find((key) => candidates.has(key) && !pinned.has(key));
}
