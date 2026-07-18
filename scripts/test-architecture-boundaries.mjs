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

rejectMatches(
  sourceFiles(join(projectRoot, 'src', 'stores'), '.ts'),
  [
    /['"]@\/api['"]/,
    /['"]@\/tauriInvoke['"]/,
    /['"]@\/platform(?:\/|['"])/,
    /['"]@\/composables(?:\/|['"])/,
    /['"]@\/application(?:\/|['"])/,
    /['"]@tauri-apps\//,
    /\basync\b/,
    /new\s+Promise\b/,
    /new\s+WeakMap\b/,
    /\bset(?:Timeout|Interval)\s*\(/,
  ],
  'the synchronous side-effect-free Store boundary',
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

rejectMatches(
  [
    ...sourceFiles(join(projectRoot, 'src-tauri', 'src', 'state'), '.rs'),
    ...sourceFiles(join(projectRoot, 'src-tauri', 'src', 'ops'), '.rs'),
  ],
  [/crate::io(?:::|\b)/],
  'the Rust document aggregate boundary',
);

rejectMatches(
  [join(projectRoot, 'src-tauri', 'src', 'application', 'prepared_document_repository.rs')],
  [/active_document_store/, /crate::state::active_document_store/],
  'the prepared-document repository boundary',
);

rejectMatches(
  sourceFiles(join(projectRoot, 'src-tauri', 'src', 'commands'), '.rs'),
  [/crate::ops(?:::|\b)/, /crate::state(?:::|\b)/, /active_document_store/],
  'the transport-only Rust command boundary',
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
  [
    join(projectRoot, 'src-tauri', 'src', 'state', 'state.rs'),
    join(projectRoot, 'src-tauri', 'src', 'application', 'mutation_replay.rs'),
    join(projectRoot, 'src-tauri', 'src', 'application', 'prepared_document_repository.rs'),
    join(projectRoot, 'src-tauri', 'src', 'state', 'search_service.rs'),
  ],
  [
    /static\s+ACTIVE_DOCUMENT_STORE/,
    /static\s+MUTATION_REPLAYS/,
    /static\s+PREPARED_DOCUMENTS/,
    /static\s+INDEX_SCHEDULER/,
    /fn\s+active_document_store\s*\(/,
    /fn\s+replay_coordinator\s*\(/,
    /fn\s+index_scheduler\s*\(/,
  ],
  'the explicitly-owned Rust application runtime boundary',
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
    join(projectRoot, 'src-tauri', 'src', 'io', 'save_work.rs'),
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
    /ts_rs/,
    /tauri::/,
  ],
  'the transport-independent Rust domain model boundary',
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
  ],
  'the port-driven document file coordinator boundary',
);

if (violations.length > 0) {
  console.error(violations.join('\n'));
  process.exit(1);
}

console.log('Architecture boundary checks passed.');
