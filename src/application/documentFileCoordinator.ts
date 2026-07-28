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
import { createDocumentCloseOperation } from '@/application/documentCloseOperation';
import type { FileOperationReceipt } from '@/types/fileRuntime';

export type {
  ExportFileOutcome,
  SaveFileOutcome,
} from '@/application/documentPersistenceWorkflow';

type DocumentFileWorkflowPorts<ActiveDocument, SavedDocument> =
  & DocumentOpenWorkflowPorts<ActiveDocument>
  & DocumentPersistenceWorkflowPorts<ActiveDocument, SavedDocument>
  & DocumentCloseWorkflowPorts;

export type DocumentFileCoordinatorPorts<ActiveDocument, SavedDocument> =
  & Omit<DocumentFileWorkflowPorts<ActiveDocument, SavedDocument>, 'closeDocument'>
  & {
    commitCloseDocument: (
      context: Parameters<DocumentCloseWorkflowPorts['closeDocument']>[0],
      operationId: string,
    ) => Promise<FileOperationReceipt>;
  };

export function createDocumentFileCoordinator<ActiveDocument, SavedDocument>(
  ports: DocumentFileCoordinatorPorts<ActiveDocument, SavedDocument>,
  preparations?: DocumentPreparationCoordinator,
) {
  const closeDocument = createDocumentCloseOperation(ports);
  const workflowPorts: DocumentFileWorkflowPorts<ActiveDocument, SavedDocument> = {
    ...ports,
    closeDocument,
  };
  return {
    ...createDocumentOpenWorkflow(workflowPorts, preparations),
    ...createDocumentPersistenceWorkflow(workflowPorts),
    ...createDocumentCloseWorkflow(workflowPorts),
  };
}
