const mutationQueues = new Map<number, Promise<void>>();

export function enqueueEditorMutation<T>(scope: number, task: () => Promise<T>): Promise<T> {
  const tail = mutationQueues.get(scope) ?? Promise.resolve();
  const run = tail.then(task, task);
  mutationQueues.set(scope, run.then(
    () => undefined,
    () => undefined
  ));
  return run;
}

export function waitForEditorMutations(scope: number): Promise<void> {
  return mutationQueues.get(scope) ?? Promise.resolve();
}

export function resetEditorMutationQueue(scope: number): void {
  mutationQueues.delete(scope);
}
