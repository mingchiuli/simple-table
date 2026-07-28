import type { ApplicationExitPreparation } from '@/application/applicationExitCoordinator';
import type {
  DocumentLifecycleRunner,
  DocumentReplacementLease,
} from '@/application/documentFileWorkflow';
import type { EditorCommandContext } from '@/types/documentRuntime';

export type DocumentCloseWorkflowPorts = {
  getCommandContext: () => EditorCommandContext | null;
  beginDocumentReplacement: () => Promise<DocumentReplacementLease | null>;
  runDocumentLifecycle: DocumentLifecycleRunner;
  closeDocument: (context: EditorCommandContext) => Promise<void>;
  clearDocument: () => void;
};

export function createDocumentCloseWorkflow(ports: DocumentCloseWorkflowPorts) {
  async function closeCurrentDocument(options: { waitForIdle?: boolean } = {}): Promise<boolean> {
    let closed = false;
    const lifecycleStatus = await ports.runDocumentLifecycle(
      'closing',
      async () => {
        const replacement = await ports.beginDocumentReplacement();
        if (!replacement) return;
        const context = ports.getCommandContext();
        if (!context) {
          replacement.commit();
          ports.clearDocument();
          closed = true;
          return;
        }
        try {
          await ports.closeDocument(context);
        } catch (error) {
          replacement.cancel();
          throw error;
        }
        replacement.commit();
        ports.clearDocument();
        closed = true;
      },
      { waitForIdle: options.waitForIdle },
    );
    return lifecycleStatus === 'completed' && closed;
  }

  async function prepareApplicationExit(
    options: { waitForIdle?: boolean } = {},
  ): Promise<ApplicationExitPreparation | null> {
    let preparation: ApplicationExitPreparation | null = null;
    const lifecycleStatus = await ports.runDocumentLifecycle(
      'closing',
      async ({ retain }) => {
        const replacement = await ports.beginDocumentReplacement();
        if (!replacement) return;
        const lifecycleLease = retain();
        let settled = false;
        const settle = (action: () => void) => {
          if (settled) return;
          settled = true;
          try {
            action();
          } finally {
            lifecycleLease.release();
          }
        };
        preparation = {
          commit: () => settle(replacement.commit),
          rollback: () => settle(replacement.cancel),
        };
      },
      { waitForIdle: options.waitForIdle },
    );
    return lifecycleStatus === 'completed' ? preparation : null;
  }

  return { closeCurrentDocument, prepareApplicationExit };
}
