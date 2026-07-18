export type RegionLoadPriority = 'required' | 'viewport';

type QueuedLoad = {
  generation: number;
  viewportGeneration: number | null;
  priority: RegionLoadPriority;
  key: string;
  run: (isCurrent: () => boolean) => Promise<boolean>;
  resolve: (value: boolean) => void;
  reject: (error: unknown) => void;
};

type RegionLoadRecord = {
  promise: Promise<boolean>;
  state: 'queued' | 'active';
};

const MAX_CONCURRENT_REGION_LOADS = 4;
const MAX_ADMITTED_REGION_LOADS = 16;

export function createDocumentRegionLoadScheduler() {
  let generation = 0;
  let viewportGeneration = 0;
  let activeLoads = 0;
  const loads = new Map<string, RegionLoadRecord>();
  let queue: QueuedLoad[] = [];

  function reset() {
    generation += 1;
    viewportGeneration += 1;
    loads.clear();
    drainQueue();
  }

  function beginViewportRegionLoad(retainedKeys: Iterable<string>): number {
    viewportGeneration += 1;
    const retained = new Set(retainedKeys);
    cancelQueuedLoads((load) => load.priority === 'viewport' && !retained.has(load.key));
    for (const load of queue) {
      if (load.priority === 'viewport' && retained.has(load.key)) {
        load.viewportGeneration = viewportGeneration;
      }
    }
    return viewportGeneration;
  }

  function scheduleRegionLoad(
    key: string,
    run: (isCurrent: () => boolean) => Promise<boolean>,
    options: { priority?: RegionLoadPriority; viewportGeneration?: number } = {}
  ): Promise<boolean> {
    const existing = loads.get(key);
    if (existing) return existing.promise;

    const priority = options.priority ?? 'required';
    admitRegionLoad(priority);
    if (loads.size >= MAX_ADMITTED_REGION_LOADS) {
      return priority === 'viewport'
        ? Promise.resolve(false)
        : Promise.reject(new Error('Region load queue is at its admission limit'));
    }

    const loadGeneration = generation;
    let record!: RegionLoadRecord;
    const rawPromise = new Promise<boolean>((resolve, reject) => {
      const queued: QueuedLoad = {
        generation: loadGeneration,
        viewportGeneration: priority === 'viewport'
          ? options.viewportGeneration ?? viewportGeneration
          : null,
        priority,
        key,
        run,
        resolve,
        reject,
      };
      const firstViewport = priority === 'required'
        ? queue.findIndex((load) => load.priority === 'viewport')
        : -1;
      if (firstViewport >= 0) queue.splice(firstViewport, 0, queued);
      else queue.push(queued);
    });
    const promise = rawPromise.finally(() => {
      if (loads.get(key) === record) loads.delete(key);
    });
    record = { promise, state: 'queued' };
    loads.set(key, record);
    pumpQueue();
    return promise;
  }

  function admitRegionLoad(priority: RegionLoadPriority) {
    if (loads.size < MAX_ADMITTED_REGION_LOADS) return;
    if (priority === 'required') {
      cancelQueuedLoads((load) => load.priority === 'viewport', 1);
    }
  }

  function cancelQueuedLoads(
    predicate: (load: QueuedLoad) => boolean,
    maximum = Number.POSITIVE_INFINITY
  ) {
    let canceled = 0;
    const retained: QueuedLoad[] = [];
    for (const load of queue) {
      if (canceled < maximum && predicate(load)) {
        const record = loads.get(load.key);
        if (record?.state === 'queued') loads.delete(load.key);
        load.resolve(false);
        canceled += 1;
      } else {
        retained.push(load);
      }
    }
    queue = retained;
  }

  function pumpQueue() {
    while (activeLoads < MAX_CONCURRENT_REGION_LOADS && queue.length > 0) {
      const load = queue.shift()!;
      const record = loads.get(load.key);
      if (!record || load.generation !== generation) {
        load.resolve(false);
        continue;
      }
      if (load.priority === 'viewport' && load.viewportGeneration !== viewportGeneration) {
        loads.delete(load.key);
        load.resolve(false);
        continue;
      }
      record.state = 'active';
      activeLoads += 1;
      const isCurrent = () => {
        if (load.generation !== generation) return false;
        if (loads.get(load.key) !== record) return false;
        return load.priority !== 'viewport'
          || load.viewportGeneration === viewportGeneration;
      };
      void load.run(isCurrent).then(
        (value) => load.resolve(load.generation === generation && value),
        (error) => {
          if (isCurrent()) load.reject(error);
          else load.resolve(false);
        }
      ).finally(() => {
        activeLoads = Math.max(0, activeLoads - 1);
        pumpQueue();
      });
    }
  }

  function drainQueue() {
    for (const load of queue.splice(0)) load.resolve(false);
    pumpQueue();
  }

  return { reset, beginViewportRegionLoad, scheduleRegionLoad };
}
