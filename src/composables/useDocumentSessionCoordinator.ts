import { createDocumentSessionCoordinator } from '@/application/documentSessionCoordinator';
import { createDocumentRegionCoordinator } from '@/application/documentRegionCoordinator';
import { useDocumentSessionStore } from '@/stores/documentSession';
import { useDocumentStatusStore } from '@/stores/documentStatus';
import { useEditorSelectionStore } from '@/stores/editorSelection';
import { usePendingCellSaveCoordinator } from '@/composables/usePendingCellSaveCoordinator';
import { useSearchSessionCoordinator } from '@/composables/useSearchSessionCoordinator';

type DocumentSessionCoordinator = ReturnType<typeof createDocumentSessionCoordinator> & Pick<
  ReturnType<typeof createDocumentRegionCoordinator>,
  'ensureSheetLoaded' | 'ensureSheetRegionLoaded'
>;

const coordinators = new WeakMap<object, DocumentSessionCoordinator>();

export function useDocumentSessionCoordinator() {
  const document = useDocumentSessionStore();
  const existing = coordinators.get(document);
  if (existing) return existing;
  const regions = createDocumentRegionCoordinator(document);
  const session = createDocumentSessionCoordinator({
    document,
    status: useDocumentStatusStore(),
    selection: useEditorSelectionStore(),
    pending: usePendingCellSaveCoordinator(),
    search: useSearchSessionCoordinator(),
    regions,
  });
  const coordinator: DocumentSessionCoordinator = {
    ...session,
    ensureSheetLoaded: regions.ensureSheetLoaded,
    ensureSheetRegionLoaded: regions.ensureSheetRegionLoaded,
  };
  coordinators.set(document, coordinator);
  return coordinator;
}
