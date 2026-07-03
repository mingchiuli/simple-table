const mutationQueues = new Map<number, Promise<void>>();

export function enqueueEditorMutation<T>(scope: number, task: () => Promise<T>): Promise<T> {
  const tail = mutationQueues.get(scope) ?? Promise.resolve();
  const run = tail.then(task, task);
  const cleanup = run.then(
    () => undefined,
    () => undefined
  );
  mutationQueues.set(scope, cleanup);
  cleanup.finally(() => {
    if (mutationQueues.get(scope) === cleanup) {
      mutationQueues.delete(scope);
    }
  });
  return run;
}

export function waitForEditorMutations(scope: number): Promise<void> {
  return mutationQueues.get(scope) ?? Promise.resolve();
}

export function resetEditorMutationQueue(scope: number): void {
  mutationQueues.delete(scope);
}
