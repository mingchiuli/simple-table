import * as api from "@/api";
import type { DocumentCapabilities } from "@/types";

export async function documentCapabilities(
  fileName: string,
  currentPath: string | null
): Promise<DocumentCapabilities> {
  return api.getDocumentCapabilities(fileName, currentPath);
}
