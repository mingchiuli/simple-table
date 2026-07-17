import { createDocumentSessionCoordinator } from '@/application/documentSessionCoordinator';
import { useDocumentSessionStore } from '@/stores/documentSession';
import { useDocumentStatusStore } from '@/stores/documentStatus';
import { useEditorSelectionStore } from '@/stores/editorSelection';
import { usePendingCellSavesStore } from '@/stores/pendingCellSaves';
import { useSearchSessionStore } from '@/stores/searchSession';

type DocumentSessionCoordinator = ReturnType<typeof createDocumentSessionCoordinator>;

const coordinators = new WeakMap<object, DocumentSessionCoordinator>();

export function useDocumentSessionCoordinator() {
  const document = useDocumentSessionStore();
  const existing = coordinators.get(document);
  if (existing) return existing;
  const coordinator = createDocumentSessionCoordinator({
    document,
    status: useDocumentStatusStore(),
    selection: useEditorSelectionStore(),
    pending: usePendingCellSavesStore(),
    search: useSearchSessionStore(),
  });
  coordinators.set(document, coordinator);
  return coordinator;
}
