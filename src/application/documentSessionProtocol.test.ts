import { describe, expect, it } from 'vitest';

import {
  interpretMutationResponse,
  openSessionState,
  recoveredSessionState,
  savedSessionState,
  type DocumentProtocolState,
} from '@/application/documentSessionProtocol';
import {
  defaultHistoryStatus,
  defaultRichProjection,
  defaultWorkbookCapabilities,
  readyFormulaStatus,
} from '@/types';
import type { EditorMutationResponse, EditorSessionInfo } from '@/types/protocol';
import { openResponseFromFileData } from '@/test/documentFixtures';

describe('documentSessionProtocol', () => {
  it('distinguishes document replacement from in-flight recovery', () => {
    const opened = response('1');
    const openState = openSessionState(opened, '/tmp/book.xlsx');

    expect(openState.resetEditorCommandDepth).toBe(true);

    const recovered = recoveredSessionState(protocolState(openState), response('2'));
    expect(recovered?.resetEditorCommandDepth).toBe(false);
  });

  it('preserves resident sheet order for identity-only saves', () => {
    const opened = openSessionState(response('1'), '/tmp/book.xlsx');
    const saved = savedSessionState(protocolState(opened), {
      identity: { path: '/tmp/saved.xlsx', fileName: 'saved.xlsx' },
      editorSession: session('2'),
    });

    expect(saved.data.fileName).toBe('saved.xlsx');
    expect(saved.preserveResidentSheetOrder).toBe(true);
    expect(saved.activatePreferredSheet).toBe(false);
  });

  it('requires a resync when a mutation skips revisions', () => {
    const opened = openSessionState(response('1'));
    const mutation = mutationResponse('3');

    const result = interpretMutationResponse(protocolState(opened), mutation);

    expect(result.status).toBe('accepted');
    if (result.status === 'accepted') {
      expect(result.state.resyncRequired).toBe(true);
      expect(result.state.data).toBe(opened.data);
    }
  });

  it('rejects unsupported mutation protocol versions before state changes', () => {
    const opened = openSessionState(response('1'));
    const mutation = {
      ...mutationResponse('2'),
      protocolVersion: 999,
    } as unknown as EditorMutationResponse;

    expect(() => interpretMutationResponse(protocolState(opened), mutation)).toThrow(
      'Unsupported editor mutation protocol',
    );
  });
});

function response(revision: `${bigint}`) {
  return openResponseFromFileData({
    path: '/tmp/book.xlsx',
    fileName: 'book.xlsx',
    sheets: [{
      name: 'Sheet1',
      rows: [],
      merges: [],
      rich: defaultRichProjection(),
    }],
  }, session(revision));
}

function session(revision: `${bigint}`): EditorSessionInfo {
  return {
    documentId: '1',
    revision,
    formulaStatus: readyFormulaStatus(),
    capabilities: defaultWorkbookCapabilities(),
    editorState: {
      canUndo: false,
      canRedo: false,
      isDirty: false,
      history: defaultHistoryStatus(),
    },
  };
}

function mutationResponse(revision: `${bigint}`): EditorMutationResponse {
  return {
    protocolVersion: 4,
    documentId: '1',
    revision,
    formulaStatus: readyFormulaStatus(),
    capabilities: defaultWorkbookCapabilities(),
    editorState: {
      canUndo: true,
      canRedo: false,
      isDirty: true,
      history: defaultHistoryStatus(),
    },
  };
}

function protocolState(
  state: ReturnType<typeof openSessionState>,
): DocumentProtocolState {
  return {
    data: state.data,
    currentFilePath: state.currentFilePath,
    documentId: state.documentId,
    revision: state.revision,
  };
}
