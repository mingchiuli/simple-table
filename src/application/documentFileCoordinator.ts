import {
  createDocumentCloseWorkflow,
  type DocumentCloseWorkflowPorts,
} from '@/application/documentCloseWorkflow';
import {
  createDocumentOpenWorkflow,
  type DocumentOpenWorkflowPorts,
} from '@/application/documentOpenWorkflow';
import {
  createDocumentPersistenceWorkflow,
  type DocumentPersistenceWorkflowPorts,
} from '@/application/documentPersistenceWorkflow';
import type { DocumentPreparationCoordinator } from '@/application/documentPreparationCoordinator';

export type {
  ExportFileOutcome,
  SaveFileOutcome,
} from '@/application/documentPersistenceWorkflow';

export type DocumentFileCoordinatorPorts<ActiveDocument, SavedDocument> =
  & DocumentOpenWorkflowPorts<ActiveDocument>
  & DocumentPersistenceWorkflowPorts<ActiveDocument, SavedDocument>
  & DocumentCloseWorkflowPorts;

export function createDocumentFileCoordinator<ActiveDocument, SavedDocument>(
  ports: DocumentFileCoordinatorPorts<ActiveDocument, SavedDocument>,
  preparations?: DocumentPreparationCoordinator,
) {
  return {
    ...createDocumentOpenWorkflow(ports, preparations),
    ...createDocumentPersistenceWorkflow(ports),
    ...createDocumentCloseWorkflow(ports),
  };
}
