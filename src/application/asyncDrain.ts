export type AsyncDrainTask = () => PromiseLike<unknown> | unknown;

export async function drainAllSettled(
  tasks: readonly AsyncDrainTask[],
  message: string,
): Promise<void> {
  const results = await Promise.allSettled(tasks.map(async (task) => task()));
  const failures = results.flatMap((result) => (
    result.status === 'rejected' ? [result.reason] : []
  ));
  if (failures.length > 0) throw new AggregateError(failures, message);
}
