import { createDocumentRegionStagingBudget } from '@/application/documentRegionStagingBudget';
import type { RegionStagingLease } from '@/application/documentRegionStagingBudget';

export type RegionLoadPriority = 'required' | 'viewport';

type QueuedLoad = {
  generation: number;
  priority: RegionLoadPriority;
  key: string;
  run: (isCurrent: () => boolean, staging: RegionStagingLease) => Promise<boolean>;
  resolve: (value: boolean) => void;
  reject: (error: unknown) => void;
};

type RegionLoadRecord = {
  promise: Promise<boolean>;
  state: 'queued' | 'active';
  priority: RegionLoadPriority;
};

const MAX_CONCURRENT_REGION_LOADS = 4;
const MAX_ADMITTED_REGION_LOADS = 16;

export function createDocumentRegionLoadScheduler() {
  const stagingBudget = createDocumentRegionStagingBudget();
  let generation = 0;
  let viewportGeneration = 0;
  let viewportKeys = new Set<string>();
  let activeLoads = 0;
  const loads = new Map<string, RegionLoadRecord>();
  let queue: QueuedLoad[] = [];
  const idleWaiters: Array<() => void> = [];

  function reset() {
    generation += 1;
    viewportGeneration += 1;
    viewportKeys.clear();
    loads.clear();
    drainQueue();
  }

  function beginViewportRegionLoad(retainedKeys: Iterable<string>): number {
    viewportGeneration += 1;
    viewportKeys = new Set(retainedKeys);
    cancelQueuedLoads(
      (load) => load.priority === 'viewport' && !viewportKeys.has(load.key),
    );
    return viewportGeneration;
  }

  function scheduleRegionLoad(
    key: string,
    run: (isCurrent: () => boolean, staging: RegionStagingLease) => Promise<boolean>,
    options: { priority?: RegionLoadPriority; viewportGeneration?: number } = {}
  ): Promise<boolean> {
    const priority = options.priority ?? 'required';
    if (priority === 'viewport'
      && (options.viewportGeneration !== viewportGeneration || !viewportKeys.has(key))) {
      return Promise.resolve(false);
    }
    const existing = loads.get(key);
    if (existing) {
      if (priority === 'required' && existing.priority === 'viewport') {
        existing.priority = 'required';
        const queuedIndex = queue.findIndex((load) => load.key === key);
        if (queuedIndex >= 0) {
          const [queued] = queue.splice(queuedIndex, 1);
          if (queued) {
            queued.priority = 'required';
            const firstViewport = queue.findIndex((load) => load.priority === 'viewport');
            if (firstViewport >= 0) queue.splice(firstViewport, 0, queued);
            else queue.push(queued);
          }
        }
      }
      return existing.promise;
    }

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
    record = { promise, state: 'queued', priority };
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
    resolveIdleWaiters();
  }

  function pumpQueue() {
    while (activeLoads < MAX_CONCURRENT_REGION_LOADS && queue.length > 0) {
      const load = queue.shift()!;
      const record = loads.get(load.key);
      if (!record || load.generation !== generation) {
        load.resolve(false);
        continue;
      }
      if (record.priority === 'viewport' && !viewportKeys.has(load.key)) {
        loads.delete(load.key);
        load.resolve(false);
        continue;
      }
      record.state = 'active';
      activeLoads += 1;
      const staging = stagingBudget.acquire();
      const isCurrent = () => {
        if (load.generation !== generation) return false;
        if (loads.get(load.key) !== record) return false;
        return record.priority !== 'viewport' || viewportKeys.has(load.key);
      };
      void load.run(isCurrent, staging).then(
        (value) => load.resolve(isCurrent() && value),
        (error) => {
          if (isCurrent()) load.reject(error);
          else load.resolve(false);
        }
      ).finally(() => {
        staging.release();
        activeLoads = Math.max(0, activeLoads - 1);
        pumpQueue();
        resolveIdleWaiters();
      });
    }
  }

  function drainQueue() {
    for (const load of queue.splice(0)) load.resolve(false);
    pumpQueue();
    resolveIdleWaiters();
  }

  function waitForIdle(): Promise<void> {
    if (activeLoads === 0 && queue.length === 0) return Promise.resolve();
    return new Promise((resolve) => idleWaiters.push(resolve));
  }

  function resolveIdleWaiters() {
    if (activeLoads !== 0 || queue.length !== 0) return;
    for (const resolve of idleWaiters.splice(0)) resolve();
  }

  return { reset, beginViewportRegionLoad, scheduleRegionLoad, waitForIdle };
}
