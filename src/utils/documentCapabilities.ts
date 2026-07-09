import * as api from "@/api";
import type { DocumentCapabilities, EditorCommandContext, NativeSavePlan } from "@/types";

export async function documentCapabilities(
  context: EditorCommandContext,
  fileName: string,
  currentPath: string | null
): Promise<DocumentCapabilities> {
  return api.getDocumentCapabilities(context, fileName, currentPath);
}

export async function nativeSavePlan(
  context: EditorCommandContext,
  targetPathOrName: string
): Promise<NativeSavePlan> {
  return api.getNativeSavePlan(context, targetPathOrName);
}
