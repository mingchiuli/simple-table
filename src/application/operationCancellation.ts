export type OperationCancellationSignal = {
  isCancelled(): boolean;
  onCancel(handler: () => void): () => void;
};

export const neverCancelled: OperationCancellationSignal = {
  isCancelled: () => false,
  onCancel: () => () => undefined,
};
