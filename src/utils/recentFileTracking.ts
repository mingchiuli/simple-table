import * as api from "@/api";
import type { EditorCommandContext, StorageType } from "@/types";

export type RecentFileTrackingRequest = {
  path: string;
  fileName: string;
  storageType: StorageType;
  originalPath?: string;
  context?: EditorCommandContext | null;
};

export type RecentFileTrackingInput = Omit<RecentFileTrackingRequest, "storageType">;

export function warnRecentFileTrackingFailure(error: unknown) {
  console.warn("Failed to update recent file metadata", error);
}

export async function tryAddRecentFileWithThumbnail({
  path,
  fileName,
  storageType,
  originalPath,
  context,
}: RecentFileTrackingRequest): Promise<boolean> {
  try {
    const fileSize = await api.getFileSize(path);
    await api.addRecentFileWithThumbnail(
      path,
      fileName,
      fileSize,
      storageType,
      originalPath,
      context
    );
    return true;
  } catch (error) {
    warnRecentFileTrackingFailure(error);
    return false;
  }
}

export async function tryAddRecentFileWithResolvedStorage(
  request: RecentFileTrackingInput,
  resolveStorageType: () => Promise<StorageType>
): Promise<boolean> {
  try {
    return await tryAddRecentFileWithThumbnail({
      ...request,
      storageType: await resolveStorageType(),
    });
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
