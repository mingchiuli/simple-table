import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';

import { dependencyGraphFixtureViolations } from './architecture/dependency-graph-fixtures.mjs';
import { createFrontendDependencyGraph } from './architecture/frontend-dependency-graph.mjs';
import { createRustDependencyGraph } from './architecture/rust-dependency-graph.mjs';
import { sourceFiles } from './architecture/source-files.mjs';

const projectRoot = fileURLToPath(new URL('..', import.meta.url));
const frontendRoot = join(projectRoot, 'src');
const rustRoot = join(projectRoot, 'src-tauri', 'src');
const violations = [];

const frontendFiles = [
  ...sourceFiles(frontendRoot, '.ts'),
  ...sourceFiles(frontendRoot, '.vue'),
];
const frontendGraph = createFrontendDependencyGraph(
  frontendRoot,
  frontendFiles,
  (file) => violations.push(`${projectPath(file)} could not be parsed as a Vue SFC`),
);

const rustFiles = sourceFiles(rustRoot, '.rs').filter(
  (file) => !file.endsWith('/test_support.rs') && !file.endsWith('/types/typescript.rs'),
);
const rustGraph = createRustDependencyGraph(rustRoot, rustFiles);

function projectPath(file) {
  return relative(projectRoot, file);
}

function frontendPath(path) {
  return join(frontendRoot, path);
}

function rustPath(path) {
  return join(rustRoot, path);
}

function filesIn(root, extension) {
  return sourceFiles(root, extension);
}

function rustLayerFiles(layer) {
  return rustFiles.filter((file) =>
    file === rustPath(`${layer}.rs`) || file.startsWith(`${rustPath(layer)}/`));
}

function frontendTargetIs(dependency, path) {
  return dependency.path === frontendPath(path);
}

function frontendTargetIn(dependency, directory) {
  return dependency.path?.startsWith(`${frontendPath(directory)}/`) ?? false;
}

function frontendExternalIs(dependency, packageName) {
  return dependency.external
    && (dependency.specifier === packageName || dependency.specifier.startsWith(`${packageName}/`));
}

function rustTargetIs(dependency, path) {
  return dependency === rustPath(path);
}

function rustTargetIn(dependency, directory) {
  return dependency.startsWith(`${rustPath(directory)}/`);
}

function rustTargetInLayer(dependency, layer) {
  return rustTargetIs(dependency, `${layer}.rs`) || rustTargetIn(dependency, layer);
}

function describeFrontendDependency(dependency) {
  return dependency.path ? projectPath(dependency.path) : dependency.specifier;
}

function rejectFrontendRule({ name, files, forbidden, transitive = false }) {
  for (const file of files) {
    if (transitive) {
      const path = frontendGraph.findForbiddenPath(file, forbidden);
      if (!path) continue;
      const formatted = path.map(({ from, dependency }) =>
        `${projectPath(from)} --${dependency.specifier}--> ${describeFrontendDependency(dependency)}`,
      ).join(' | ');
      violations.push(`${projectPath(file)} violates ${name}: ${formatted}`);
      continue;
    }
    for (const dependency of frontendGraph.dependencies.get(file) ?? []) {
      if (forbidden(dependency)) {
        violations.push(
          `${projectPath(file)} violates ${name}: ${describeFrontendDependency(dependency)}`,
        );
      }
    }
  }
}

function rejectRustRule({ name, files, forbidden, transitive = false }) {
  for (const file of files) {
    if (transitive) {
      const path = rustGraph.findForbiddenPath(file, forbidden);
      if (!path) continue;
      violations.push(
        `${projectPath(file)} violates ${name}: ${[file, ...path].map(projectPath).join(' -> ')}`,
      );
      continue;
    }
    for (const dependency of rustGraph.dependencies.get(file) ?? []) {
      if (forbidden(dependency)) {
        violations.push(`${projectPath(file)} violates ${name}: ${projectPath(dependency)}`);
      }
    }
  }
}

function rejectRustExternalRule({ name, files, forbidden }) {
  for (const file of files) {
    for (const dependency of rustGraph.externalDependencies.get(file) ?? []) {
      if (forbidden(dependency)) {
        violations.push(`${projectPath(file)} violates ${name}: ${dependency}`);
      }
    }
  }
}

