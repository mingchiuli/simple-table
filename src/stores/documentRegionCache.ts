export type RegionLoadPriority = 'required' | 'viewport';

type QueuedLoad = {
  generation: number;
  viewportGeneration: number | null;
  priority: RegionLoadPriority;
  key: string;
  run: () => Promise<boolean>;
  resolve: (value: boolean) => void;
  reject: (error: unknown) => void;
};

type RegionLoadRecord = {
  promise: Promise<boolean>;
  state: 'queued' | 'active';
};

type RegionCacheRuntime = {
  generation: number;
  viewportGeneration: number;
  activeLoads: number;
  loads: Map<string, RegionLoadRecord>;
  queue: QueuedLoad[];
  lru: string[];
  pinned: Set<string>;
};

const MAX_CONCURRENT_REGION_LOADS = 4;
const MAX_ADMITTED_REGION_LOADS = 16;
const MAX_PINNED_REGION_BLOCKS = 8;
const runtimes = new WeakMap<object, RegionCacheRuntime>();

export function resetRegionCache(owner: object, preserveBlocks = false) {
  const runtime = runtimeFor(owner);
  runtime.generation += 1;
  runtime.viewportGeneration += 1;
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

export function beginViewportRegionLoad(owner: object, retainedKeys: Iterable<string>): number {
  const runtime = runtimeFor(owner);
  runtime.viewportGeneration += 1;
  const retained = new Set(retainedKeys);
  cancelQueuedLoads(runtime, (load) =>
    load.priority === 'viewport' && !retained.has(load.key)
  );
  for (const load of runtime.queue) {
    if (load.priority === 'viewport' && retained.has(load.key)) {
      load.viewportGeneration = runtime.viewportGeneration;
    }
  }
  return runtime.viewportGeneration;
}

export function scheduleRegionLoad(
  owner: object,
  key: string,
  run: () => Promise<boolean>,
  options: { priority?: RegionLoadPriority; viewportGeneration?: number } = {}
): Promise<boolean> {
  const runtime = runtimeFor(owner);
  const existing = runtime.loads.get(key);
  if (existing) return existing.promise;

  const priority = options.priority ?? 'required';
  admitRegionLoad(runtime, priority);
  if (runtime.loads.size >= MAX_ADMITTED_REGION_LOADS) {
    return priority === 'viewport'
      ? Promise.resolve(false)
      : Promise.reject(new Error('Region load queue is at its admission limit'));
  }

  const generation = runtime.generation;
  let record!: RegionLoadRecord;
  const rawPromise = new Promise<boolean>((resolve, reject) => {
    const queued: QueuedLoad = {
      generation,
      viewportGeneration: priority === 'viewport'
        ? options.viewportGeneration ?? runtime.viewportGeneration
        : null,
      priority,
      key,
      run,
      resolve,
      reject,
    };
    const firstViewport = priority === 'required'
      ? runtime.queue.findIndex((load) => load.priority === 'viewport')
      : -1;
    if (firstViewport >= 0) runtime.queue.splice(firstViewport, 0, queued);
    else runtime.queue.push(queued);
  });
  const promise = rawPromise.finally(() => {
    if (runtime.loads.get(key) === record) runtime.loads.delete(key);
  });
  record = { promise, state: 'queued' };
  runtime.loads.set(key, record);
  pumpQueue(owner, runtime);
  return promise;
}

export function pinRegionBlocks(owner: object, keys: string[]) {
  const runtime = runtimeFor(owner);
  runtime.pinned = new Set(keys.slice(-MAX_PINNED_REGION_BLOCKS));
  for (const key of keys) touchRegionBlock(owner, key);
}

export function replacePinnedRegionBlock(owner: object, key: string, replacements: string[]) {
  const runtime = runtimeFor(owner);
  const wasPinned = runtime.pinned.has(key);
  const pinned = [...runtime.pinned].filter((entry) => entry !== key);
  runtime.pinned = new Set(
    (wasPinned ? [...pinned, ...replacements] : pinned).slice(-MAX_PINNED_REGION_BLOCKS)
  );
  runtime.lru = runtime.lru.filter((entry) => entry !== key);
  for (const replacement of replacements) touchRegionBlock(owner, replacement);
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

function admitRegionLoad(runtime: RegionCacheRuntime, priority: RegionLoadPriority) {
  if (runtime.loads.size < MAX_ADMITTED_REGION_LOADS) return;
  if (priority === 'required') {
    cancelQueuedLoads(runtime, (load) => load.priority === 'viewport', 1);
  }
}

function cancelQueuedLoads(
  runtime: RegionCacheRuntime,
  predicate: (load: QueuedLoad) => boolean,
  maximum = Number.POSITIVE_INFINITY
) {
  let canceled = 0;
  const retained: QueuedLoad[] = [];
  for (const load of runtime.queue) {
    if (canceled < maximum && predicate(load)) {
      const record = runtime.loads.get(load.key);
      if (record?.state === 'queued') runtime.loads.delete(load.key);
      load.resolve(false);
      canceled += 1;
    } else {
      retained.push(load);
    }
  }
  runtime.queue = retained;
}

function pumpQueue(owner: object, runtime: RegionCacheRuntime) {
  while (runtime.activeLoads < MAX_CONCURRENT_REGION_LOADS && runtime.queue.length > 0) {
    const load = runtime.queue.shift()!;
    const record = runtime.loads.get(load.key);
    if (!record || load.generation !== runtime.generation) {
      load.resolve(false);
      continue;
    }
    if (
      load.priority === 'viewport'
      && load.viewportGeneration !== runtime.viewportGeneration
    ) {
      runtime.loads.delete(load.key);
      load.resolve(false);
      continue;
    }
    record.state = 'active';
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
      viewportGeneration: 0,
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
