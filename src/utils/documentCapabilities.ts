import * as api from "@/api";
import type { DocumentCapabilities, NativeSavePlan } from "@/types";

export async function documentCapabilities(
  fileName: string,
  currentPath: string | null
): Promise<DocumentCapabilities> {
  return api.getDocumentCapabilities(fileName, currentPath);
}

export async function nativeSavePlan(targetPathOrName: string): Promise<NativeSavePlan> {
  return api.getNativeSavePlan(targetPathOrName);
}