const frontendRuntimeTypes = filesIn(frontendPath('types'), '.ts').filter(
  (file) => !['index.ts', 'protocol.ts', 'generated.ts'].some((name) => file.endsWith(`/${name}`)),
);
const frontendVueFiles = filesIn(frontendRoot, '.vue');
const frontendPlatformFiles = filesIn(frontendPath('platform'), '.ts');
const frontendApplicationFiles = filesIn(frontendPath('application'), '.ts');
const frontendStoreFiles = filesIn(frontendPath('stores'), '.ts');
const frontendCoreFiles = [
  ...frontendApplicationFiles,
  ...frontendStoreFiles,
  ...['projection', 'protocol', 'resourcePolicy', 'table-geometry', 'types', 'utils']
    .flatMap((directory) => filesIn(frontendPath(directory), '.ts')),
];

const frontendRules = [
  {
    name: 'the synchronous inward-only Store boundary',
    files: frontendStoreFiles,
    transitive: true,
    forbidden: (dependency) =>
      frontendTargetIs(dependency, 'api.ts')
      || frontendTargetIs(dependency, 'tauriInvoke.ts')
      || frontendTargetIs(dependency, 'types/index.ts')
      || frontendTargetIs(dependency, 'types/generated.ts')
      || frontendTargetIs(dependency, 'types/protocol.ts')
      || frontendTargetIn(dependency, 'application')
      || frontendTargetIn(dependency, 'composables')
      || frontendTargetIn(dependency, 'platform')
      || frontendExternalIs(dependency, '@tauri-apps'),
  },
  {
    name: 'the UI-independent application boundary',
    files: frontendApplicationFiles,
    transitive: true,
    forbidden: (dependency) =>
      frontendTargetIs(dependency, 'api.ts')
      || frontendTargetIs(dependency, 'tauriInvoke.ts')
      || frontendTargetIs(dependency, 'types/index.ts')
      || frontendTargetIn(dependency, 'stores')
      || frontendTargetIn(dependency, 'composables')
      || frontendTargetIn(dependency, 'platform')
      || ['@tauri-apps', 'element-plus', 'pinia', 'vue', 'vue-router'].some((packageName) =>
        frontendExternalIs(dependency, packageName)),
  },
  {
    name: 'the runtime-only type barrel boundary',
    files: [frontendPath('types/index.ts')],
    forbidden: (dependency) =>
      frontendTargetIs(dependency, 'types/generated.ts')
      || frontendTargetIs(dependency, 'types/protocol.ts'),
  },
  {
    name: 'the generated-protocol-independent runtime model boundary',
    files: frontendRuntimeTypes,
    forbidden: (dependency) =>
      frontendTargetIs(dependency, 'types/generated.ts')
      || frontendTargetIs(dependency, 'types/protocol.ts'),
  },
  {
    name: 'the single-entry generated protocol boundary',
    files: frontendFiles.filter((file) => file !== frontendPath('types/protocol.ts')),
    forbidden: (dependency) => frontendTargetIs(dependency, 'types/generated.ts'),
  },
  {
    name: 'the generated editor policy adapter boundary',
    files: frontendFiles.filter((file) => !new Set([
      frontendPath('types/generated.ts'),
      frontendPath('protocol/editorLayoutPolicy.ts'),
      frontendPath('protocol/editorResourcePolicy.ts'),
    ]).has(file)),
    forbidden: (dependency) => frontendTargetIs(dependency, 'protocol/generatedEditorPolicy.ts'),
  },
  {
    name: 'the acyclic table-grid package boundary',
    files: filesIn(frontendPath('components/table-grid'), '.vue'),
    forbidden: (dependency) => frontendTargetIs(dependency, 'components/table-grid/index.ts'),
  },
  {
    name: 'the transport-free Vue component boundary',
    files: frontendVueFiles,
    forbidden: (dependency) =>
      frontendTargetIs(dependency, 'api.ts')
      || frontendTargetIs(dependency, 'tauriInvoke.ts')
      || frontendTargetIn(dependency, 'platform')
      || frontendExternalIs(dependency, '@tauri-apps'),
  },
  {
    name: 'the platform-owned Tauri integration boundary',
    files: frontendFiles.filter((file) =>
      file !== frontendPath('tauriInvoke.ts') && !frontendPlatformFiles.includes(file)),
    forbidden: (dependency) => frontendExternalIs(dependency, '@tauri-apps'),
  },
  {
    name: 'the transport-free frontend core boundary',
    files: frontendCoreFiles,
    transitive: true,
    forbidden: (dependency) =>
      frontendTargetIs(dependency, 'api.ts')
      || frontendTargetIs(dependency, 'tauriInvoke.ts')
      || frontendTargetIn(dependency, 'platform')
      || frontendExternalIs(dependency, '@tauri-apps'),
  },
];

