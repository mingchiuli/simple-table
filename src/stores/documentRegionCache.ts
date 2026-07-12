type QueuedLoad = {
  generation: number;
  key: string;
  run: () => Promise<boolean>;
  resolve: (value: boolean) => void;
  reject: (error: unknown) => void;
};

type RegionCacheRuntime = {
  generation: number;
  activeLoads: number;
  loads: Map<string, Promise<boolean>>;
  queue: QueuedLoad[];
  lru: string[];
  pinned: Set<string>;
};

const MAX_CONCURRENT_REGION_LOADS = 4;
const MAX_PINNED_REGION_BLOCKS = 8;
const runtimes = new WeakMap<object, RegionCacheRuntime>();

export function resetRegionCache(owner: object, preserveBlocks = false) {
  const runtime = runtimeFor(owner);
  runtime.generation += 1;
  runtime.loads.clear();
  if (!preserveBlocks) {
    runtime.lru = [];
    runtime.pinned.clear();
  }
  drainQueue(owner, runtime);
}

export function deleteRegionCache(owner: object) {
  resetRegionCache(owner);
  runtimes.delete(owner);
}

export function scheduleRegionLoad(
  owner: object,
  key: string,
  run: () => Promise<boolean>
): Promise<boolean> {
  const runtime = runtimeFor(owner);
  const existing = runtime.loads.get(key);
  if (existing) return existing;

  const generation = runtime.generation;
  const load = new Promise<boolean>((resolve, reject) => {
    runtime.queue.push({ generation, key, run, resolve, reject });
    pumpQueue(owner, runtime);
  }).finally(() => {
    if (runtime.loads.get(key) === load) runtime.loads.delete(key);
  });
  runtime.loads.set(key, load);
  return load;
}

export function pinRegionBlocks(owner: object, keys: string[]) {
  const runtime = runtimeFor(owner);
  runtime.pinned = new Set(keys.slice(-MAX_PINNED_REGION_BLOCKS));
  for (const key of keys) touchRegionBlock(owner, key);
}

export function touchRegionBlock(owner: object, key: string) {
  const runtime = runtimeFor(owner);
  runtime.lru = [...runtime.lru.filter((entry) => entry !== key), key];
}

export function removeRegionBlocks(owner: object, keys: Iterable<string>) {
  const removed = new Set(keys);
  const runtime = runtimeFor(owner);
  runtime.lru = runtime.lru.filter((key) => !removed.has(key));
  for (const key of removed) runtime.pinned.delete(key);
}

export function reconcileRegionBlocks(owner: object, keys: Iterable<string>) {
  const current = new Set(keys);
  const runtime = runtimeFor(owner);
  runtime.lru = runtime.lru.filter((key) => current.has(key));
  runtime.pinned = new Set([...runtime.pinned].filter((key) => current.has(key)));
  for (const key of current) {
    if (!runtime.lru.includes(key)) runtime.lru.push(key);
  }
}

export function oldestEvictableRegionBlock(
  owner: object,
  candidates: ReadonlySet<string>
): string | undefined {
  const runtime = runtimeFor(owner);
  return runtime.lru.find((key) => candidates.has(key) && !runtime.pinned.has(key));
}

function pumpQueue(owner: object, runtime: RegionCacheRuntime) {
  while (runtime.activeLoads < MAX_CONCURRENT_REGION_LOADS && runtime.queue.length > 0) {
    const load = runtime.queue.shift()!;
    if (load.generation !== runtime.generation) {
      load.resolve(false);
      continue;
    }
    runtime.activeLoads += 1;
    void load.run().then(
      (value) => load.resolve(load.generation === runtime.generation && value),
      load.reject
    ).finally(() => {
      runtime.activeLoads = Math.max(0, runtime.activeLoads - 1);
      pumpQueue(owner, runtime);
    });
  }
}

function drainQueue(owner: object, runtime: RegionCacheRuntime) {
  for (const load of runtime.queue.splice(0)) load.resolve(false);
  pumpQueue(owner, runtime);
}

function runtimeFor(owner: object): RegionCacheRuntime {
  let runtime = runtimes.get(owner);
  if (!runtime) {
    runtime = {
      generation: 0,
      activeLoads: 0,
      loads: new Map(),
      queue: [],
      lru: [],
      pinned: new Set(),
    };
    runtimes.set(owner, runtime);
  }
  return runtime;
}
