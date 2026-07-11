import * as api from "@/api";
import type { DocumentCapabilities, EditorCommandContext, NativeSavePlan } from "@/types";

export async function documentCapabilities(
  context: EditorCommandContext
): Promise<DocumentCapabilities> {
  return api.getDocumentCapabilities(context);
}

export async function nativeSavePlan(
  context: EditorCommandContext,
  targetPathOrName: string
): Promise<NativeSavePlan> {
  return api.getNativeSavePlan(context, targetPathOrName);
}
