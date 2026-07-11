import type {
  EditorMutationResponse,
  CellValue,
  EditorSessionInfo,
  DocumentProjection,
  FileData,
  OpenDocumentResponse,
  SavedDocumentResponse,
  EditorCommandContext,
  SheetExtent,
  SheetData,
  SheetProjectionResponse,
  SheetRegion,
  SheetRegionProjectionResponse,
  U64String,
} from "@/types";
import {
  applyProjectionPatches,
  createDocumentProjection,
} from "@/stores/documentProjection";
import { compareU64, isNextU64, maxU64, ZERO_U64 } from "@/utils/u64";
import { blankCell } from "@/utils/cellValue";
import { useEditorSelectionStore } from "@/stores/editorSelection";
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
  data: DocumentProjection | null;
  resyncRequired: boolean;
  applied: boolean;
};

export const useDocumentSessionStore = defineStore("documentSession", {
  state: () => ({
    data: null as DocumentProjection | null,
    currentFilePath: null as string | null,
    documentId: null as U64String | null,
    revision: ZERO_U64,
    lifecycle: "idle" as DocumentSessionLifecycle,
    editorCommandDepth: 0,
    projectionStale: false,
    residentSheetOrder: [] as number[],
  }),
  getters: {
    isInteractionLocked: (state) => state.lifecycle !== "idle" || state.editorCommandDepth > 0,
    isEditorInteractionLocked: (state) =>
      state.lifecycle !== "idle" || state.projectionStale || state.editorCommandDepth > 0,
    sheetExtents: (state): SheetExtent[] => state.data?.sheets.map((slot) => slot.extent) ?? [],
    loadedSheetIndexes: (state): number[] => state.data?.sheets
      .map((slot, index) => slot.state === 'loaded' ? index : -1)
      .filter((index) => index >= 0) ?? [],
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
      documentId: U64String,
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
    commandContextForDocument(documentId: U64String): EditorCommandContext | null {
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
      this.data = createDocumentProjection(
        response.fileData,
        response.sheetExtents,
        response.loadedSheetIndexes,
        response.loadedSheetRegions
      );
      this.residentSheetOrder = this.loadedSheetIndexes;
      this.currentFilePath = path !== null ? path : response.fileData.path || null;
      this.documentId = response.editorSession.documentId;
      this.revision = response.editorSession.revision;
      resetSessionEditorCommands(this);
      this.projectionStale = false;
      resetSessionUi();
      this.enforceResidentSheetBudget();
      resetDocumentStatus();
      applyEditorSessionStatus(response.editorSession);
    },
    applySavedDocumentResponse(response: SavedDocumentResponse, path: string | null = null) {
      if (!response.fileData && (!response.identity || !this.data)) {
        throw new Error("Saved document response did not include projection or identity data");
      }
      resetTransientDocumentWork(this);
      if (response.fileData) {
        const resident = this.residentSheetOrder.length
          ? this.residentSheetOrder
          : [useEditorSelectionStore().currentSheetIndex];
        this.data = createDocumentProjection(response.fileData, undefined, resident);
      } else if (this.data && response.identity) {
        this.data = {
          ...this.data,
          path: response.identity.path,
          fileName: response.identity.fileName,
        };
      }
      const responsePath = response.fileData?.path ?? response.identity?.path;
      this.currentFilePath = path !== null ? path : responsePath || null;
      this.documentId = response.editorSession.documentId;
      this.revision = response.editorSession.revision;
      this.projectionStale = false;
      this.enforceResidentSheetBudget();
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
        || compareU64(response.editorSession.revision, context.baseRevision) < 0
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
      this.revision = ZERO_U64;
      resetSessionEditorCommands(this);
      this.projectionStale = false;
      this.residentSheetOrder = [];
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
      if (compareU64(response.revision, this.revision) < 0) {
        return { data: this.data, resyncRequired: false, applied: false };
      }
      if (
        compareU64(response.revision, this.revision) > 0
        && !isNextU64(response.revision, this.revision)
      ) {
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
        const result = applyProjectionPatches(
          this.data,
          response.patches,
          response.sheetExtents
        );
        this.data = result.data;
        this.residentSheetOrder = this.residentSheetOrder
          .filter((index) => this.isSheetLoaded(index));
        for (const index of this.loadedSheetIndexes) {
          if (!this.residentSheetOrder.includes(index)) this.residentSheetOrder.push(index);
        }
        this.enforceResidentSheetBudget();
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
    isSheetLoaded(sheetIndex: number): boolean {
      return this.data?.sheets[sheetIndex]?.state === 'loaded';
    },
    touchResidentSheet(sheetIndex: number) {
      this.residentSheetOrder = [
        ...this.residentSheetOrder.filter((index) => index !== sheetIndex),
        sheetIndex,
      ];
      this.enforceResidentSheetBudget();
    },
    enforceResidentSheetBudget() {
      const maximumResidentSheets = 4;
      if (!this.data) return;
      const protectedSheet = useEditorSelectionStore().currentSheetIndex;
      const sheets = [...this.data.sheets];
      while (this.residentSheetOrder.length > maximumResidentSheets) {
        const candidatePosition = this.residentSheetOrder.findIndex(
          (index) => index !== protectedSheet
        );
        if (candidatePosition < 0) break;
        const [evicted] = this.residentSheetOrder.splice(candidatePosition, 1);
        const slot = sheets[evicted];
        if (slot?.state === 'loaded') {
          sheets[evicted] = {
            state: 'unloaded',
            name: slot.name,
            extent: slot.extent,
          };
        }
      }
      this.residentSheetOrder = [...this.residentSheetOrder];
      this.data = { ...this.data, sheets };
    },
    loadedSheet(sheetIndex: number): SheetData | null {
      const slot = this.data?.sheets[sheetIndex];
      return slot?.state === 'loaded' ? slot.data : null;
    },
    async ensureSheetLoaded(
      sheetIndex: number,
      fetchProjection: (
        context: EditorCommandContext,
        sheetIndex: number
      ) => Promise<SheetProjectionResponse>
    ): Promise<boolean> {
      if (this.isSheetLoaded(sheetIndex)) {
        this.touchResidentSheet(sheetIndex);
        return true;
      }
      const context = this.currentCommandContext();
      if (!context || !this.data?.sheets[sheetIndex]) return false;
      const response = await fetchProjection(context, sheetIndex);
      if (
        !this.matchesCommandContext(context)
        || response.documentId !== context.documentId
        || response.revision !== context.baseRevision
        || response.sheetIndex !== sheetIndex
      ) {
        return false;
      }
      const sheets = [...this.data.sheets];
      sheets[sheetIndex] = {
        state: 'loaded',
        name: response.sheet.name,
        extent: response.extent,
        data: response.sheet,
        regions: [response.loadedRegion],
      };
      this.data = { ...this.data, sheets };
      this.touchResidentSheet(sheetIndex);
      return true;
    },
    async ensureSheetRegionLoaded(
      region: SheetRegion,
      fetchProjection: (
        context: EditorCommandContext,
        region: SheetRegion
      ) => Promise<SheetRegionProjectionResponse>
    ): Promise<boolean> {
      const slot = this.data?.sheets[region.sheetIndex];
      if (!slot || slot.state !== 'loaded') return false;
      if (slot.regions.some((loaded) => containsRegion(loaded, region))) return true;
      const context = this.currentCommandContext();
      if (!context) return false;
      const response = await fetchProjection(context, region);
      if (
        !this.matchesCommandContext(context)
        || response.documentId !== context.documentId
        || response.revision !== context.baseRevision
        || response.region.sheetIndex !== region.sheetIndex
      ) {
        return false;
      }
      const data = this.data;
      const current = data?.sheets[region.sheetIndex];
      if (!current || current.state !== 'loaded') return false;
      const rows = mergeRegionCells(current.data.rows, response.cells);
      const sheets = [...data.sheets];
      sheets[region.sheetIndex] = {
        ...current,
        data: { ...current.data, rows },
        regions: [...current.regions, response.region],
      };
      this.data = { ...data, sheets };
      this.touchResidentSheet(region.sheetIndex);
      return true;
    },
    markProjectionStaleFromMutationResponse(response: EditorMutationResponse): boolean {
      if (this.documentId !== null && response.documentId !== this.documentId) {
        return false;
      }
      if (this.documentId === null && this.data === null) {
        return false;
      }
      if (compareU64(response.revision, this.revision) < 0) {
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
      const revisionAdvancedWithoutProjection = compareU64(info.revision, this.revision) > 0;
      this.documentId = info.documentId;
      this.revision = maxU64(this.revision, info.revision);
      applyEditorSessionStatus(info);
      if (revisionAdvancedWithoutProjection) {
        this.projectionStale = true;
        clearSearchSession();
      }
    },
  },
});

function containsRegion(loaded: SheetRegion, requested: SheetRegion): boolean {
  return loaded.sheetIndex === requested.sheetIndex
    && loaded.rowStart <= requested.rowStart
    && loaded.rowEnd >= requested.rowEnd
    && loaded.colStart <= requested.colStart
    && loaded.colEnd >= requested.colEnd;
}

function mergeRegionCells(
  currentRows: CellValue[][],
  cells: SheetRegionProjectionResponse['cells']
): CellValue[][] {
  const rows = [...currentRows];
  for (const cell of cells) {
    while (rows.length <= cell.row) rows.push([]);
    const row = [...(rows[cell.row] ?? [])];
    while (row.length < cell.col) row.push(blankCell());
    row[cell.col] = cell.value;
    rows[cell.row] = row;
  }
  return rows;
}
