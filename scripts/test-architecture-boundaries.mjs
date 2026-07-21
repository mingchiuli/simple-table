import { readFileSync, readdirSync } from 'node:fs';
import { extname, join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

const projectRoot = fileURLToPath(new URL('..', import.meta.url));
const violations = [];

function sourceFiles(directory, extension) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return sourceFiles(path, extension);
    if (extname(entry.name) !== extension || entry.name.includes('.test.')) return [];
    return [path];
  });
}

function rejectMatches(files, patterns, boundary) {
  for (const file of files) {
    const source = readFileSync(file, 'utf8');
    for (const pattern of patterns) {
      if (pattern.test(source)) {
        violations.push(`${relative(projectRoot, file)} violates ${boundary}: ${pattern}`);
      }
    }
  }
}

function rejectRustProductionMatches(files, patterns, boundary) {
  for (const file of files) {
    const source = readFileSync(file, 'utf8').split(/\n#\[cfg\(test\)\]\nmod tests\s*\{/)[0];
    for (const pattern of patterns) {
      if (pattern.test(source)) {
        violations.push(`${relative(projectRoot, file)} violates ${boundary}: ${pattern}`);
      }
    }
  }
}

rejectMatches(
  sourceFiles(join(projectRoot, 'src', 'stores'), '.ts'),
  [
    /['"]@\/api['"]/,
    /['"]@\/tauriInvoke['"]/,
    /['"]@\/platform(?:\/|['"])/,
    /['"]@\/composables(?:\/|['"])/,
    /['"]@\/application(?:\/|['"])/,
    /['"]@tauri-apps\//,
    /['"]@\/types['"]/,
    /['"]@\/types\/(?:generated|protocol)['"]/,
    /\basync\b/,
    /new\s+Promise\b/,
    /new\s+WeakMap\b/,
    /\bset(?:Timeout|Interval)\s*\(/,
  ],
  'the synchronous side-effect-free Store boundary',
);

rejectMatches(
  sourceFiles(join(projectRoot, 'src', 'application'), '.ts'),
  [/['"]@\/types['"]/],
  'the explicit frontend application type boundary',
);

rejectMatches(
  [join(projectRoot, 'src', 'types', 'index.ts')],
  [/generated/, /['"]\.\/protocol['"]/],
  'the runtime-only frontend type barrel boundary',
);

rejectMatches(
  sourceFiles(join(projectRoot, 'src'), '.ts').filter(
    (file) => relative(projectRoot, file) !== 'src/types/protocol.ts',
  ),
  [/['"]@\/types\/generated['"]/, /['"]\.\/generated['"]/],
  'the single-entry frontend generated protocol boundary',
);

rejectMatches(
  [
    join(projectRoot, 'src', 'types', 'documentRuntime.ts'),
    join(projectRoot, 'src', 'types', 'pendingCellSave.ts'),
    join(projectRoot, 'src', 'types', 'recentFileRuntime.ts'),
    join(projectRoot, 'src', 'types', 'fileRuntime.ts'),
    join(projectRoot, 'src', 'projection', 'documentProjection.ts'),
  ],
  [
    /['"]@\/types\/(?:generated|protocol)['"]/,
    /['"]\.\/generated['"]/,
    /\b(?:OpenDocumentResponse|SheetRegionProjectionResponse|EditorMutationResponse)\b/,
  ],
  'the generated-protocol-independent frontend runtime model boundary',
);

rejectMatches(
  [join(projectRoot, 'src', 'composables', 'useRecentFileUpdates.ts')],
  [
    /new\s+WeakMap\b/,
    /\bRecentFileUpdateScheduler\b/,
    /\bstartRecentFileUpdateWorker\b/,
    /\brunRecentFileUpdateWorker\b/,
    /\bactive\s*:\s*Promise/,
    /\bpending\s*:/,
  ],
  'the application-owned recent-file scheduling boundary',
);

const recentFilesServiceSource = readFileSync(
  join(projectRoot, 'src', 'application', 'recentFilesService.ts'),
  'utf8',
);
for (const requirement of [
  /\bactiveTracking\b/,
  /\bpendingTracking\b/,
  /\bqueueRecentFileEntryUpdate\b/,
]) {
  if (!requirement.test(recentFilesServiceSource)) {
    violations.push(
      `src/application/recentFilesService.ts violates the application-owned recent-file scheduling boundary: ${requirement}`,
    );
  }
}

rejectMatches(
  [
    join(projectRoot, 'src', 'stores', 'documentStatus.ts'),
    join(projectRoot, 'src', 'stores', 'searchSession.ts'),
    join(projectRoot, 'src', 'stores', 'editorSelection.ts'),
  ],
  [
    /['"]@\/types['"]/,
    /['"]@\/types\/generated['"]/,
    /\b(?:EditorPatch|EditorSessionInfo|EditorStateInfo|SearchResponse)\b/,
  ],
  'the generated-protocol-independent frontend Store boundary',
);

rejectMatches(
  [
    join(projectRoot, 'src', 'stores', 'updateSession.ts'),
    join(projectRoot, 'src', 'application', 'updateCoordinator.ts'),
  ],
  [
    /['"]@\/types['"]/,
    /['"]@\/types\/generated['"]/,
    /\bUpdateInfo\b/,
  ],
  'the internal-model-only frontend update boundary',
);

for (const file of sourceFiles(join(projectRoot, 'src', 'stores'), '.ts')) {
  const source = readFileSync(file, 'utf8');
  const state = source.match(/state\s*:\s*\(\)\s*=>\s*\(\{([\s\S]*?)\}\),\s*(?:getters|actions)\s*:/)?.[1];
  if (state && /\bnew\s+(?:Map|Set)\b/.test(state)) {
    violations.push(
      `${relative(projectRoot, file)} violates the serializable Store state boundary`,
    );
  }
}

rejectMatches(
  sourceFiles(join(projectRoot, 'src', 'types'), '.ts'),
  [/\b(?:Readonly)?Map\s*</, /\b(?:Readonly)?Set\s*</],
  'the serializable frontend runtime contract boundary',
);

rejectMatches(
  sourceFiles(join(projectRoot, 'src-tauri', 'src', 'io'), '.rs'),
  [
    /crate::application(?:::|\b)/,
    /crate::commands(?:::|\b)/,
    /crate::ops(?:::|\b)/,
    /crate::state(?:::|\b)/,
  ],
  'the inward-only Rust application dependency boundary',
);

rejectMatches(
  sourceFiles(join(projectRoot, 'src-tauri', 'src', 'io'), '.rs'),
  [/crate::types(?:::|\b)/, /crate::recent(?:::|\b)/],
  'the protocol-and-recent-independent Rust I/O boundary',
);

rejectMatches(
  sourceFiles(join(projectRoot, 'src-tauri', 'src', 'types'), '.rs'),
  [
    /crate::application(?:::|\b)/,
    /crate::display(?:::|\b)/,
    /crate::io(?:::|\b)/,
    /crate::ops(?:::|\b)/,
    /crate::state(?:::|\b)/,
  ],
  'the runtime-independent Rust contract boundary',
);

rejectRustProductionMatches(
  sourceFiles(join(projectRoot, 'src-tauri', 'src', 'application'), '.rs'),
  [/crate::types(?:::|\b)/, /crate::protocol_projection(?:::|\b)/],
  'the internal-outcome-only Rust application boundary',
);

rejectRustProductionMatches(
  sourceFiles(join(projectRoot, 'src-tauri', 'src', 'ops'), '.rs'),
  [/crate::types(?:::|\b)/, /crate::protocol_projection(?:::|\b)/],
  'the internal-mutation-outcome Rust operation boundary',
);

rejectRustProductionMatches(
  [
    join(projectRoot, 'src-tauri', 'src', 'application', 'mutation_replay.rs'),
    join(projectRoot, 'src-tauri', 'src', 'application', 'editor_command_service.rs'),
  ],
  [/\bserde(?:::|\b)/, /\bserde_json(?:::|\b)/, /\bSerialize\b/, /\bDeserialize\b/],
  'the semantic-mutation-fingerprint application boundary',
);

rejectMatches(
  sourceFiles(join(projectRoot, 'src-tauri', 'src', 'projection_model'), '.rs'),
  [
    /crate::types(?:::|\b)/,
    /\bserde(?:::|\b)/,
    /\bserde_json(?:::|\b)/,
    /\bts_rs\b/,
    /\bSerialize\b/,
    /\bDeserialize\b/,
  ],
  'the serialization-independent Rust projection model boundary',
);

rejectMatches(
  [
    join(projectRoot, 'src-tauri', 'src', 'application', 'search_service.rs'),
    join(projectRoot, 'src-tauri', 'src', 'application', 'search_ports.rs'),
  ],
  [
    /\bEditorMutationResponse\b/,
    /\bEditorPatch\b/,
    /\btantivy\b/,
    /std::thread/,
    /\bCondvar\b/,
    /\bIndexScheduler(?:State)?\b/,
    /\bActiveDocumentRepository\b/,
    /\bDocumentHandle\b/,
    /crate::state(?:::|\b)/,
    /\bRepositorySearchDocumentSource\b/,
  ],
  'the transport-independent search scheduling boundary',
);

rejectMatches(
  [join(projectRoot, 'src-tauri', 'src', 'application', 'search_ports.rs')],
  [/\bActiveDocumentRepository\b/, /\bDocumentHandle\b/, /crate::state(?:::|\b)/],
  'the repository-independent search port boundary',
);

rejectMatches(
  [join(projectRoot, 'src-tauri', 'src', 'application', 'search_ports.rs')],
  [/\bSearchIndexPort\b/],
  'the segregated search query and index-maintenance port boundary',
);

rejectMatches(
  [
    join(projectRoot, 'src-tauri', 'src', 'application', 'document_save_service.rs'),
    join(projectRoot, 'src-tauri', 'src', 'application', 'document_service.rs'),
  ],
  [/\bSearchService\b/],
  'the search-use-case-independent document workflow boundary',
);

rejectMatches(
  [join(projectRoot, 'src-tauri', 'src', 'application', 'document_query_service.rs')],
  [/crate::io(?:::|\b)/, /crate::ops(?:::|\b)/],
  'the query-orchestration-only document service boundary',
);

rejectMatches(
  [join(projectRoot, 'src-tauri', 'src', 'application', 'document_save_service.rs')],
  [/document_query_service/, /\.generate_file_bytes_for_target\s*\(/],
  'the independent document save workflow boundary',
);

rejectMatches(
  sourceFiles(join(projectRoot, 'src-tauri', 'src', 'types'), '.rs'),
  [/\bSearchIndexWork\b/, /\bSearchIndexUpdatePlan\b/],
  'the wire-only Rust response contract boundary',
);

rejectMatches(
  sourceFiles(join(projectRoot, 'src-tauri', 'src', 'types'), '.rs'),
  [/\bDocumentData\b/, /\bDocumentSheet\b/],
  'the projection-only Rust protocol model boundary',
);

rejectMatches(
  [join(projectRoot, 'src-tauri', 'src', 'document_data.rs')],
  [
    /crate::types(?:::|\b)/,
    /\bserde(?:::|\b)/,
    /\bserde_json(?:::|\b)/,
    /\bts_rs\b/,
    /\bTS\b/,
    /\bSerialize\b/,
    /\bDeserialize\b/,
    /#\[(?:serde|ts)\b/,
  ],
  'the serialization-independent canonical document data boundary',
);

rejectMatches(
  [join(projectRoot, 'src-tauri', 'src', 'application', 'editor_command_service.rs')],
  [/\bSearchService\b/, /\bSearchResponse\b/, /\bSearchScope\b/, /pub\s+fn\s+search\s*\(/],
  'the mutation-only editor command service boundary',
);

rejectMatches(
  [
    ...sourceFiles(join(projectRoot, 'src-tauri', 'src', 'state'), '.rs'),
    ...sourceFiles(join(projectRoot, 'src-tauri', 'src', 'ops'), '.rs'),
  ],
  [/crate::io(?:::|\b)/],
  'the Rust document aggregate boundary',
);

rejectMatches(
  [
    ...sourceFiles(join(projectRoot, 'src-tauri', 'src', 'document'), '.rs').filter((file) => {
      const path = relative(projectRoot, file);
      return !path.includes('/backing/') && !path.endsWith('/test_support.rs');
    }),
    ...sourceFiles(join(projectRoot, 'src-tauri', 'src', 'state'), '.rs'),
  ],
  [
    /\bEditorMutationResponse\b/,
    /\bEditorPatch\b/,
    /\bAppliedOperationResult\b/,
    /\bSheetCellChange\b/,
    /\bLayoutPatch\b/,
    /\bResyncRequiredPatch\b/,
    /\bts_rs\b/,
  ],
  'the protocol-independent Rust document and state outcome boundary',
);

rejectMatches(
  sourceFiles(join(projectRoot, 'src-tauri', 'src', 'document'), '.rs').filter((file) => {
    const path = relative(projectRoot, file);
    return !path.includes('/backing/') && !path.endsWith('/test_support.rs');
  }),
  [/crate::io(?:::|\b)/],
  'the backing-mediated Rust document dependency boundary',
);

rejectMatches(
  [join(projectRoot, 'src-tauri', 'src', 'application', 'prepared_document_repository.rs')],
  [/active_document_store/, /crate::state::active_document_store/],
  'the prepared-document repository boundary',
);

rejectMatches(
  sourceFiles(join(projectRoot, 'src-tauri', 'src', 'commands'), '.rs'),
  [
    /crate::ops(?:::|\b)/,
    /crate::state(?:::|\b)/,
    /crate::io(?:::|\b)/,
    /crate::update(?:::|\b)/,
    /active_document_store/,
  ],
  'the transport-only Rust command boundary',
);

rejectMatches(
  sourceFiles(join(projectRoot, 'src-tauri', 'src', 'commands'), '.rs'),
  [/\bOnceLock\b/, /static\s+EXECUTOR\b/, /fn\s+executor\s*\(/],
  'the explicitly-owned command execution runtime boundary',
);

rejectMatches(
  [
    ...sourceFiles(join(projectRoot, 'src-tauri', 'src', 'state'), '.rs'),
    ...sourceFiles(join(projectRoot, 'src-tauri', 'src', 'ops'), '.rs'),
  ],
  [
    /\bIndexScheduler(?:State)?\b/,
    /\bIndexJob\b/,
    /\bSearchIndexStore\b/,
    /\bSearchSheetIndex\b/,
    /\bSearchWriterHandle\b/,
    /\bSearchQueryPlan\b/,
    /\btantivy\b/,
    /crate::adapters(?:::|\b)/,
  ],
  'the infrastructure-free Rust state and operation boundary',
);

rejectMatches(
  sourceFiles(join(projectRoot, 'src-tauri', 'src', 'io', 'platform'), '.rs'),
  [/active_document_store/, /crate::state(?:::|\b)/],
  'the state-free platform I/O boundary',
);

rejectMatches(
  sourceFiles(join(projectRoot, 'src', 'application'), '.ts'),
  [
    /['"]element-plus['"]/,
    /['"]vue-router['"]/,
    /['"]@\/composables(?:\/|['"])/,
    /['"]@\/stores(?:\/|['"])/,
    /['"]@\/platform(?:\/|['"])/,
    /['"]@\/api['"]/,
    /['"]@\/tauriInvoke['"]/,
    /['"]@tauri-apps\//,
    /['"]pinia['"]/,
    /['"]vue['"]/,
  ],
  'the UI-independent frontend application boundary',
);

rejectMatches(
  [join(projectRoot, 'src', 'application', 'applicationExitCoordinator.ts')],
  [
    /^const\s+exitGuards\b/m,
    /^let\s+activeExitRequest\b/m,
    /\bExitAction\b/,
  ],
  'the instance-owned intent-based frontend exit boundary',
);

const frontendMainSource = readFileSync(join(projectRoot, 'src', 'main.ts'), 'utf8');
for (const requirement of [
  /\bcreateApplicationExitCoordinator\b/,
  /\.provide\s*\(\s*applicationExitCoordinatorKey\b/,
]) {
  if (!requirement.test(frontendMainSource)) {
    violations.push(
      `src/main.ts violates the composition-root-owned frontend exit boundary: ${requirement}`,
    );
  }
}

rejectMatches(
  [join(projectRoot, 'src', 'platform', 'updatePort.ts')],
  [
    /applicationExitCoordinator/,
    /['"]@tauri-apps\/plugin-process['"]/,
    /\brequestExit\b/,
    /\brelaunch\b/,
  ],
  'the update-transport-only frontend platform boundary',
);

rejectMatches(
  [join(projectRoot, 'src', 'application', 'updateCoordinator.ts')],
  [/relaunchApplication/, /requestExit\s*\(/],
  'the narrow frontend update exit-port boundary',
);

rejectMatches(
  [
    join(projectRoot, 'src-tauri', 'src', 'state', 'state.rs'),
    join(projectRoot, 'src-tauri', 'src', 'application', 'mutation_replay.rs'),
    join(projectRoot, 'src-tauri', 'src', 'application', 'prepared_document_repository.rs'),
    join(projectRoot, 'src-tauri', 'src', 'application', 'search_service.rs'),
  ],
  [
    /static\s+ACTIVE_DOCUMENT_STORE/,
    /static\s+MUTATION_REPLAYS/,
    /static\s+PREPARED_DOCUMENTS/,
    /static\s+INDEX_SCHEDULER/,
    /fn\s+active_document_store\s*\(/,
    /fn\s+replay_coordinator\s*\(/,
    /fn\s+index_scheduler\s*\(/,
    /static\s+SEARCH_SCAN_WORK/,
  ],
  'the explicitly-owned Rust application runtime boundary',
);

rejectMatches(
  [
    ...sourceFiles(join(projectRoot, 'src-tauri', 'src', 'application'), '.rs').filter(
      (file) => relative(projectRoot, file) !== 'src-tauri/src/application/runtime.rs',
    ),
  ],
  [/\bApplicationRuntime\b/],
  'the narrow Rust application service dependency boundary',
);

rejectMatches(
  sourceFiles(join(projectRoot, 'src-tauri', 'src', 'application'), '.rs').filter(
    (file) => relative(projectRoot, file) !== 'src-tauri/src/application/runtime.rs',
  ),
  [
    /\btauri::(?:AppHandle|State)\b/,
    /crate::io(?:::|\b)/,
    /crate::adapters(?:::|\b)/,
  ],
  'the framework-independent Rust application service boundary',
);

rejectMatches(
  [join(projectRoot, 'src-tauri', 'src', 'application', 'runtime.rs')],
  [/fn\s+desktop_files\s*\(/, /fn\s+mobile_files\s*\(/, /fn\s+recent_store\s*\(/],
  'the adapter-only Rust composition-root interface',
);

rejectMatches(
  sourceFiles(join(projectRoot, 'src-tauri', 'src'), '.rs').filter(
    (file) => relative(projectRoot, file) !== 'src-tauri/src/state/state.rs',
  ),
  [/\bActiveDocumentStore\b/, /Arc\s*<\s*RwLock\s*<\s*ActiveDocumentStore/],
  'the encapsulated active-document repository boundary',
);

rejectMatches(
  [
    join(projectRoot, 'src-tauri', 'src', 'adapters', 'document_work_budget_adapter.rs'),
    join(projectRoot, 'src-tauri', 'src', 'io', 'transient_files.rs'),
    join(projectRoot, 'src-tauri', 'src', 'io', 'managed_documents.rs'),
    join(projectRoot, 'src-tauri', 'src', 'io', 'platform', 'desktop.rs'),
    join(projectRoot, 'src-tauri', 'src', 'io', 'platform', 'mobile.rs'),
    join(projectRoot, 'src-tauri', 'src', 'recent', 'store.rs'),
  ],
  [
    /static\s+(?:SAVE_WORK|TRANSIENT_FILE_REGISTRY|MANAGED_DOCUMENT_TRANSACTION|AUTHORIZED_OPEN_PATHS|AUTHORIZED_SAVE_PATHS|MOBILE_STORAGE_DIRECTORY|RECENT_STORE_TRANSACTION)\b/,
    /fn\s+(?:transient_file_registry|open_paths|save_paths)\s*\(/,
  ],
  'the explicitly-owned Rust infrastructure runtime boundary',
);

rejectMatches(
  sourceFiles(join(projectRoot, 'src-tauri', 'src', 'domain'), '.rs'),
  [
    /crate::types(?:::|\b)/,
    /SetCellRequest/,
    /EditorMutationResponse/,
    /EditorPatch/,
    /AppliedOperationResult/,
    /\bserde(?:::|\b)/,
    /\bserde_json(?:::|\b)/,
    /ts_rs/,
    /tauri::/,
  ],
  'the transport-independent Rust domain model boundary',
);

rejectMatches(
  [
    ...sourceFiles(join(projectRoot, 'src-tauri', 'src', 'document'), '.rs'),
    ...sourceFiles(join(projectRoot, 'src-tauri', 'src', 'formula'), '.rs'),
    ...sourceFiles(join(projectRoot, 'src-tauri', 'src', 'state'), '.rs'),
  ],
  [/crate::types(?:::|\b)/],
  'the wire-independent Rust document, formula, and state model boundary',
);

rejectMatches(
  [
    join(projectRoot, 'src-tauri', 'src', 'application', 'search_ports.rs'),
    join(projectRoot, 'src-tauri', 'src', 'application', 'search_service.rs'),
    join(projectRoot, 'src-tauri', 'src', 'adapters', 'search_query_adapter.rs'),
    join(projectRoot, 'src-tauri', 'src', 'adapters', 'search_index_adapter.rs'),
  ],
  [
    /crate::types(?:::|\b)/,
    /\bSearchResponse\b/,
    /\bSearchResult\b/,
    /\bserde_json\b/,
  ],
  'the internal-model-only Rust search use-case boundary',
);

rejectMatches(
  [join(projectRoot, 'src', 'application', 'documentSessionCoordinator.ts')],
  [
    /documentRegionLoadScheduler/,
    /documentRegionRepository/,
    /\bensureSheetRegionLoaded\b/,
    /\bloadRegionBlock\b/,
  ],
  'the region-loading-independent document session transaction boundary',
);

rejectMatches(
  [join(projectRoot, 'src', 'application', 'documentSessionRuntime.ts')],
  [/function\s+reset\s*\(\)\s*\{[^}]*\btail\s*=\s*null/s],
  'the drain-preserving frontend mutation reset boundary',
);

rejectMatches(
  [join(projectRoot, 'src', 'application', 'pendingCellSaveCoordinator.ts')],
  [/function\s+reset\s*\(\)\s*\{[^}]*\bpendingSavePromise\s*=\s*null/s],
  'the drain-preserving frontend pending-save reset boundary',
);

rejectMatches(
  [
    join(projectRoot, 'src', 'application', 'documentRegionCoordinator.ts'),
    join(projectRoot, 'src', 'application', 'documentRegionRepository.ts'),
  ],
  [
    /['"]@\/types\/protocol['"]/,
    /\bSheetRegionProjectionResponse\b/,
    /\bEditorMutationResponse\b/,
    /\bDocumentSessionLifecycle\b/,
    /\bFormulaStatus\b/,
    /\bWorkbookCapabilities\b/,
    /\bSearchSessionSnapshot\b/,
  ],
  'the narrow frontend document region coordinator boundary',
);

rejectMatches(
  [join(projectRoot, 'src', 'composables', 'useRouteFileLoader.ts')],
  [
    /\brouteLoadGeneration\b/,
    /\bpendingLoad\b/,
    /\bworkerRunning\b/,
    /\bactiveCancellation\b/,
    /\bRouteContinuationGuard\b/,
  ],
  'the application-owned route document load scheduling boundary',
);

const routeDocumentLoadCoordinatorSource = readFileSync(
  join(projectRoot, 'src', 'application', 'routeDocumentLoadCoordinator.ts'),
  'utf8',
);
for (const requirement of [
  /\brouteLoadGeneration\b/,
  /\bpendingLoad\b/,
  /\bworkerRunning\b/,
  /\bactiveCancellation\b/,
]) {
  if (!requirement.test(routeDocumentLoadCoordinatorSource)) {
    violations.push(
      `src/application/routeDocumentLoadCoordinator.ts violates the application-owned route document load scheduling boundary: ${requirement}`,
    );
  }
}

rejectMatches(
  [join(projectRoot, 'src', 'stores', 'pendingCellSaves.ts')],
  [
    /MAX_CELL_CHANGES_PER_BATCH\s*=\s*4_?096/,
    /MAX_CELL_TEXT_BYTES\s*=\s*4\s*\*\s*1024\s*\*\s*1024/,
    /MAX_BATCH_TEXT_BYTES\s*=\s*8\s*\*\s*1024\s*\*\s*1024/,
  ],
  'the generated frontend cell mutation resource policy boundary',
);

const editorResourcePolicySource = readFileSync(
  join(projectRoot, 'src', 'protocol', 'editorResourcePolicy.ts'),
  'utf8',
);
for (const requirement of [
  /\bMAX_SET_CELL_CHANGES\b/,
  /\bPROTOCOL_MAX_CELL_TEXT_BYTES\b/,
  /\bPROTOCOL_MAX_MUTATION_TEXT_BYTES\b/,
]) {
  if (!requirement.test(editorResourcePolicySource)) {
    violations.push(
      `src/protocol/editorResourcePolicy.ts violates the generated frontend cell mutation resource policy boundary: ${requirement}`,
    );
  }
}

rejectMatches(
  [join(projectRoot, 'src-tauri', 'src', 'commands', 'common.rs')],
  [/^const\s+MAX_SET_CELL_CHANGES\b/m],
  'the Rust-owned generated cell mutation resource policy boundary',
);

rejectMatches(
  [join(projectRoot, 'src', 'stores', 'documentSession.ts')],
  [
    /\bEditorMutationResponse\b/,
    /\bOpenDocumentResponse\b/,
    /\bSavedDocumentResponse\b/,
    /\bEditorSessionInfo\b/,
    /\bEDITOR_MUTATION_PROTOCOL_VERSION\b/,
    /['"]@\/types\/generated['"]/,
    /\bapplyProjectionPatches\b/,
    /\bcreateDocumentProjection\b/,
  ],
  'the protocol-agnostic frontend document Store boundary',
);

rejectMatches(
  sourceFiles(join(projectRoot, 'src', 'utils'), '.ts'),
  [
    /['"]@\/api['"]/,
    /['"]@\/tauriInvoke['"]/,
    /['"]@\/platform(?:\/|['"])/,
    /['"]@\/stores(?:\/|['"])/,
    /['"]@\/composables(?:\/|['"])/,
    /['"]@tauri-apps\//,
    /['"]element-plus['"]/,
    /['"]vue-router['"]/,
  ],
  'the side-effect-free frontend utility boundary',
);

rejectMatches(
  [
    join(projectRoot, 'src', 'application', 'recentFilesService.ts'),
    join(projectRoot, 'src', 'application', 'spreadsheetFormatService.ts'),
  ],
  [
    /['"]@\/api['"]/,
    /['"]@\/platform(?:\/|['"])/,
    /['"]@\/stores(?:\/|['"])/,
    /['"]@\/composables(?:\/|['"])/,
    /['"]@tauri-apps\//,
  ],
  'the port-driven frontend service boundary',
);

rejectMatches(
  [join(projectRoot, 'src', 'application', 'documentFileCoordinator.ts')],
  [
    /['"]@\/api['"]/,
    /['"]@\/platform(?:\/|['"])/,
    /['"]@\/stores(?:\/|['"])/,
    /['"]@tauri-apps\//,
    /\bOpenDocumentResponse\b/,
    /\bSavedDocumentResponse\b/,
  ],
  'the port-driven document file coordinator boundary',
);

rejectMatches(
  [join(projectRoot, 'src', 'application', 'searchSessionCoordinator.ts')],
  [/\bSearchResponse\b/, /['"]@\/types\/generated['"]/, /['"]@\/types['"]/],
  'the runtime-search-outcome frontend coordinator boundary',
);

rejectMatches(
  [
    join(projectRoot, 'src-tauri', 'src', 'adapters', 'search_query_adapter.rs'),
    join(projectRoot, 'src-tauri', 'src', 'adapters', 'search_index_adapter.rs'),
  ],
  [/std::thread/, /\bCondvar\b/, /\bIndexJob\b/, /\bIndexSchedulerState\b/],
  'the worker-runtime-independent search port adapter boundary',
);

rejectMatches(
  [join(projectRoot, 'src-tauri', 'src', 'adapters', 'search_query_adapter.rs')],
  [/impl\s+SearchIndexMaintenancePort\b/],
  'the query-only search adapter boundary',
);

rejectMatches(
  [join(projectRoot, 'src-tauri', 'src', 'adapters', 'search_index_adapter.rs')],
  [/impl\s+SearchQueryPort\b/],
  'the maintenance-only search adapter boundary',
);

const searchRuntimeSource = readFileSync(
  join(projectRoot, 'src-tauri', 'src', 'adapters', 'search_index_runtime.rs'),
  'utf8',
);
for (const requirement of [/JoinHandle/, /shutdown\.store/, /\.join\s*\(\)/]) {
  if (!requirement.test(searchRuntimeSource)) {
    violations.push(
      `src-tauri/src/adapters/search_index_runtime.rs violates the deterministically-owned search worker boundary: ${requirement}`,
    );
  }
}

rejectMatches(
  [join(projectRoot, 'src-tauri', 'src', 'commands', 'mobile.rs')],
  [/crate::adapters::update_adapter/, /crate::update(?:::|\b)/],
  'the application-service-mediated mobile update command boundary',
);

rejectMatches(
  [join(projectRoot, 'src-tauri', 'src', 'application', 'update_service.rs')],
  [
    /crate::types(?:::|\b)/,
    /\breqwest(?:::|\b)/,
    /\bserde(?:::|\b)/,
    /\bserde_json(?:::|\b)/,
    /\bAtomicBool\b/,
    /\bOnceLock\b/,
  ],
  'the provider-and-transport-independent update application boundary',
);

rejectRustProductionMatches(
  [join(projectRoot, 'src-tauri', 'src', 'adapters', 'update_adapter.rs')],
  [/crate::types(?:::|\b)/, /static\s+UPDATE/, /\bOnceLock\b/],
  'the instance-owned update infrastructure boundary',
);

const applicationRuntimeSource = readFileSync(
  join(projectRoot, 'src-tauri', 'src', 'application', 'runtime.rs'),
  'utf8',
);
for (const requirement of [/\bUpdateService\b/, /\bupdate_queries\b/]) {
  if (!requirement.test(applicationRuntimeSource)) {
    violations.push(
      `src-tauri/src/application/runtime.rs violates the explicitly-owned update runtime boundary: ${requirement}`,
    );
  }
}

rejectMatches(
  [
    join(projectRoot, 'src', 'stores', 'documentSession.ts'),
    join(projectRoot, 'src', 'application', 'documentSessionCoordinator.ts'),
  ],
  [/protocolVersion\s*[!=]==?\s*4\b/],
  'the generated editor protocol version boundary',
);

rejectMatches(
  [
    join(projectRoot, 'src', 'stores', 'documentSession.ts'),
    join(projectRoot, 'src', 'application', 'documentRegionRepository.ts'),
  ],
  [/16\s*\*\s*1024\s*\*\s*1024/],
  'the generated Sheet-region response policy boundary',
);

rejectMatches(
  sourceFiles(join(projectRoot, 'src', 'composables'), '.ts').filter(
    (file) => relative(projectRoot, file) !== 'src/composables/useDocumentFileCoordinator.ts',
  ),
  [
    /\bprepareNewFile\s*\(/,
    /\bprepareRecentFile\s*\(/,
    /\bprepareOpenFile\s*\(/,
    /\bcommitPreparedDocument\s*\(/,
    /\babortPreparedDocument\s*\(/,
  ],
  'the centralized frontend prepared-document workflow boundary',
);

if (violations.length > 0) {
  console.error(violations.join('\n'));
  process.exit(1);
}

console.log('Architecture boundary checks passed.');
