import * as api from "@/api";
import type { EditorCommandContext } from "@/types";

export type RecentFileTrackingRequest = {
  originalPath?: string;
  context: EditorCommandContext;
};

export function warnRecentFileTrackingFailure(error: unknown) {
  console.warn("Failed to update recent file metadata", error);
}

export async function tryAddRecentFileWithThumbnail({
  originalPath,
  context,
}: RecentFileTrackingRequest): Promise<boolean> {
  try {
    await api.addRecentFileWithThumbnail(context, originalPath);
    return true;
  } catch (error) {
    warnRecentFileTrackingFailure(error);
    return false;
  }
}

export async function tryRefreshRecentFiles(refresh: () => Promise<void>): Promise<boolean> {
  try {
    await refresh();
    return true;
  } catch (error) {
    warnRecentFileTrackingFailure(error);
    return false;
  }
}
