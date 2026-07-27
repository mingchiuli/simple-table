export type DocumentLifecycle = 'loading' | 'saving' | 'closing';
export type DocumentLifecycleStatus = 'completed' | 'skipped';

export type DocumentReplacementLease = {
  commit: () => void;
  cancel: () => void;
};

export type DocumentLifecycleController = {
  release: () => void;
  retain: () => DocumentLifecycleLease;
};

export type DocumentLifecycleLease = {
  release: () => void;
};

export type DocumentLifecycleOptions = {
  waitForIdle?: boolean;
  shouldContinue?: () => boolean;
};

export type DocumentLifecycleRunner = (
  lifecycle: DocumentLifecycle,
  action: (controller: DocumentLifecycleController) => Promise<void>,
  options?: DocumentLifecycleOptions,
) => Promise<DocumentLifecycleStatus>;

export type OpenFileSelection = {
  path: string;
  fileName: string;
  originalPath?: string;
};

export type ReservedSaveLocation = {
  path: string;
  markPersisted: () => void;
};