for (const rule of frontendRules) rejectFrontendRule(rule);
for (const cycle of frontendGraph.cycles()) {
  violations.push(`frontend module dependency cycle: ${cycle.map(projectPath).join(' -> ')}`);
}

const rustApplicationFiles = rustLayerFiles('application');
const rustCommandFiles = rustLayerFiles('commands');
const rustDocumentFiles = rustLayerFiles('document');
const rustDomainFiles = rustLayerFiles('domain');
const rustFormulaFiles = rustLayerFiles('formula');
const rustIoFiles = rustLayerFiles('io');
const rustOpsFiles = rustLayerFiles('ops');
const rustProjectionFiles = rustLayerFiles('projection_model');
const rustStateFiles = rustLayerFiles('state');
const rustTypeFiles = rustLayerFiles('types');

const rustRules = [
  {
    name: 'the transitive inward-only application boundary',
    files: rustApplicationFiles,
    transitive: true,
    forbidden: (dependency) =>
      rustTargetIs(dependency, 'runtime.rs')
      || ['adapters', 'commands', 'io', 'recent'].some((directory) =>
        rustTargetInLayer(dependency, directory)),
  },
  {
    name: 'the protocol-independent application boundary',
    files: rustApplicationFiles,
    forbidden: (dependency) =>
      rustTargetIs(dependency, 'types.rs')
      || rustTargetIs(dependency, 'protocol_projection.rs')
      || rustTargetIn(dependency, 'types')
      || rustTargetIn(dependency, 'protocol_projection'),
  },
  {
    name: 'the inward-only I/O boundary',
    files: rustIoFiles,
    forbidden: (dependency) =>
      ['application', 'commands', 'ops', 'recent', 'state', 'types'].some((directory) =>
        rustTargetInLayer(dependency, directory)),
  },
  {
    name: 'the runtime-independent protocol boundary',
    files: rustTypeFiles,
    forbidden: (dependency) =>
      ['application', 'io', 'ops', 'recent', 'state'].some((directory) =>
        rustTargetInLayer(dependency, directory)),
  },
  {
    name: 'the inward-only domain boundary',
    files: rustDomainFiles,
    forbidden: (dependency) =>
      ['adapters', 'application', 'commands', 'io', 'ops', 'protocol_projection', 'recent', 'state', 'types']
        .some((layer) => rustTargetInLayer(dependency, layer))
      || rustTargetIs(dependency, 'runtime.rs'),
  },
  {
    name: 'the direct-sibling protocol module boundary',
    files: rustTypeFiles,
    forbidden: (dependency) => rustTargetIs(dependency, 'types.rs'),
  },
  {
    name: 'the internal-outcome operation boundary',
    files: rustOpsFiles,
    forbidden: (dependency) =>
      rustTargetIs(dependency, 'types.rs')
      || rustTargetIs(dependency, 'protocol_projection.rs')
      || rustTargetIn(dependency, 'types')
      || rustTargetIn(dependency, 'protocol_projection'),
  },
  {
    name: 'the wire-independent document, formula, and state boundary',
    files: [...rustDocumentFiles, ...rustFormulaFiles, ...rustStateFiles],
    forbidden: (dependency) =>
      rustTargetIs(dependency, 'types.rs') || rustTargetIn(dependency, 'types'),
  },
  {
    name: 'the infrastructure-free document operation boundary',
    files: [...rustDocumentFiles, ...rustOpsFiles, ...rustStateFiles],
    forbidden: (dependency) =>
      rustTargetInLayer(dependency, 'io')
      || rustTargetInLayer(dependency, 'adapters'),
  },
  {
    name: 'the transport-only command boundary',
    files: rustCommandFiles,
    forbidden: (dependency) =>
      ['io', 'ops', 'state'].some((directory) => rustTargetInLayer(dependency, directory)),
  },
  {
    name: 'the independent command use-case boundary',
    files: rustCommandFiles.filter((file) => ![
      rustPath('commands/execution_runtime.rs'),
      rustPath('commands/input.rs'),
    ].includes(file)),
    forbidden: (dependency) =>
      rustTargetIn(dependency, 'commands')
      && ![
        rustPath('commands/execution_runtime.rs'),
        rustPath('commands/input.rs'),
      ].includes(dependency),
  },
  {
    name: 'the serialization-independent projection model boundary',
    files: rustProjectionFiles,
    forbidden: (dependency) =>
      rustTargetIs(dependency, 'types.rs') || rustTargetIn(dependency, 'types'),
  },
  {
    name: 'the repository-independent search use-case boundary',
    files: [rustPath('application/search_ports.rs'), rustPath('application/search_service.rs')],
    forbidden: (dependency) =>
      rustTargetInLayer(dependency, 'state'),
  },
  {
    name: 'the feature-isolated protocol projection boundary',
    files: filesIn(rustPath('protocol_projection'), '.rs').filter((file) =>
      !['cell.rs', 'size.rs', 'status.rs'].some((name) => file.endsWith(`/${name}`))),
    forbidden: (dependency) =>
      rustTargetIn(dependency, 'protocol_projection')
      && !['cell.rs', 'size.rs', 'status.rs'].some((name) => dependency.endsWith(`/${name}`)),
  },
];

