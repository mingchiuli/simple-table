let mutationTail: Promise<void> = Promise.resolve();

export function enqueueEditorMutation<T>(task: () => Promise<T>): Promise<T> {
  const run = mutationTail.then(task, task);
  mutationTail = run.then(
    () => undefined,
    () => undefined
  );
  return run;
}

export function waitForEditorMutations(): Promise<void> {
  return mutationTail;
}
