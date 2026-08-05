import { ElMessage } from 'element-plus';

import * as api from '@/api';
import {
  createDocumentCommandCoordinator,
  type InteractiveMutationOptions,
} from '@/application/documentCommandCoordinator';
import type { RegionLoadPriority } from '@/application/documentRegionLoadScheduler';
import { searchOutcomeState } from '@/application/editorRuntimeProtocol';
import type { createDocumentSessionCoordinator } from '@/application/documentSessionCoordinator';
import type { createDocumentRegionCoordinator } from '@/application/documentRegionCoordinator';
import type { useDocumentSessionStore } from '@/stores/documentSession';
import type { useEditorSelectionStore } from '@/stores/editorSelection';
import type {
  EditorCommandContext,
  SheetRegion,
  U64String,
} from '@/types/documentRuntime';
import type { SearchOutcomeStateInput, SearchScope } from '@/types/editorRuntime';
import type { CellSaveRequest } from '@/types/pendingCellSave';
import type { ImageAnchor } from '@/types/documentRuntime';
import { appErrorMessage } from '@/utils/appError';
import {
  neverCancelled,
  type OperationCancellationSignal,
} from '@/application/operationCancellation';

type DocumentSessionCoordinator = ReturnType<typeof createDocumentSessionCoordinator> & Pick<
  ReturnType<typeof createDocumentRegionCoordinator>,
  'ensureSheetLoaded' | 'ensureSheetRegionLoaded'
>;
type DocumentSessionStore = ReturnType<typeof useDocumentSessionStore>;
type EditorSelectionStore = ReturnType<typeof useEditorSelectionStore>;

type InteractiveCommandOptions = {
  flushPendingChanges: () => Promise<boolean>;
  errorMessage: string;
  refreshProjectionOnError?: boolean;
  afterApplied?: () => void;
};

export type DocumentCommandBus = ReturnType<typeof createDocumentCommandBus>;

