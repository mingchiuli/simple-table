import type { CellValue } from './documentRuntime';

export type CellSaveRequest = {
  sheetIndex: number;
  row: number;
  col: number;
  value: string;
  oldValue: CellValue;
};

export type CellSaveState = {
  key: string;
  draft?: string;
  queued?: CellSaveRequest;
  active?: CellSaveRequest;
};

export type PendingCellSavePhase = 'idle' | 'debouncing' | 'saving' | 'failed';

export type QueueDraftResult = {
  queued: boolean;
  shouldMarkPending: boolean;
  shouldClearPendingIfIdle: boolean;
};

export type PendingCellSaveCallbacks = {
  commitBatch: (changes: CellSaveRequest[]) => Promise<void>;
  clearPendingContentChange: () => void;
  onBatchCommitted?: () => void;
  onCommitFailed?: (error: unknown) => Promise<void> | void;
};
