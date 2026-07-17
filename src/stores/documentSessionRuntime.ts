import type {
  DocumentProjection,
  U64String,
} from "@/types";

export type DocumentSessionLifecycle = "idle" | "loading" | "saving" | "closing";

type DocumentSessionRuntime = {
  tail: Promise<void> | null;
  generation: number;
  interactionIdleWaiters: Array<() => void>;
};

export type DocumentSessionStateTarget = {
  data: DocumentProjection | null;
  currentFilePath: string | null;
  documentId: U64String | null;
  revision: U64String;
  lifecycle: DocumentSessionLifecycle;
  editorCommandDepth: number;
  projectionStale: boolean;
  residentSheetOrder: number[];
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
};

const documentSessionRuntimes = new WeakMap<object, DocumentSessionRuntime>();

export function beginSessionLifecycle(
  store: DocumentSessionStateTarget,
  lifecycle: Exclude<DocumentSessionLifecycle, "idle">
): boolean {
  if (!isSessionInteractionIdle(store)) {
    return false;
  }
  store.lifecycle = lifecycle;
  return true;
}

export function endSessionLifecycle(
  store: DocumentSessionStateTarget,
  lifecycle: Exclude<DocumentSessionLifecycle, "idle">
) {
  if (store.lifecycle === lifecycle) {
    store.lifecycle = "idle";
    resolveIdleWaitersIfInteractionIdle(store);
  }
}

export function resetSessionLifecycle(store: DocumentSessionStateTarget) {
  store.lifecycle = "idle";
  resolveIdleWaitersIfInteractionIdle(store);
}

export function resetSessionEditorCommands(store: DocumentSessionStateTarget) {
  store.editorCommandDepth = 0;
  resolveIdleWaitersIfInteractionIdle(store);
}

export function beginSessionEditorCommand(store: DocumentSessionStateTarget): (() => void) | null {
  if (store.lifecycle !== "idle" || store.projectionStale || store.editorCommandDepth > 0) {
    return null;
  }
  store.editorCommandDepth += 1;
  let released = false;
  return () => {
    if (released) return;
    released = true;
    store.editorCommandDepth = Math.max(0, store.editorCommandDepth - 1);
    resolveIdleWaitersIfInteractionIdle(store);
  };
}

export function waitForIdleSessionInteraction(store: DocumentSessionStateTarget): Promise<void> {
  if (isSessionInteractionIdle(store)) {
    return Promise.resolve();
  }
  return new Promise((resolve) => {
    sessionRuntimeFor(store).interactionIdleWaiters.push(resolve);
  });
}

export function enqueueMutation<T>(store: object, task: () => Promise<T>): Promise<T | undefined> {
  const runtime = sessionRuntimeFor(store);
  const generation = runtime.generation;
  const tail = runtime.tail ?? Promise.resolve();
  const run = tail.then(
    () => runMutationForGeneration(runtime, generation, task),
    () => runMutationForGeneration(runtime, generation, task)
  );
  const cleanup = run.then(
    () => undefined,
    () => undefined
  );
  runtime.tail = cleanup;
  cleanup.finally(() => {
    if (runtime.tail === cleanup) {
      runtime.tail = null;
    }
  });
  return run;
}

export function waitForQueuedMutations(store: object): Promise<void> {
  return sessionRuntimeFor(store).tail ?? Promise.resolve();
}

export function resetSessionMutationQueue(store: object) {
  resetMutationQueue(store);
}

export function captureMutationSnapshot(
  store: DocumentSessionStateTarget
): DocumentSessionSnapshot {
  return {
    data: store.data,
    currentFilePath: store.currentFilePath,
    documentId: store.documentId,
    revision: store.revision,
    lifecycle: store.lifecycle,
    editorCommandDepth: store.editorCommandDepth,
    projectionStale: store.projectionStale,
    residentSheetOrder: [...store.residentSheetOrder],
  };
}

export function restoreMutationSnapshot(
  store: DocumentSessionStateTarget,
  snapshot: DocumentSessionSnapshot
) {
  store.data = snapshot.data;
  store.currentFilePath = snapshot.currentFilePath;
  store.documentId = snapshot.documentId;
  store.revision = snapshot.revision;
  store.lifecycle = snapshot.lifecycle;
  store.editorCommandDepth = snapshot.editorCommandDepth;
  store.projectionStale = snapshot.projectionStale;
  store.residentSheetOrder = [...snapshot.residentSheetOrder];

  resolveIdleWaitersIfInteractionIdle(store);
}

function resetMutationQueue(store: object) {
  const runtime = sessionRuntimeFor(store);
  runtime.generation += 1;
  runtime.tail = null;
}

function sessionRuntimeFor(store: object): DocumentSessionRuntime {
  let runtime = documentSessionRuntimes.get(store);
  if (!runtime) {
    runtime = { tail: null, generation: 0, interactionIdleWaiters: [] };
    documentSessionRuntimes.set(store, runtime);
  }
  return runtime;
}

function runMutationForGeneration<T>(
  runtime: DocumentSessionRuntime,
  generation: number,
  task: () => Promise<T>
): Promise<T | undefined> {
  if (runtime.generation !== generation) {
    return Promise.resolve(undefined);
  }
  return task();
}

function resolveInteractionIdleWaiters(store: object) {
  const runtime = sessionRuntimeFor(store);
  const waiters = runtime.interactionIdleWaiters.splice(0);
  for (const resolve of waiters) {
    resolve();
  }
}

function resolveIdleWaitersIfInteractionIdle(store: DocumentSessionStateTarget) {
  if (isSessionInteractionIdle(store)) {
    resolveInteractionIdleWaiters(store);
  }
}

function isSessionInteractionIdle(store: DocumentSessionStateTarget): boolean {
  return store.lifecycle === "idle" && store.editorCommandDepth === 0;
}
