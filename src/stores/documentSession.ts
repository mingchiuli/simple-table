import type {
  EditorMutationResponse,
  EditorSessionInfo,
  FileData,
  OpenDocumentResponse,
  SavedDocumentResponse,
  EditorCommandContext,
} from "@/types";
import { applyDocumentPatches } from "@/stores/documentPatches";
import {
  applyEditorSessionStatus,
  applyResponseStatus,
  applySelectionPatches,
  beginSessionEditorCommand,
  beginSessionLifecycle,
  captureMutationSnapshot,
  clampSelectionToCurrentSheet,
  clearSearchSession,
  endSessionLifecycle,
  enqueueMutation,
  mutationInvalidatesSearch,
  replaceProjection,
  resetDocumentStatus,
  resetSessionEditorCommands,
  resetSearchSession,
  resetSessionLifecycle,
  resetSessionUi,
  resetTransientDocumentWork,
  restoreMutationSnapshot,
  waitForIdleSessionInteraction,
  waitForQueuedMutations,
  type DocumentSessionLifecycle,
} from "@/stores/documentSessionRuntime";

export type { DocumentSessionLifecycle } from "@/stores/documentSessionRuntime";

export type MutationApplyResult = {
  data: FileData | null;
  resyncRequired: boolean;
  applied: boolean;
};

