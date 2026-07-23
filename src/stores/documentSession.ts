import type {
  DocumentIdentityStateInput,
  DocumentMutationStateInput,
  DocumentProjection,
  DocumentSessionLifecycle,
  DocumentSessionStateInput,
  EditorCommandContext,
  LoadedSheetSlot,
  SheetExtent,
  SheetRegionBlock,
  U64String,
} from '@/types/documentRuntime';
import {
  admitDocumentManifestResidentBytes,
} from '@/projection/documentProjection';
import { ZERO_U64 } from '@/utils/u64';
import { markRaw } from 'vue';

export type { DocumentSessionLifecycle } from '@/types/documentRuntime';

export type MutationApplyResult = {
  data: DocumentProjection | null;
  resyncRequired: boolean;
  applied: boolean;
};

type DocumentSessionStoreStateInput = Pick<
  DocumentSessionStateInput,
  'data' | 'currentFilePath' | 'documentId' | 'revision' | 'resetEditorCommandDepth'
>;

export type DocumentSessionSnapshot = {
  data: DocumentProjection | null;
  currentFilePath: string | null;
  documentId: U64String | null;
  revision: U64String;
  lifecycle: DocumentSessionLifecycle;
  editorCommandDepth: number;
  projectionStale: boolean;
  manifestResidentBytes: number;
};

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
  }),
  getters: {
    isInteractionLocked: (state) => state.lifecycle !== 'idle' || state.editorCommandDepth > 0,
    isEditorInteractionLocked: (state) =>
      state.lifecycle !== 'idle' || state.projectionStale || state.editorCommandDepth > 0,
    sheetExtents: (state): SheetExtent[] => state.data?.sheets.map((slot) => slot.extent) ?? [],
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
    replaceProjection(data: DocumentProjection) {
      const manifestResidentBytes = admitDocumentManifestResidentBytes(data);
      this.data = markProjectionCellIndexesRaw(data);
      this.manifestResidentBytes = manifestResidentBytes;
      this.projectionStale = false;
    },
    replaceCachedProjection(data: DocumentProjection) {
      this.data = markProjectionCellIndexesRaw(data);
    },
    replaceSessionState(state: DocumentSessionStoreStateInput) {
      this.replaceAdmittedSessionState(state, admitDocumentManifestResidentBytes(state.data));
    },
    replaceAdmittedSessionState(
      state: DocumentSessionStoreStateInput,
      manifestResidentBytes: number,
    ) {
      this.data = markProjectionCellIndexesRaw(state.data);
      this.manifestResidentBytes = manifestResidentBytes;
      this.currentFilePath = state.currentFilePath;
      this.documentId = state.documentId;
      this.revision = state.revision;
      if (state.resetEditorCommandDepth) this.editorCommandDepth = 0;
      this.projectionStale = false;
    },
    updateIdentity(path: string | null, fileName: string) {
      if (this.data) {
        const data = { ...this.data, path: path ?? this.data.path, fileName };
        this.manifestResidentBytes = admitDocumentManifestResidentBytes(data);
        this.data = data;
      }
      this.currentFilePath = path;
    },
    clearDocument() {
      this.data = null;
      this.currentFilePath = null;
      this.documentId = null;
      this.revision = ZERO_U64;
      this.editorCommandDepth = 0;
      this.projectionStale = false;
      this.manifestResidentBytes = 0;
      this.lifecycle = 'idle';
    },
    applyMutationState(state: DocumentMutationStateInput): MutationApplyResult {
      const data = markProjectionCellIndexesRaw(state.data);
      const manifestResidentBytes = data ? admitDocumentManifestResidentBytes(data) : 0;
      this.documentId = state.documentId;
      this.revision = state.revision;
      this.data = data;
      this.manifestResidentBytes = manifestResidentBytes;
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
    },
  },
});

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
