import type {
  EditorMutationResponse,
  EditorSessionInfo,
  EditorPatch,
  FileData,
  SheetExtent,
  U64String,
} from "@/types";
import { usePendingCellSavesStore } from "@/stores/pendingCellSaves";
import { useSearchSessionStore } from "@/stores/searchSession";
import { useDocumentStatusStore } from "@/stores/documentStatus";
import type { DocumentStatusSnapshot } from "@/stores/documentStatus";
import { useEditorSelectionStore } from "@/stores/editorSelection";
import type { EditorSelectionSnapshot } from "@/stores/editorSelection";
import { calculateSheetExtent } from "@/table-geometry/sheetExtent";

export type DocumentSessionLifecycle = "idle" | "loading" | "saving" | "closing";

type DocumentSessionRuntime = {
  tail: Promise<void> | null;
  generation: number;
  interactionIdleWaiters: Array<() => void>;
};

export type DocumentSessionStateTarget = {
  data: FileData | null;
  currentFilePath: string | null;
  documentId: U64String | null;
  revision: U64String;
  lifecycle: DocumentSessionLifecycle;
  editorCommandDepth: number;
  projectionStale: boolean;
  sheetExtents: SheetExtent[];
  loadedSheetIndexes: number[];
};

export type DocumentSessionSnapshot = {
  data: FileData | null;
  currentFilePath: string | null;
  documentId: U64String | null;
  revision: U64String;
  lifecycle: DocumentSessionLifecycle;
  editorCommandDepth: number;
  projectionStale: boolean;
  sheetExtents: SheetExtent[];
  loadedSheetIndexes: number[];
  status: DocumentStatusSnapshot;
  selection: EditorSelectionSnapshot;
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

export function resetTransientDocumentWork(store: object) {
  resetMutationQueue(store);
  usePendingCellSavesStore().reset();
  useDocumentStatusStore().clearPendingContentChange();
}

export function resetSessionUi() {
  useEditorSelectionStore().reset();
  useSearchSessionStore().reset();
}

export function resetDocumentStatus() {
  useDocumentStatusStore().reset();
}

export function resetSearchSession() {
  useSearchSessionStore().reset();
}

export function clearSearchSession() {
  useSearchSessionStore().clearSearch();
}

export function applyEditorSessionStatus(info: EditorSessionInfo) {
  useDocumentStatusStore().applyEditorSession(info);
}

export function applySelectionPatches(patches: EditorPatch[] | undefined) {
  useEditorSelectionStore().applyEditorPatches(patches);
}

export function replaceProjection(store: DocumentSessionStateTarget, data: FileData) {
  const currentFileName = store.data?.fileName;
  store.data = {
    ...data,
    path: store.currentFilePath ?? data.path,
    fileName: currentFileName ?? data.fileName,
  };
  store.projectionStale = false;
  store.sheetExtents = projectionExtents(data);
  store.loadedSheetIndexes = data.sheets.map((_, index) => index);
  clampSelectionToCurrentSheet(store);
  useSearchSessionStore().clearSearch();
}

export function applyResponseStatus(response: EditorMutationResponse) {
  const statusStore = useDocumentStatusStore();
  statusStore.applyRuntimeStatus(response.formulaStatus, response.capabilities);
  statusStore.applyEditorState(response.editorState);
}

export function captureMutationSnapshot(
  store: DocumentSessionStateTarget
): DocumentSessionSnapshot {
  const statusStore = useDocumentStatusStore();
  const selectionStore = useEditorSelectionStore();
  return {
    data: store.data,
    currentFilePath: store.currentFilePath,
    documentId: store.documentId,
    revision: store.revision,
    lifecycle: store.lifecycle,
    editorCommandDepth: store.editorCommandDepth,
    projectionStale: store.projectionStale,
    sheetExtents: [...store.sheetExtents],
    loadedSheetIndexes: [...store.loadedSheetIndexes],
    status: statusStore.captureSnapshot(),
    selection: selectionStore.captureSnapshot(),
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
  store.sheetExtents = [...snapshot.sheetExtents];
  store.loadedSheetIndexes = [...snapshot.loadedSheetIndexes];

  useDocumentStatusStore().restoreSnapshot(snapshot.status);
  useEditorSelectionStore().restoreSnapshot(snapshot.selection);
  resolveIdleWaitersIfInteractionIdle(store);
}

export function clampSelectionToCurrentSheet(store: DocumentSessionStateTarget) {
  const selectionStore = useEditorSelectionStore();
  if (!store.data) {
    selectionStore.clearSelection();
    return;
  }
  selectionStore.clampToSheetData(store.data.sheets.length, (sheetIndex, row, col) => {
    const sheet = store.data?.sheets[sheetIndex];
    if (!sheet) return false;
    const extent = calculateSheetExtent(
      sheet.rows,
      sheet.merges,
      sheet.columnWidths,
      sheet.rowHeights,
      sheet.rich
    );
    return row >= 0 && col >= 0 && row < extent.rowCount && col < extent.columnCount;
  });
}

export function mutationInvalidatesSearch(patches: EditorPatch[] | undefined): boolean {
  return (patches ?? []).some((patch) => patch.type !== "Layout");
}

export function projectionExtents(data: FileData): SheetExtent[] {
  return data.sheets.map((sheet) => calculateSheetExtent(
    sheet.rows,
    sheet.merges,
    sheet.columnWidths,
    sheet.rowHeights,
    sheet.rich
  ));
}

export function updateLoadedSheetIndexes(
  loadedSheetIndexes: number[],
  patches: EditorPatch[] | undefined
): number[] {
  let loaded = new Set(loadedSheetIndexes);
  for (const patch of patches ?? []) {
    switch (patch.type) {
      case 'SheetInserted': {
        const inserted = patch.data.patch.sheetIndex;
        loaded = new Set(Array.from(loaded, (index) => index >= inserted ? index + 1 : index));
        loaded.add(inserted);
        break;
      }
      case 'SheetDeleted': {
        const deleted = patch.data.patch.sheetIndex;
        loaded = new Set(
          Array.from(loaded)
            .filter((index) => index !== deleted)
            .map((index) => index > deleted ? index - 1 : index)
        );
        break;
      }
      case 'SheetUpdated':
        loaded.add(patch.data.patch.sheetIndex);
        break;
      case 'SheetsReplaced': {
        const { startIndex, sheets } = patch.data.patch;
        loaded = new Set(Array.from(loaded).filter((index) => index < startIndex));
        sheets.forEach((_, offset) => loaded.add(startIndex + offset));
        break;
      }
    }
  }
  return Array.from(loaded).sort((left, right) => left - right);
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
