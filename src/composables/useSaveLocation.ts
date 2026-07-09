import { discardSaveLocation, pickSaveLocation } from "@/platform";

type ReservedSaveLocation = {
  path: string;
  /** Mark the target as physically written; after this point cleanup must not remove it. */
  markPersisted: () => void;
};

export function useSaveLocation() {
  async function withReservedSaveLocation<T>(
    defaultName: string,
    action: (location: ReservedSaveLocation) => Promise<T>
  ): Promise<T | null> {
    const path = await pickSaveLocation(defaultName);
    if (!path) return null;

    let shouldDiscard = true;
    try {
      return await action({
        path,
        markPersisted: () => {
          shouldDiscard = false;
        },
      });
    } finally {
      if (shouldDiscard) {
        await discardSaveLocation(path);
      }
    }
  }

  return {
    withReservedSaveLocation,
  };
}