export const useDocumentSessionStore = defineStore("documentSession", {
  state: () => ({
    data: null as FileData | null,
    currentFilePath: null as string | null,
    documentId: null as number | null,
    revision: 0,
    lifecycle: "idle" as DocumentSessionLifecycle,
    editorCommandDepth: 0,
    projectionStale: false,
  }),
  getters: {
    isInteractionLocked: (state) => state.lifecycle !== "idle" || state.editorCommandDepth > 0,
    isEditorInteractionLocked: (state) =>
      state.lifecycle !== "idle" || state.projectionStale || state.editorCommandDepth > 0,
  },
  actions: {
    beginLifecycle(lifecycle: Exclude<DocumentSessionLifecycle, "idle">): boolean {
      return beginSessionLifecycle(this, lifecycle);
    },
    endLifecycle(lifecycle: Exclude<DocumentSessionLifecycle, "idle">) {
      endSessionLifecycle(this, lifecycle);
    },
    waitForInteractionIdle(): Promise<void> {
      return waitForIdleSessionInteraction(this);
    },
    enqueueDocumentMutation<T>(
      documentId: number,
      task: (context: EditorCommandContext) => Promise<T>
    ): Promise<T | undefined> {
      return enqueueMutation(this, async () => {
        if (this.projectionStale) {
          throw new Error("Document projection is stale; refresh the document before editing.");
        }
        const context = this.commandContextForDocument(documentId);
        if (!context) {
          return undefined;
        }
        return task(context);
      });
    },
    waitForMutations(): Promise<void> {
      return waitForQueuedMutations(this);
    },
    beginEditorCommand(): (() => void) | null {
      return beginSessionEditorCommand(this);
    },
    currentCommandContext(): EditorCommandContext | null {
      if (this.documentId === null) return null;
      return {
        documentId: this.documentId,
        baseRevision: this.revision,
      };
    },
    commandContextForDocument(documentId: number): EditorCommandContext | null {
      const context = this.currentCommandContext();
      if (!context || context.documentId !== documentId) {
        return null;
      }
      return context;
    },
    requireCommandContext(): EditorCommandContext {
      const context = this.currentCommandContext();
      if (!context) {
        throw new Error("No active editor document");
      }
      return context;
    },
    matchesCommandContext(context: EditorCommandContext): boolean {
      return this.documentId === context.documentId && this.revision === context.baseRevision;
    },
    discardPendingLocalWork() {
      resetTransientDocumentWork(this);
    },
    openDocumentResponse(response: OpenDocumentResponse, path: string | null = null) {
      resetTransientDocumentWork(this);
      this.data = response.fileData;
      this.currentFilePath = path !== null ? path : response.fileData.path || null;
      this.documentId = response.editorSession.documentId;
      this.revision = response.editorSession.revision;
      resetSessionEditorCommands(this);
      this.projectionStale = false;
      resetSessionUi();
      resetDocumentStatus();
      applyEditorSessionStatus(response.editorSession);
    },
    applySavedDocumentResponse(response: SavedDocumentResponse, path: string | null = null) {
      resetTransientDocumentWork(this);
      this.data = response.fileData;
      this.currentFilePath = path !== null ? path : response.fileData.path || null;
      this.documentId = response.editorSession.documentId;
      this.revision = response.editorSession.revision;
      this.projectionStale = false;
      clampSelectionToCurrentSheet(this);
      resetSearchSession();
      applyEditorSessionStatus(response.editorSession);
    },
    applySavedDocumentResponseForContext(
      context: EditorCommandContext,
      response: SavedDocumentResponse,
      path: string | null = null
    ): boolean {
      if (
        response.editorSession.documentId !== context.documentId
        || response.editorSession.revision < context.baseRevision
        || !this.matchesCommandContext(context)
      ) {
        return false;
      }
      this.applySavedDocumentResponse(response, path);
      return true;
    },
    updateIdentity(path: string | null, fileName: string) {
      if (this.data) {
        this.data = {
          ...this.data,
          path: path ?? this.data.path,
          fileName,
        };
      }
      this.currentFilePath = path;
    },
    clearDocument() {
      resetTransientDocumentWork(this);
      this.data = null;
      this.currentFilePath = null;
      this.documentId = null;
      this.revision = 0;
      resetSessionEditorCommands(this);
      this.projectionStale = false;
      resetSessionLifecycle(this);
      resetSessionUi();
      resetDocumentStatus();
    },
    applyMutationResponse(response: EditorMutationResponse): MutationApplyResult {
      if (response.protocolVersion !== 1) {
        throw new Error(`Unsupported editor mutation protocol: ${response.protocolVersion}`);
      }
      if (this.documentId !== null && response.documentId !== this.documentId) {
        return { data: this.data, resyncRequired: false, applied: false };
      }
      if (this.documentId === null && this.data === null) {
        return { data: this.data, resyncRequired: false, applied: false };
      }
      if (this.documentId === null) {
        this.documentId = response.documentId;
      }
      if (response.revision < this.revision) {
        return { data: this.data, resyncRequired: false, applied: false };
      }
      if (response.revision > this.revision + 1) {
        this.revision = response.revision;
        applyResponseStatus(response);
        this.projectionStale = true;
        clearSearchSession();
        return { data: this.data, resyncRequired: true, applied: true };
      }
      if (response.revision === this.revision && response.patches?.length) {
        applyResponseStatus(response);
        this.projectionStale = true;
        clearSearchSession();
        return { data: this.data, resyncRequired: true, applied: true };
      }
      if (response.revision === this.revision) {
        applyResponseStatus(response);
        return { data: this.data, resyncRequired: false, applied: true };
      }
      applyResponseStatus(response);
      this.revision = response.revision;
      try {
        const result = applyDocumentPatches(this.data, response.patches);
        this.data = result.data;
        applySelectionPatches(response.patches);
        if (mutationInvalidatesSearch(response.patches)) {
          clearSearchSession();
        }
        clampSelectionToCurrentSheet(this);
        if (result.resyncRequired) {
          this.projectionStale = true;
          clearSearchSession();
        }
        return {
          data: result.data,
          resyncRequired: result.resyncRequired,
          applied: true,
        };
      } catch (error) {
        this.projectionStale = true;
        clearSearchSession();
        throw error;
      }
    },
    markProjectionStaleFromMutationResponse(response: EditorMutationResponse): boolean {
      if (this.documentId !== null && response.documentId !== this.documentId) {
        return false;
      }
      if (this.documentId === null && this.data === null) {
        return false;
      }
      if (response.revision < this.revision) {
        return false;
      }
      if (this.documentId === null) {
        this.documentId = response.documentId;
      }
      this.revision = response.revision;
      if (response.protocolVersion === 1) {
        applyResponseStatus(response);
      }
      this.projectionStale = true;
      clearSearchSession();
      return true;
    },
    async applyMutationResponseWithResync(
      response: EditorMutationResponse,
      fetchProjection: (context: EditorCommandContext) => Promise<FileData>
    ): Promise<MutationApplyResult> {
      const snapshot = captureMutationSnapshot(this);
      const result = this.applyMutationResponse(response);
      if (!result.applied) {
        return result;
      }
      if (!result.resyncRequired) {
        return result;
      }
      const resyncContext = {
        documentId: response.documentId,
        baseRevision: response.revision,
      };
      try {
        const projection = await fetchProjection(resyncContext);
        if (!this.matchesCommandContext(resyncContext)) {
          return {
            data: this.data,
            resyncRequired: true,
            applied: false,
          };
        }
        replaceProjection(this, projection);
      } catch (error) {
        if (this.matchesCommandContext(resyncContext)) {
          restoreMutationSnapshot(this, snapshot);
          this.documentId = response.documentId;
          this.revision = response.revision;
          applyResponseStatus(response);
          this.projectionStale = true;
        }
        throw error;
      }
      return {
        data: this.data,
        resyncRequired: true,
        applied: true,
      };
    },
    async refreshAfterMutationFailure(
      fetchEditorSession: (
        context: EditorCommandContext | null
      ) => Promise<EditorSessionInfo | null | undefined>,
      fetchProjection?: (context: EditorCommandContext) => Promise<FileData>
    ) {
      const context = this.currentCommandContext();
      if (!fetchProjection || !context) {
        this.applyEditorSessionForContext(context, await fetchEditorSession(context));
        return;
      }

      const snapshot = captureMutationSnapshot(this);
      try {
        const [projection, session] = await Promise.all([
          fetchProjection(context),
          fetchEditorSession(context),
        ]);
        if (!this.matchesCommandContext(context)) {
          return;
        }
        replaceProjection(this, projection);
        this.applyEditorSessionForContext(context, session);
      } catch (error) {
        if (this.matchesCommandContext(context)) {
          restoreMutationSnapshot(this, snapshot);
        }
        throw error;
      }
    },
    applyEditorSessionForContext(
      context: EditorCommandContext | null,
      info: EditorSessionInfo | null | undefined
    ) {
      if (context) {
        if (!this.matchesCommandContext(context)) {
          return;
        }
        this.applyEditorSession(info);
        return;
      }

      if (this.documentId !== null) {
        return;
      }
      if (!info) {
        this.clearDocument();
        return;
      }
      if (this.data !== null) {
        this.applyEditorSession(info);
      }
    },
    applyEditorSession(info: EditorSessionInfo | null | undefined) {
      if (!info) {
        this.clearDocument();
        return;
      }
      if (this.data === null) {
        return;
      }
      if (this.documentId !== null && info.documentId !== this.documentId) {
        return;
      }
      const revisionAdvancedWithoutProjection = info.revision > this.revision;
      this.documentId = info.documentId;
      this.revision = Math.max(this.revision, info.revision);
      applyEditorSessionStatus(info);
      if (revisionAdvancedWithoutProjection) {
        this.projectionStale = true;
        clearSearchSession();
      }
    },
  },
});