for (const rule of rustRules) rejectRustRule(rule);

const serializationFreeRustFiles = [
  ...rustDomainFiles,
  rustPath('document_data.rs'),
  ...rustProjectionFiles,
  rustPath('recent/model.rs'),
];
rejectRustExternalRule({
  name: 'the serialization-independent internal model boundary',
  files: serializationFreeRustFiles,
  forbidden: (dependency) => ['serde', 'serde_json', 'ts_rs', 'tauri'].includes(dependency),
});
rejectRustExternalRule({
  name: 'the framework-independent application boundary',
  files: rustApplicationFiles,
  forbidden: (dependency) =>
    ['serde', 'serde_json', 'tauri', 'tantivy', 'tantivy_jieba', 'ts_rs'].includes(dependency),
});
rejectRustExternalRule({
  name: 'the encapsulated search-index backend boundary',
  files: rustFiles.filter((file) => file !== rustPath('adapters/search_index_backend.rs')),
  forbidden: (dependency) => ['tantivy', 'tantivy_jieba'].includes(dependency),
});

const ownedRustModules = [
  {
    file: rustPath('adapters/search_index_scheduler.rs'),
    consumers: new Set([
      rustPath('adapters/search_index_runtime.rs'),
      rustPath('adapters/search_index_worker.rs'),
    ]),
  },
  {
    file: rustPath('adapters/search_index_worker.rs'),
    consumers: new Set([rustPath('adapters/search_index_runtime.rs')]),
  },
  {
    file: rustPath('adapters/search_index_backend.rs'),
    consumers: new Set([
      rustPath('adapters/search_index_registry.rs'),
      rustPath('adapters/search_index_runtime.rs'),
      rustPath('adapters/search_index_worker.rs'),
      rustPath('adapters/search_query_engine.rs'),
    ]),
  },
];
for (const ownership of ownedRustModules) {
  for (const [consumer, dependencies] of rustGraph.dependencies) {
    if (!ownership.consumers.has(consumer) && dependencies.includes(ownership.file)) {
      violations.push(
        `${projectPath(consumer)} violates the privately owned infrastructure boundary: ${projectPath(ownership.file)}`,
      );
    }
  }
}

for (const cycle of rustGraph.cycles()) {
  violations.push(`Rust production module dependency cycle: ${cycle.map(projectPath).join(' -> ')}`);
}

violations.push(...dependencyGraphFixtureViolations({
  frontendRoot,
  rustRoot,
  frontendGraph,
  rustGraph,
}));

if (violations.length > 0) {
  console.error(violations.join('\n'));
  process.exit(1);
}

console.log(
  `Architecture dependency checks passed (${frontendFiles.length} frontend modules, ${rustFiles.length} Rust modules).`,
);