export function createDocumentCommandBus(
  document: DocumentSessionStore,
  session: DocumentSessionCoordinator,
  selection: EditorSelectionStore,
  cancellation: OperationCancellationSignal = neverCancelled,
) {
  const coordinator = createDocumentCommandCoordinator({
    document,
    session,
    transport: {
      getMutationResult: (documentId, commandId) =>
        api.getMutationResult(documentId, commandId),
      getActiveDocument: () => api.getActiveDocument(),
      getCurrentDocumentProjection: (context, preferredSheetIndex) =>
        api.getCurrentDocumentProjection(context, preferredSheetIndex),
      getEditorState: (context) => api.getEditorState(context),
      getSheetRegionProjection: (context, region) =>
        api.getSheetRegionProjection(context, region),
    },
    preferredSheetIndex: () => selection.currentSheetIndex,
    reportDiagnostic: (message, error) => console.error(`${message}:`, error),
    cancellation,
  });

  async function runInteractiveCommand(
    action: InteractiveMutationOptions['action'],
    {
      flushPendingChanges,
      errorMessage,
      refreshProjectionOnError,
      afterApplied,
    }: InteractiveCommandOptions,
  ): Promise<void> {
    const outcome = await coordinator.runInteractiveMutation({
      action,
      flushPendingChanges,
      refreshProjectionOnError,
      afterApplied,
    });
    if (outcome.status === 'failed') {
      ElMessage.error(`${errorMessage}: ${appErrorMessage(outcome.error)}`);
    } else if (outcome.status === 'refresh-failed') {
      ElMessage.error(
        `Change was applied, but the editor could not refresh: ${appErrorMessage(outcome.error)}`,
      );
    } else if (outcome.status === 'after-applied-failed') {
      console.error('Post-mutation UI update failed:', outcome.error);
      ElMessage.error(
        `Change was applied, but the editor UI could not update: ${appErrorMessage(outcome.error)}`,
      );
    }
  }

  function addRow(sheetIndex: number, rowIndex: number, options: InteractiveCommandOptions) {
    return runInteractiveCommand(
      (context) => api.addRow(context, sheetIndex, rowIndex),
      options,
    );
  }

  function deleteRow(sheetIndex: number, rowIndex: number, options: InteractiveCommandOptions) {
    return runInteractiveCommand(
      (context) => api.deleteRow(context, sheetIndex, rowIndex),
      options,
    );
  }

  function addColumn(sheetIndex: number, colIndex: number, options: InteractiveCommandOptions) {
    return runInteractiveCommand(
      (context) => api.addColumn(context, sheetIndex, colIndex),
      options,
    );
  }

  function deleteColumn(sheetIndex: number, colIndex: number, options: InteractiveCommandOptions) {
    return runInteractiveCommand(
      (context) => api.deleteColumn(context, sheetIndex, colIndex),
      options,
    );
  }

  function addSheet(options: InteractiveCommandOptions) {
    return runInteractiveCommand((context) => api.addSheet(context), options);
  }

  function deleteSheet(sheetIndex: number, options: InteractiveCommandOptions) {
    return runInteractiveCommand((context) => api.deleteSheet(context, sheetIndex), options);
  }

  function undo(options: InteractiveCommandOptions) {
    return runInteractiveCommand((context) => api.undo(context), options);
  }

  function redo(options: InteractiveCommandOptions) {
    return runInteractiveCommand((context) => api.redo(context), options);
  }

  function setColumnWidth(
    sheetIndex: number,
    colIndex: number,
    width: number | null,
    options: InteractiveCommandOptions,
  ) {
    return runInteractiveCommand(
      (context) => api.setColumnWidth(context, sheetIndex, colIndex, width),
      options,
    );
  }

  function setRowHeight(
    sheetIndex: number,
    rowIndex: number,
    height: number | null,
    options: InteractiveCommandOptions,
  ) {
    return runInteractiveCommand(
      (context) => api.setRowHeight(context, sheetIndex, rowIndex, height),
      options,
    );
  }

  function insertImage(
    sheetIndex: number,
    row: number,
    col: number,
    selectionToken: string,
    options: InteractiveCommandOptions,
  ) {
    return runInteractiveCommand(
      (context) => api.insertImage(context, sheetIndex, row, col, selectionToken),
      options,
    );
  }

  function updateImage(
    sheetIndex: number,
    imageId: string,
    anchor: ImageAnchor,
    options: InteractiveCommandOptions,
  ) {
    return runInteractiveCommand(
      (context) => api.updateImage(context, sheetIndex, imageId, anchor),
      options,
    );
  }

  function deleteImage(
    sheetIndex: number,
    imageId: string,
    options: InteractiveCommandOptions,
  ) {
    return runInteractiveCommand(
      (context) => api.deleteImage(context, sheetIndex, imageId),
      options,
    );
  }

  async function setCells(
    documentId: U64String,
    changes: CellSaveRequest[],
    onRefreshFailed?: (error: unknown) => void,
  ): Promise<void> {
    const payload = changes.map(({ sheetIndex, row, col, value }) => ({
      sheetIndex,
      row,
      col,
      text: value,
    }));
    const outcome = await coordinator.runBackgroundMutation({
      documentId,
      action: (context) => api.setCells(context, payload),
    });
    if (outcome.status === 'refresh-failed') onRefreshFailed?.(outcome.error);
  }

  async function search(
    query: string,
    scope: SearchScope,
    currentSheetIndex: number,
    flushPendingChanges: () => Promise<boolean>,
  ): Promise<SearchOutcomeStateInput | undefined> {
    const response = await coordinator.runConsistentRead({
      flushPendingChanges,
      lockInteraction: true,
      action: (context) => api.search(
        context,
        query,
        scope,
        scope === 'currentSheet' ? currentSheetIndex : null,
      ),
    });
    return response ? searchOutcomeState(response) : undefined;
  }

  async function ensureSheetLoaded(
    sheetIndex: number,
    flushPendingChanges: () => Promise<boolean>,
  ): Promise<boolean> {
    try {
      return await coordinator.ensureSheetLoaded(sheetIndex, flushPendingChanges);
    } catch (error) {
      ElMessage.error(`Failed to load sheet: ${appErrorMessage(error)}`);
      return false;
    }
  }

  async function ensureSheetRegionLoaded(
    region: SheetRegion,
    options: { priority?: RegionLoadPriority } = {},
  ): Promise<boolean> {
    try {
      return await coordinator.ensureSheetRegionLoaded(region, options);
    } catch (error) {
      console.error('Failed to load sheet viewport:', error);
      return false;
    }
  }

  async function refreshEditorState(): Promise<void> {
    const outcome = await coordinator.refreshEditorState();
    if (outcome.status === 'failed') throw outcome.error;
  }

  function prepareConsistentContext(
    flushPendingChanges: () => Promise<boolean>,
  ): Promise<EditorCommandContext | undefined> {
    return coordinator.prepareConsistentContext(flushPendingChanges);
  }

  return {
    addRow,
    deleteRow,
    addColumn,
    deleteColumn,
    addSheet,
    deleteSheet,
    undo,
    redo,
    setColumnWidth,
    setRowHeight,
    insertImage,
    updateImage,
    deleteImage,
    setCells,
    search,
    refreshAfterMutationError: coordinator.refreshAfterMutationError,
    refreshEditorState,
    ensureSheetLoaded,
    ensureSheetRegionLoaded,
    prepareConsistentContext,
  };
}
