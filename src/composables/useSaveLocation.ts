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
    } catch (error) {
      if (shouldDiscard) {
        await discardReservedSaveLocation(path, error);
        shouldDiscard = false;
      }
      throw error;
    } finally {
      if (shouldDiscard) {
        await discardReservedSaveLocation(path);
      }
    }
  }

  return {
    withReservedSaveLocation,
  };
}

async function discardReservedSaveLocation(path: string, originalError?: unknown) {
  try {
    await discardSaveLocation(path);
  } catch (cleanupError) {
    if (originalError !== undefined) {
      console.error("Failed to discard reserved save location after action error:", cleanupError);
      return;
    }
    throw cleanupError;
  }
}
