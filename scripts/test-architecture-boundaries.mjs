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
  sourceFiles(join(projectRoot, 'src', 'components', 'table-grid'), '.vue'),
  [
    /['"]@\/components\/table-grid(?:\/index)?['"]/,
    /['"]\.(?:\/index)?['"]/,
  ],
  'the acyclic table-grid internal component boundary',
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

const commandExecutionRuntimeSource = readFileSync(
  join(projectRoot, 'src-tauri', 'src', 'commands', 'execution_runtime.rs'),
  'utf8',
);
for (const requirement of [
  /query:\s*Arc<BoundedBlockingExecutor>/,
  /pub\(crate\)\s+async\s+fn\s+run_mapped\b/,
  /pub\(crate\)\s+async\s+fn\s+run_fallibly_mapped\b/,
]) {
  if (!requirement.test(commandExecutionRuntimeSource)) {
    violations.push(
      `src-tauri/src/commands/execution_runtime.rs violates the permit-scoped response projection boundary: ${requirement}`,
    );
  }
}

const lockingQueryCommandSources = [
  join(projectRoot, 'src-tauri', 'src', 'commands', 'document.rs'),
  join(projectRoot, 'src-tauri', 'src', 'commands', 'editor.rs'),
];
rejectRustProductionMatches(
  lockingQueryCommandSources,
  [/pub\s+fn\s+(?:get_editor_state|get_mutation_result|get_document_capabilities|get_native_save_plan)\s*\(/],
  'the bounded asynchronous document query boundary',
);
const lockingQueryCommandSource = lockingQueryCommandSources
  .map((file) => readFileSync(file, 'utf8'))
  .join('\n');
for (const command of [
  'get_editor_state',
  'get_mutation_result',
  'get_document_capabilities',
  'get_native_save_plan',
]) {
  const requirement = new RegExp(
    `pub\\s+async\\s+fn\\s+${command}\\b[\\s\\S]*?\\.query\\(\\)\\s*\\.(?:run|run_mapped)\\(`,
  );
  if (!requirement.test(lockingQueryCommandSource)) {
    violations.push(
      `Rust locking query command ${command} violates the bounded asynchronous document query boundary`,
    );
  }
}

rejectRustProductionMatches(
  sourceFiles(join(projectRoot, 'src-tauri', 'src', 'commands'), '.rs'),
  [/\.await\s*(?:\?|;)?\s*\.map\s*\(\s*protocol_projection::/],
  'the permit-scoped outward protocol projection boundary',
);

const protocolProjectionSource = readFileSync(
  join(projectRoot, 'src-tauri', 'src', 'protocol_projection.rs'),
  'utf8',
);
for (const requirement of [
  /open_document_response[\s\S]*MAX_DOCUMENT_RESPONSE_BYTES/,
  /saved_document_response[\s\S]*MAX_DOCUMENT_RESPONSE_BYTES/,
  /response\.initial_region\s*=\s*None/,
  /serialized_json_bytes\s*\(\s*&?response\s*\)/,
]) {
  if (!requirement.test(protocolProjectionSource)) {
    violations.push(
      `src-tauri/src/protocol_projection.rs violates the bounded whole-document response boundary: ${requirement}`,
    );
  }
}

const boundedDocumentSessionSource = readFileSync(
  join(projectRoot, 'src', 'stores', 'documentSession.ts'),
  'utf8',
);
for (const requirement of [
  /\bmanifestResidentBytes\b/,
  /\bestimateDocumentManifestResidentBytes\b/,
  /\bMAX_DOCUMENT_MANIFEST_RESIDENT_BYTES\b/,
  /manifestResidentBytes\s*\+\s*totalBytes\(\)\s*>\s*MAX_DOCUMENT_PROJECTION_RESIDENT_BYTES/,
]) {
  if (!requirement.test(boundedDocumentSessionSource)) {
    violations.push(
      `src/stores/documentSession.ts violates the bounded frontend document projection boundary: ${requirement}`,
    );
  }
}

if (sourceFiles(join(projectRoot, 'src-tauri', 'src', 'commands'), '.rs')
  .some((file) => relative(projectRoot, file) === 'src-tauri/src/commands/common.rs')) {
  violations.push(
    'src-tauri/src/commands/common.rs violates the use-case-segregated Rust command boundary',
  );
}

rejectMatches(
  [join(projectRoot, 'src-tauri', 'src', 'commands', 'input.rs')],
  [/\#\[tauri::command/],
  'the input-deserialization-only Rust command boundary',
);

rejectMatches(
  [
    join(projectRoot, 'src-tauri', 'src', 'commands', 'document.rs'),
    join(projectRoot, 'src-tauri', 'src', 'commands', 'editor.rs'),
    join(projectRoot, 'src-tauri', 'src', 'commands', 'file.rs'),
    join(projectRoot, 'src-tauri', 'src', 'commands', 'recent.rs'),
    join(projectRoot, 'src-tauri', 'src', 'commands', 'search.rs'),
  ],
  [/super::(?:document|editor|file|recent|search)(?:::|\b)/],
  'the independent Rust command use-case module boundary',
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

const applicationExitCoordinatorSource = readFileSync(
  join(projectRoot, 'src', 'application', 'applicationExitCoordinator.ts'),
  'utf8',
);
for (const requirement of [
  /\bApplicationExitPreparation\b/,
  /await\s+executor\.execute\(intent\)[\s\S]*commitPreparations\(preparations\)/,
  /catch\s*\(error\)\s*\{[\s\S]*rollbackPreparations\(preparations\)/,
]) {
  if (!requirement.test(applicationExitCoordinatorSource)) {
    violations.push(
      `src/application/applicationExitCoordinator.ts violates the two-phase exit preparation boundary: ${requirement}`,
    );
  }
}

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

rejectMatches(
  [
    join(projectRoot, 'src', 'composables', 'useEditorCommands.ts'),
    join(projectRoot, 'src', 'composables', 'useCellEditController.ts'),
  ],
  [
    /['"]@\/api['"]/,
    /['"]@\/types\/protocol['"]/,
    /\b(?:EditorMutationResponse|SetCellRequest|MutationCommandContext)\b/,
  ],
  'the semantic frontend document command facade boundary',
);

const documentCommandBusSource = readFileSync(
  join(projectRoot, 'src', 'composables', 'useDocumentCommandBus.ts'),
  'utf8',
);
for (const requirement of [
  /new\s+WeakMap\b/,
  /\bcreateDocumentCommandCoordinator\b/,
]) {
  if (!requirement.test(documentCommandBusSource)) {
    violations.push(
      `src/composables/useDocumentCommandBus.ts violates the single-instance semantic command facade boundary: ${requirement}`,
    );
  }
}

rejectMatches(
  [join(projectRoot, 'src', 'composables', 'useDocumentCommandBus.ts')],
  [/\bcreateDocumentMutationProtocol\b/],
  'the application-owned document command execution boundary',
);

rejectMatches(
  [join(projectRoot, 'src', 'composables', 'useDocumentStatus.ts')],
  [/['"]@\/api['"]/, /\bgetEditorState\b/, /\brefreshEditorState\b/],
  'the read-only frontend document status boundary',
);

for (const [file, requirement] of [
  ['src/application/documentCommandCoordinator.ts', /\bfunction\s+refreshEditorState\b/],
  ['src/composables/useDocumentCommandBus.ts', /\brefreshEditorState\b/],
]) {
  if (!requirement.test(readFileSync(join(projectRoot, file), 'utf8'))) {
    violations.push(`${file} violates the application-owned editor state refresh boundary`);
  }
}

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
  /\bPROTOCOL_MAX_SEARCH_QUERY_BYTES\b/,
  /\bMAX_DOCUMENT_RESPONSE_BYTES\b/,
  /\bMAX_DOCUMENT_MANIFEST_RESIDENT_BYTES\b/,
  /\bMAX_DOCUMENT_PROJECTION_RESIDENT_BYTES\b/,
  /\bMAX_REGION_STAGING_WIRE_BYTES\b/,
  /\bMAX_DOCUMENT_WIRE_BYTES\b/,
  /\bPROTOCOL_SHEET_REGION_TILE_ROWS\b/,
  /\bPROTOCOL_SHEET_REGION_TILE_COLUMNS\b/,
]) {
  if (!requirement.test(editorResourcePolicySource)) {
    violations.push(
      `src/protocol/editorResourcePolicy.ts violates the generated frontend editor resource policy boundary: ${requirement}`,
    );
  }
}

const editorLayoutPolicySource = readFileSync(
  join(projectRoot, 'src', 'protocol', 'editorLayoutPolicy.ts'),
  'utf8',
);
for (const requirement of [
  /\bDEFAULT_COLUMN_WIDTH_PX\b/,
  /\bDEFAULT_ROW_HEIGHT_PX\b/,
  /\bMIN_INTERACTIVE_COLUMN_WIDTH_PX\b/,
  /\bMIN_INTERACTIVE_ROW_HEIGHT_PX\b/,
  /\bMAX_COLUMN_WIDTH_PX\b/,
  /\bMAX_ROW_HEIGHT_PX\b/,
]) {
  if (!requirement.test(editorLayoutPolicySource)) {
    violations.push(
      `src/protocol/editorLayoutPolicy.ts violates the Rust-owned generated layout policy boundary: ${requirement}`,
    );
  }
}

const tableEditorSource = readFileSync(
  join(projectRoot, 'src', 'components', 'TableEditor.vue'),
  'utf8',
);
for (const requirement of [
  /['"]@\/protocol\/editorLayoutPolicy['"]/,
  /\bDEFAULT_GRID_COLUMN_WIDTH\b/,
  /\bDEFAULT_GRID_ROW_HEIGHT\b/,
  /\bMIN_GRID_COLUMN_WIDTH\b/,
  /\bMIN_GRID_ROW_HEIGHT\b/,
  /\bMAX_GRID_COLUMN_WIDTH\b/,
  /\bMAX_GRID_ROW_HEIGHT\b/,
]) {
  if (!requirement.test(tableEditorSource)) {
    violations.push(
      `src/components/TableEditor.vue violates the generated layout policy boundary: ${requirement}`,
    );
  }
}

const operationCancellationSource = readFileSync(
  join(projectRoot, 'src', 'application', 'operationCancellation.ts'),
  'utf8',
);
for (const requirement of [/\bOperationCancellationSignal\b/, /\bisCancelled\b/, /\bonCancel\b/]) {
  if (!requirement.test(operationCancellationSource)) {
    violations.push(
      `src/application/operationCancellation.ts violates the explicit operation cancellation boundary: ${requirement}`,
    );
  }
}

const documentFileCoordinatorSource = readFileSync(
  join(projectRoot, 'src', 'application', 'documentFileCoordinator.ts'),
  'utf8',
);
for (const requirement of [
  /preparations\.runCancellable\s*\(/,
  /preparations\.run\s*\(/,
  /\bprepareApplicationExit\b/,
]) {
  if (!requirement.test(documentFileCoordinatorSource)) {
    violations.push(
      `src/application/documentFileCoordinator.ts violates the shared preparation and two-phase exit boundary: ${requirement}`,
    );
  }
}

const documentPreparationCoordinatorSource = readFileSync(
  join(projectRoot, 'src', 'application', 'documentPreparationCoordinator.ts'),
  'utf8',
);
for (const requirement of [
  /\blet\s+tail\s*:/,
  /cancellation\.onCancel\s*\(/,
  /function\s+enqueue\b/,
  /tail\s*=\s*result\.then\s*\(/,
]) {
  if (!requirement.test(documentPreparationCoordinatorSource)) {
    violations.push(
      `src/application/documentPreparationCoordinator.ts violates the drain-preserving shared preparation boundary: ${requirement}`,
    );
  }
}

const useDocumentFileCoordinatorSource = readFileSync(
  join(projectRoot, 'src', 'composables', 'useDocumentFileCoordinator.ts'),
  'utf8',
);
for (const requirement of [
  /new\s+WeakMap\s*</,
  /\bpreparationCoordinators\b/,
  /\bcreateDocumentPreparationCoordinator\b/,
]) {
  if (!requirement.test(useDocumentFileCoordinatorSource)) {
    violations.push(
      `src/composables/useDocumentFileCoordinator.ts violates the Store-scoped document preparation runtime boundary: ${requirement}`,
    );
  }
}
rejectMatches(
  [join(projectRoot, 'src', 'application', 'documentFileCoordinator.ts')],
  [/\bContinuationGuard\b/, /shouldContinue\s*:\s*ContinuationGuard/],
  'the explicit operation cancellation signal boundary',
);

const tableViewSource = readFileSync(join(projectRoot, 'src', 'views', 'TableView.vue'), 'utf8');
if (!/useApplicationExitGuard\s*\(\s*\(\)\s*=>\s*prepareApplicationExit\b/.test(tableViewSource)) {
  violations.push('src/views/TableView.vue violates the two-phase application exit guard boundary');
}
if (/useApplicationExitGuard\s*\(\s*\(\)\s*=>\s*closeCurrentDocument\b/.test(tableViewSource)) {
  violations.push('src/views/TableView.vue closes the document before platform exit succeeds');
}

rejectMatches(
  [
    join(projectRoot, 'src', 'components', 'TableEditor.vue'),
    join(projectRoot, 'src', 'components', 'cell', 'CellView.vue'),
    join(projectRoot, 'src', 'components', 'cell', 'EditableCell.vue'),
  ],
  [
    /^const\s+(?:DEFAULT|MIN|MAX)_(?:ROW_HEIGHT|COLUMN_WIDTH)\b/m,
    /rowHeight\s*:\s*72\b/,
    /minHeight\s*:\s*72\b/,
    /Math\.max\(\s*36\b/,
  ],
  'the Rust-owned generated layout policy boundary',
);

rejectMatches(
  [join(projectRoot, 'src-tauri', 'src', 'commands', 'input.rs')],
  [/^const\s+MAX_SET_CELL_CHANGES\b/m],
  'the Rust-owned generated cell mutation resource policy boundary',
);

rejectMatches(
  [join(projectRoot, 'src', 'application', 'documentRegionRepository.ts')],
  [
    /^(?:export\s+)?const\s+(?:SHEET_REGION_)?TILE_(?:ROWS|COLUMNS)\b/m,
    /\b(?:128|32)\s+as\s+const\b/,
  ],
  'the generated frontend Sheet-region tile policy boundary',
);

rejectMatches(
  [
    join(projectRoot, 'src-tauri', 'src', 'application', 'document_projection.rs'),
    join(projectRoot, 'src-tauri', 'src', 'document', 'region_metadata_index.rs'),
  ],
  [
    /^const\s+(?:INITIAL_REGION_ROWS|INITIAL_REGION_COLUMNS|TILE_ROWS|TILE_COLUMNS|SHEET_REGION_TILE_ROWS|SHEET_REGION_TILE_COLUMNS)\b/m,
  ],
  'the shared Rust Sheet-region tile policy boundary',
);

const resourceLimitsSource = readFileSync(
  join(projectRoot, 'src-tauri', 'src', 'resource_limits.rs'),
  'utf8',
);
for (const requirement of [
  /pub\s+const\s+SHEET_REGION_TILE_ROWS\b/,
  /pub\s+const\s+SHEET_REGION_TILE_COLUMNS\b/,
  /pub\s+const\s+MAX_PREPARED_DOCUMENT_BYTES\b/,
  /pub\s+const\s+MAX_ACTIVE_AND_PREPARED_DOCUMENT_BYTES\b/,
  /pub\s+const\s+MAX_SAVE_SOURCE_BYTES\b/,
  /pub\s+const\s+MAX_GENERATED_FILE_BYTES\b/,
  /pub\s+const\s+MAX_DOCUMENT_WORKING_SET_BYTES\b/,
  /pub\s+fn\s+validate_prepared_document_bytes\b/,
  /pub\s+fn\s+validate_active_and_prepared_document_bytes\b/,
]) {
  if (!requirement.test(resourceLimitsSource)) {
    violations.push(
      `src-tauri/src/resource_limits.rs violates the shared Rust Sheet-region tile policy boundary: ${requirement}`,
    );
  }
}

const documentWorkBudgetSource = readFileSync(
  join(projectRoot, 'src-tauri', 'src', 'adapters', 'document_work_budget_adapter.rs'),
  'utf8',
);
for (const requirement of [
  /\breservations:\s*HashMap\b/,
  /fn\s+reserve_preparation\b/,
  /fn\s+reserve_save\b/,
  /fn\s+set_work_bytes\b/,
  /\bMAX_DOCUMENT_WORKING_SET_BYTES\b/,
]) {
  if (!requirement.test(documentWorkBudgetSource)) {
    violations.push(
      `src-tauri/src/adapters/document_work_budget_adapter.rs violates the shared document working-set budget boundary: ${requirement}`,
    );
  }
}

const documentOpenServiceSource = readFileSync(
  join(projectRoot, 'src-tauri', 'src', 'application', 'document_open_service.rs'),
  'utf8',
);
for (const requirement of [/\.reserve_preparation\s*\(/, /work\.set_work_bytes\s*\(/]) {
  if (!requirement.test(documentOpenServiceSource)) {
    violations.push(
      `src-tauri/src/application/document_open_service.rs violates the shared preparation work budget boundary: ${requirement}`,
    );
  }
}

const documentSaveServiceSource = readFileSync(
  join(projectRoot, 'src-tauri', 'src', 'application', 'document_save_service.rs'),
  'utf8',
);
for (const requirement of [
  /\.reserve_save\s*\(/,
  /work\.set_work_bytes\s*\(/,
  /\bMAX_GENERATED_FILE_BYTES\b/,
  /\.plan_saved\s*\(/,
]) {
  if (!requirement.test(documentSaveServiceSource)) {
    violations.push(
      `src-tauri/src/application/document_save_service.rs violates the shared save working-set budget boundary: ${requirement}`,
    );
  }
}

const preparedDocumentRepositorySource = readFileSync(
  join(projectRoot, 'src-tauri', 'src', 'application', 'prepared_document_repository.rs'),
  'utf8',
);
if (!/\b_work:\s*Option<Box<dyn\s+DocumentWorkLease>>/.test(preparedDocumentRepositorySource)) {
  violations.push(
    'src-tauri/src/application/prepared_document_repository.rs does not retain its document work lease',
  );
}

const documentWorkRuntimeSource = readFileSync(
  join(projectRoot, 'src-tauri', 'src', 'application', 'runtime.rs'),
  'utf8',
);
if (!/DocumentOpenService::new\([\s\S]*work_budget\.clone\(\)[\s\S]*DocumentSaveService::new\([\s\S]*work_budget/m.test(documentWorkRuntimeSource)) {
  violations.push('src-tauri/src/application/runtime.rs does not share one document work budget across open and save services');
}

const documentLayoutPolicySource = readFileSync(
  join(projectRoot, 'src-tauri', 'src', 'document_layout_policy.rs'),
  'utf8',
);
for (const requirement of [
  /pub\s+const\s+DEFAULT_COLUMN_WIDTH_PX\b/,
  /pub\s+const\s+DEFAULT_ROW_HEIGHT_PX\b/,
  /pub\s+const\s+MIN_COLUMN_WIDTH_PX\b/,
  /pub\s+const\s+MAX_COLUMN_WIDTH_PX\b/,
  /pub\s+const\s+MIN_ROW_HEIGHT_PX\b/,
  /pub\s+const\s+MAX_ROW_HEIGHT_PX\b/,
  /pub\s+const\s+MIN_INTERACTIVE_COLUMN_WIDTH_PX\b/,
  /pub\s+const\s+MIN_INTERACTIVE_ROW_HEIGHT_PX\b/,
  /pub\s+fn\s+is_supported_column_width\b/,
  /pub\s+fn\s+is_supported_row_height\b/,
]) {
  if (!requirement.test(documentLayoutPolicySource)) {
    violations.push(
      `src-tauri/src/document_layout_policy.rs violates the canonical layout policy boundary: ${requirement}`,
    );
  }
}

for (const requirement of [
  /pub\s+fn\s+validate_column_width\b/,
  /pub\s+fn\s+validate_row_height\b/,
  /validate_column_width\(Some\(\*width\)\)/,
  /validate_row_height\(Some\(\*height\)\)/,
  /\bmetadata_text_bytes\b/,
  /\bMAX_METADATA_STRING_BYTES\b/,
  /\bMAX_PROJECTED_METADATA_TEXT_BYTES\b/,
  /\bsheet_metadata_text_usage\b/,
  /pub\s+fn\s+refresh_identity\b/,
]) {
  if (!requirement.test(resourceLimitsSource)) {
    violations.push(
      `src-tauri/src/resource_limits.rs violates the canonical document resource policy boundary: ${requirement}`,
    );
  }
}

const operationResolverSource = readFileSync(
  join(projectRoot, 'src-tauri', 'src', 'ops', 'operation_resolver.rs'),
  'utf8',
);
for (const requirement of [
  /validate_column_width\(width\)\?/,
  /validate_row_height\(height\)\?/,
  /validate_added_sheet\(file_data,\s*&sheet_name\)\?/,
]) {
  if (!requirement.test(operationResolverSource)) {
    violations.push(
      `src-tauri/src/ops/operation_resolver.rs violates the mutation resource policy boundary: ${requirement}`,
    );
  }
}

const documentResourceEstimatorSource = readFileSync(
  join(projectRoot, 'src-tauri', 'src', 'document_resource_estimator.rs'),
  'utf8',
);
for (const requirement of [
  /fn\s+document_metadata_text_usage\b/,
  /fn\s+sheet_metadata_text_usage\b/,
  /fn\s+estimate_sheet_data_bytes\b/,
  /fn\s+estimate_cell_value_bytes\b/,
  /fn\s+estimate_rich_metadata_bytes\b/,
]) {
  if (!requirement.test(documentResourceEstimatorSource)) {
    violations.push(
      `src-tauri/src/document_resource_estimator.rs violates the shared resource estimator boundary: ${requirement}`,
    );
  }
}

rejectRustProductionMatches(
  sourceFiles(join(projectRoot, 'src-tauri', 'src', 'document'), '.rs'),
  [
    /fn\s+estimate_sheet_data_bytes\b/,
    /fn\s+estimate_cell_value_bytes\b/,
    /fn\s+estimate_rich_metadata_bytes\b/,
  ],
  'the shared document resource estimator boundary',
);

for (const [file, requirements] of [
  [
    'src-tauri/src/state/editor_state.rs',
    [
      /\bresource_estimate_floor\b/,
      /\.max\(self\.resource_estimate_floor\)/,
      /\.refresh_identity\(self\.document\.projection\(\)\)/,
    ],
  ],
  [
    'src-tauri/src/application/document_open_service.rs',
    [/\bestimated_parse_bytes\b/, /\.with_resource_estimate_floor\(estimated_parse_bytes\)/],
  ],
]) {
  const source = readFileSync(join(projectRoot, file), 'utf8');
  for (const requirement of requirements) {
    if (!requirement.test(source)) {
      violations.push(`${file} violates the retained parse resource estimate boundary: ${requirement}`);
    }
  }
}

const documentRuntimeSource = readFileSync(
  join(projectRoot, 'src', 'types', 'documentRuntime.ts'),
  'utf8',
);
const documentRegionRepositorySource = readFileSync(
  join(projectRoot, 'src', 'application', 'documentRegionRepository.ts'),
  'utf8',
);
const documentSessionSource = readFileSync(
  join(projectRoot, 'src', 'stores', 'documentSession.ts'),
  'utf8',
);
const documentRegionStagingBudgetSource = readFileSync(
  join(projectRoot, 'src', 'application', 'documentRegionStagingBudget.ts'),
  'utf8',
);
const documentRegionLoadSchedulerSource = readFileSync(
  join(projectRoot, 'src', 'application', 'documentRegionLoadScheduler.ts'),
  'utf8',
);
for (const [file, source, requirements] of [
  [
    'src/types/documentRuntime.ts',
    documentRuntimeSource,
    [/\bwireBytes:\s*number\b/, /\bresidentBytes:\s*number\b/, /\bblock:\s*SheetRegionBlock\b/],
  ],
  [
    'src/application/documentRegionRepository.ts',
    documentRegionRepositorySource,
    [
      /\bblock\.wireBytes\b/,
      /\bblock\.residentBytes\b/,
      /\bMAX_REGION_RESPONSE_BYTES\b/,
      /\bMAX_REGION_BLOCK_RESIDENT_BYTES\b/,
    ],
  ],
  ['src/stores/documentSession.ts', documentSessionSource, [/\bblock\.residentBytes\b/]],
  [
    'src/application/documentRegionStagingBudget.ts',
    documentRegionStagingBudgetSource,
    [
      /\bMAX_DOCUMENT_PROJECTION_RESIDENT_BYTES\b/,
      /\bMAX_REGION_STAGING_WIRE_BYTES\b/,
      /\bRegionStagingLease\b/,
    ],
  ],
  [
    'src/application/documentRegionLoadScheduler.ts',
    documentRegionLoadSchedulerSource,
    [/\bstagingBudget\.acquire\s*\(\)/, /\bstaging\.release\s*\(\)/],
  ],
]) {
  for (const requirement of requirements) {
    if (!requirement.test(source)) {
      violations.push(`${file} violates the separate wire/resident region budget boundary: ${requirement}`);
    }
  }
}

if (!/staging\.reserve\s*\(\s*block\.residentBytes\s*,\s*block\.wireBytes\s*\)/.test(
  documentRegionRepositorySource,
)) {
  violations.push(
    'src/application/documentRegionRepository.ts violates the global pre-commit region staging budget boundary',
  );
}

rejectMatches(
  [
    join(projectRoot, 'src', 'types', 'documentRuntime.ts'),
    join(projectRoot, 'src', 'projection', 'documentProjection.ts'),
    join(projectRoot, 'src', 'application', 'documentRegionRepository.ts'),
    join(projectRoot, 'src', 'stores', 'documentSession.ts'),
  ],
  [/\bestimatedBytes\b/],
  'the separate wire/resident region budget boundary',
);

if (/MAX_RESIDENT_REGION_BYTES\s*=\s*MAX_SHEET_REGION_RESPONSE_BYTES/.test(editorResourcePolicySource)) {
  violations.push(
    'src/protocol/editorResourcePolicy.ts violates the independent frontend resident-memory policy boundary',
  );
}

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
  [join(projectRoot, 'src', 'components', 'search', 'SearchBox.vue')],
  [/const\s+MAX_SEARCH_QUERY_BYTES\b/, /4\s*\*\s*1024/],
  'the generated search-query resource policy boundary',
);

rejectMatches(
  [join(projectRoot, 'src-tauri', 'src', 'adapters', 'search_index_store.rs')],
  [/const\s+MAX_SEARCH_QUERY_BYTES\b/],
  'the editor-protocol-owned search-query resource policy boundary',
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
