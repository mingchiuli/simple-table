export type AsyncCache<T> = {
  get(): Promise<T>;
  clear(): void;
};

export function createAsyncCache<T>(load: () => Promise<T>): AsyncCache<T> {
  let cached: Promise<T> | null = null;

  return {
    get() {
      cached ??= load().catch((error) => {
        cached = null;
        throw error;
      });
      return cached;
    },
    clear() {
      cached = null;
    },
  };
}
