import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { dirname, extname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse as parseVueSfc } from '@vue/compiler-sfc';
import * as ts from 'typescript';

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

const rustRoot = join(projectRoot, 'src-tauri', 'src');
const rustFiles = sourceFiles(rustRoot, '.rs').filter(
  (file) => !file.endsWith('/test_support.rs') && !file.endsWith('/types/typescript.rs'),
);
const rustModulesByPath = new Map(
  rustFiles.map((file) => [rustModuleSegments(file).join('::'), file]),
);
const rustDependencies = new Map(
  rustFiles.map((file) => [
    file,
    rustModuleDependenciesFromSource(file, rustProductionSource(file)),
  ]),
);

function rustProductionSource(file) {
  return readFileSync(file, 'utf8')
    .split(/\n#\[cfg\(test\)\]\nmod tests\s*\{/)[0]
    .replace(/#\[cfg\(test\)\]\s*(?:pub(?:\([^)]*\))?\s+)?use[\s\S]*?;/g, '');
}

function rustModuleSegments(file) {
  const segments = relative(rustRoot, file).replace(/\.rs$/, '').split('/');
  if (segments.at(-1) === 'lib') return [];
  if (segments.at(-1) === 'mod') segments.pop();
  return segments;
}

function rustModuleDependenciesFromSource(file, source) {
  const tokens = rustTokens(source);
  const currentModule = rustModuleSegments(file);
  const dependencies = new Set();

  for (let index = 0; index < tokens.length - 1; index += 1) {
    const root = tokens[index];
    if (!['crate', 'self', 'super'].includes(root) || tokens[index + 1] !== '::') continue;

    let base = root === 'crate'
      ? []
      : root === 'self'
        ? [...currentModule]
        : currentModule.slice(0, -1);
    let pathStart = index + 2;
    while (tokens[pathStart] === 'super' && tokens[pathStart + 1] === '::') {
      base = base.slice(0, -1);
      pathStart += 2;
    }
    const { paths, next } = parseRustPathTree(tokens, pathStart, base);
    index = Math.max(index, next - 1);
    for (const modulePath of paths) {
      const dependency = resolveRustModule(modulePath);
      if (dependency && dependency !== file) dependencies.add(dependency);
    }
  }

  return [...dependencies];
}

function parseRustPathTree(tokens, start, prefix) {
  if (tokens[start] === '{') {
    const paths = [];
    let index = start + 1;
    while (index < tokens.length && tokens[index] !== '}') {
      if (tokens[index] === ',') {
        index += 1;
        continue;
      }
      const parsed = parseRustPathTree(tokens, index, prefix);
      paths.push(...parsed.paths);
      index = parsed.next;
      if (tokens[index] === 'as') index += Math.min(2, tokens.length - index);
    }
    return { paths, next: tokens[index] === '}' ? index + 1 : index };
  }

  const segment = tokens[start];
  if (!isRustIdentifier(segment) && segment !== '*') {
    return { paths: [], next: start + 1 };
  }
  const path = segment === 'self' || segment === '*'
    ? prefix
    : [...prefix, segment];
  if (tokens[start + 1] === '::') {
    return parseRustPathTree(tokens, start + 2, path);
  }
  return { paths: path.length > 0 ? [path] : [], next: start + 1 };
}

function resolveRustModule(segments) {
  for (let length = segments.length; length > 0; length -= 1) {
    const file = rustModulesByPath.get(segments.slice(0, length).join('::'));
    if (file) return file;
  }
  return null;
}

function isRustIdentifier(token) {
  return typeof token === 'string' && /^[A-Za-z_][A-Za-z0-9_]*$/.test(token);
}

function rustTokens(source) {
  const tokens = [];
  let index = 0;
  while (index < source.length) {
    const current = source[index];
    const next = source[index + 1];
    if (/\s/.test(current)) {
      index += 1;
      continue;
    }
    if (current === '/' && next === '/') {
      index = source.indexOf('\n', index + 2);
      if (index < 0) break;
      continue;
    }
    if (current === '/' && next === '*') {
      index = skipRustBlockComment(source, index + 2);
      continue;
    }
    const rawStringEnd = rustRawStringEnd(source, index);
    if (rawStringEnd !== null) {
      index = rawStringEnd;
      continue;
    }
    if (current === '"') {
      index = skipRustQuoted(source, index + 1, '"');
      continue;
    }
    if (current === "'" && source[index + 2] === "'") {
      index += 3;
      continue;
    }
    if (current === "'" && next === '\\') {
      index = skipRustQuoted(source, index + 1, "'");
      continue;
    }
    if (/[A-Za-z_]/.test(current)) {
      let end = index + 1;
      while (end < source.length && /[A-Za-z0-9_]/.test(source[end])) end += 1;
      tokens.push(source.slice(index, end));
      index = end;
      continue;
    }
    if (current === ':' && next === ':') {
      tokens.push('::');
      index += 2;
      continue;
    }
    if ('{},;*'.includes(current)) tokens.push(current);
    index += 1;
  }
  return tokens;
}

function skipRustBlockComment(source, start) {
  let depth = 1;
  let index = start;
  while (index < source.length && depth > 0) {
    if (source[index] === '/' && source[index + 1] === '*') {
      depth += 1;
      index += 2;
    } else if (source[index] === '*' && source[index + 1] === '/') {
      depth -= 1;
      index += 2;
    } else {
      index += 1;
    }
  }
  return index;
}

function rustRawStringEnd(source, start) {
  if (source[start] !== 'r') return null;
  let quote = start + 1;
  while (source[quote] === '#') quote += 1;
  if (source[quote] !== '"') return null;
  const suffix = `"${'#'.repeat(quote - start - 1)}`;
  const end = source.indexOf(suffix, quote + 1);
  return end < 0 ? source.length : end + suffix.length;
}

function skipRustQuoted(source, start, quote) {
  let index = start;
  while (index < source.length) {
    if (source[index] === '\\') {
      index += 2;
    } else if (source[index] === quote) {
      return index + 1;
    } else {
      index += 1;
    }
  }
  return index;
}

function findForbiddenRustDependencyPath(start, forbidden) {
  const visited = new Set([start]);
  const pending = [{ file: start, path: [] }];
  while (pending.length > 0) {
    const current = pending.shift();
    for (const dependency of rustDependencies.get(current.file) ?? []) {
      const path = [...current.path, dependency];
      if (forbidden(dependency)) return path;
      if (!visited.has(dependency)) {
        visited.add(dependency);
        pending.push({ file: dependency, path });
      }
    }
  }
  return null;
}

function rustDependencyCycles() {
  const visiting = new Set();
  const visited = new Set();
  const stack = [];
  const stackIndexes = new Map();
  const cycles = [];

  function visit(file) {
    visiting.add(file);
    stackIndexes.set(file, stack.length);
    stack.push(file);
    for (const dependency of rustDependencies.get(file) ?? []) {
      if (visiting.has(dependency)) {
        cycles.push([...stack.slice(stackIndexes.get(dependency)), dependency]);
      } else if (!visited.has(dependency)) {
        visit(dependency);
      }
    }
    stack.pop();
    stackIndexes.delete(file);
    visiting.delete(file);
    visited.add(file);
  }

  for (const file of rustDependencies.keys()) {
    if (!visited.has(file)) visit(file);
  }
  return cycles;
}

const frontendRoot = join(projectRoot, 'src');
const frontendFiles = [
  ...sourceFiles(frontendRoot, '.ts'),
  ...sourceFiles(frontendRoot, '.vue'),
];
const frontendDependencies = new Map(
  frontendFiles.map((file) => [file, moduleDependencies(file)]),
);

function moduleDependencies(file) {
  return moduleDependenciesFromSource(file, frontendScriptSource(file));
}

function moduleDependenciesFromSource(file, source) {
  const parsed = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const dependencies = [];

  function visit(node) {
    if (
      (ts.isImportDeclaration(node) || ts.isExportDeclaration(node))
      && node.moduleSpecifier
      && ts.isStringLiteralLike(node.moduleSpecifier)
    ) {
      dependencies.push(resolveFrontendDependency(file, node.moduleSpecifier.text));
    } else if (
      ts.isCallExpression(node)
      && node.expression.kind === ts.SyntaxKind.ImportKeyword
      && node.arguments.length === 1
      && ts.isStringLiteralLike(node.arguments[0])
    ) {
      dependencies.push(resolveFrontendDependency(file, node.arguments[0].text));
    }
    ts.forEachChild(node, visit);
  }

  visit(parsed);
  return dependencies;
}

function frontendScriptSource(file) {
  const source = readFileSync(file, 'utf8');
  if (extname(file) !== '.vue') return source;
  const { descriptor, errors } = parseVueSfc(source, { filename: file });
  if (errors.length > 0) {
    violations.push(`${relative(projectRoot, file)} could not be parsed as a Vue SFC`);
  }
  return [descriptor.script?.content, descriptor.scriptSetup?.content].filter(Boolean).join('\n');
}

function resolveFrontendDependency(fromFile, specifier) {
  if (!specifier.startsWith('@/') && !specifier.startsWith('.')) {
    return { specifier, external: true, path: null };
  }
  const unresolved = specifier.startsWith('@/')
    ? resolve(frontendRoot, specifier.slice(2))
    : resolve(dirname(fromFile), specifier);
  const candidates = extname(unresolved)
    ? [unresolved]
    : [
        `${unresolved}.ts`,
        `${unresolved}.vue`,
        join(unresolved, 'index.ts'),
        join(unresolved, 'index.vue'),
        unresolved,
      ];
  return {
    specifier,
    external: false,
    path: candidates.find(existsSync) ?? unresolved,
  };
}

function rejectFrontendDependencies(files, forbidden, boundary) {
  for (const file of files) {
    for (const dependency of frontendDependencies.get(file) ?? []) {
      if (forbidden(dependency)) {
        const target = dependency.path
          ? relative(projectRoot, dependency.path)
          : dependency.specifier;
        violations.push(
          `${relative(projectRoot, file)} violates ${boundary}: ${dependency.specifier} -> ${target}`,
        );
      }
    }
  }
}

function rejectTransitiveFrontendDependencies(files, forbidden, boundary) {
  for (const file of files) {
    const path = findForbiddenFrontendDependencyPath(file, forbidden);
    if (!path) continue;
    violations.push(
      `${relative(projectRoot, file)} violates ${boundary}: ${formatFrontendDependencyPath(path)}`,
    );
  }
}

function findForbiddenFrontendDependencyPath(
  start,
  forbidden,
  graph = frontendDependencies,
) {
  const visited = new Set([start]);
  const pending = [{ file: start, path: [] }];
  while (pending.length > 0) {
    const current = pending.shift();
    for (const dependency of graph.get(current.file) ?? []) {
      const path = [...current.path, { from: current.file, dependency }];
      if (forbidden(dependency)) return path;
      if (
        dependency.path
        && graph.has(dependency.path)
        && !visited.has(dependency.path)
      ) {
        visited.add(dependency.path);
        pending.push({ file: dependency.path, path });
      }
    }
  }
  return null;
}

function formatFrontendDependencyPath(path) {
  return path.map(({ from, dependency }) => {
    const target = dependency.path
      ? relative(projectRoot, dependency.path)
      : dependency.specifier;
    return `${relative(projectRoot, from)} --${dependency.specifier}--> ${target}`;
  }).join(' | ');
}

function frontendDependencyCycles(graph = frontendDependencies) {
  const visiting = new Set();
  const visited = new Set();
  const stack = [];
  const stackIndexes = new Map();
  const cycles = [];

  function visit(file) {
    visiting.add(file);
    stackIndexes.set(file, stack.length);
    stack.push(file);
    for (const dependency of graph.get(file) ?? []) {
      const target = dependency.path;
      if (!target || !graph.has(target)) continue;
      if (visiting.has(target)) {
        cycles.push([...stack.slice(stackIndexes.get(target)), target]);
      } else if (!visited.has(target)) {
        visit(target);
      }
    }
    stack.pop();
    stackIndexes.delete(file);
    visiting.delete(file);
    visited.add(file);
  }

  for (const file of graph.keys()) {
    if (!visited.has(file)) visit(file);
  }
  return cycles;
}

function isFrontendPath(dependency, path) {
  return dependency.path === join(frontendRoot, path);
}

function isFrontendDirectory(dependency, directory) {
  const root = `${join(frontendRoot, directory)}/`;
  return dependency.path?.startsWith(root) ?? false;
}

function isExternalPackage(dependency, packageName) {
  return dependency.external
    && (dependency.specifier === packageName || dependency.specifier.startsWith(`${packageName}/`));
}

const storeFiles = sourceFiles(join(frontendRoot, 'stores'), '.ts');
rejectTransitiveFrontendDependencies(
  storeFiles,
  (dependency) =>
    isFrontendPath(dependency, 'api.ts')
    || isFrontendPath(dependency, 'tauriInvoke.ts')
    || isFrontendPath(dependency, 'types/index.ts')
    || isFrontendPath(dependency, 'types/generated.ts')
    || isFrontendPath(dependency, 'types/protocol.ts')
    || isFrontendDirectory(dependency, 'platform')
    || isFrontendDirectory(dependency, 'composables')
    || isFrontendDirectory(dependency, 'application')
    || isExternalPackage(dependency, '@tauri-apps'),
  'the resolved synchronous side-effect-free Store dependency boundary',
);

const applicationFiles = sourceFiles(join(frontendRoot, 'application'), '.ts');
rejectTransitiveFrontendDependencies(
  applicationFiles,
  (dependency) =>
    isFrontendPath(dependency, 'types/index.ts')
    || isFrontendPath(dependency, 'api.ts')
    || isFrontendPath(dependency, 'tauriInvoke.ts')
    || isFrontendDirectory(dependency, 'stores')
    || isFrontendDirectory(dependency, 'composables')
    || isFrontendDirectory(dependency, 'platform')
    || ['element-plus', 'vue-router', 'pinia', 'vue', '@tauri-apps'].some((packageName) =>
      isExternalPackage(dependency, packageName)),
  'the resolved UI-independent frontend application boundary',
);

rejectFrontendDependencies(
  [join(frontendRoot, 'types', 'index.ts')],
  (dependency) =>
    isFrontendPath(dependency, 'types/generated.ts')
    || isFrontendPath(dependency, 'types/protocol.ts'),
  'the resolved runtime-only frontend type barrel boundary',
);

rejectFrontendDependencies(
  frontendFiles.filter((file) => file !== join(frontendRoot, 'types', 'protocol.ts')),
  (dependency) => isFrontendPath(dependency, 'types/generated.ts'),
  'the resolved single-entry frontend generated protocol boundary',
);

rejectFrontendDependencies(
  sourceFiles(join(frontendRoot, 'components', 'table-grid'), '.vue'),
  (dependency) => isFrontendPath(dependency, 'components/table-grid/index.ts'),
  'the resolved acyclic table-grid component boundary',
);

const dependencyParserProbe = moduleDependenciesFromSource(
  join(frontendRoot, 'application', '__architecture_probe__.ts'),
  `
    import '../stores/documentSession';
    export { useDocumentSessionStore } from '@/stores/documentSession';
    void import('../stores/documentSession');
  `,
);
if (
  dependencyParserProbe.length !== 3
  || dependencyParserProbe.some((dependency) =>
    !isFrontendPath(dependency, 'stores/documentSession.ts'))
) {
  violations.push('architecture dependency parser does not normalize relative, aliased, and dynamic imports');
}

const transitiveProbeRoot = join(frontendRoot, 'application', '__transitive_probe__.ts');
const transitiveProbeBridge = join(frontendRoot, 'utils', '__transitive_bridge__.ts');
const transitiveProbeGraph = new Map([
  [transitiveProbeRoot, moduleDependenciesFromSource(
    transitiveProbeRoot,
    `import '../../src/utils/__transitive_bridge__.ts';`,
  )],
  [transitiveProbeBridge, moduleDependenciesFromSource(
    transitiveProbeBridge,
    `export * from '../stores/documentSession';`,
  )],
]);
const transitiveProbePath = findForbiddenFrontendDependencyPath(
  transitiveProbeRoot,
  (dependency) => isFrontendDirectory(dependency, 'stores'),
  transitiveProbeGraph,
);
if (!transitiveProbePath || transitiveProbePath.length !== 2) {
  violations.push('architecture dependency graph does not reject an indirect re-export boundary bypass');
}

for (const cycle of frontendDependencyCycles()) {
  violations.push(
    `frontend module dependency cycle: ${cycle.map((file) => relative(projectRoot, file)).join(' -> ')}`,
  );
}

const rustApplicationFiles = sourceFiles(join(rustRoot, 'application'), '.rs');
for (const file of rustApplicationFiles) {
  const path = findForbiddenRustDependencyPath(
    file,
    (dependency) => {
      const target = relative(rustRoot, dependency);
      return target === 'runtime.rs'
        || target.startsWith('adapters/')
        || target.startsWith('commands/')
        || target.startsWith('io/')
        || target.startsWith('recent/');
    },
  );
  if (!path) continue;
  violations.push(
    `${relative(projectRoot, file)} violates the transitive inward-only Rust application boundary: ${[file, ...path]
      .map((dependency) => relative(projectRoot, dependency))
      .join(' -> ')}`,
  );
}

for (const cycle of rustDependencyCycles()) {
  violations.push(
    `Rust production module dependency cycle: ${cycle
      .map((file) => relative(projectRoot, file))
      .join(' -> ')}`,
  );
}

const rustDependencyParserProbe = rustModuleDependenciesFromSource(
  join(rustRoot, 'adapters', '__architecture_probe__.rs'),
  `
    use crate::{
      adapters::search_index_runtime::SearchIndexRuntime as Runtime,
      application::{search_ports::SearchQueryPort as QueryPort},
    };
  `,
);
if (
  !rustDependencyParserProbe.includes(join(rustRoot, 'adapters', 'search_index_runtime.rs'))
  || !rustDependencyParserProbe.includes(join(rustRoot, 'application', 'search_ports.rs'))
) {
  violations.push('Rust architecture dependency parser does not resolve grouped or aliased imports');
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
  sourceFiles(join(projectRoot, 'src-tauri', 'src', 'types'), '.rs')
    .filter((file) => !file.endsWith('/typescript.rs')),
  [/^use\s+crate::types(?:::|\s*\{)/m],
  'the direct-sibling Rust protocol dependency boundary',
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
    join(projectRoot, 'src-tauri', 'src', 'application', 'mutation_intent.rs'),
    join(projectRoot, 'src-tauri', 'src', 'application', 'mutation_replay.rs'),
    join(projectRoot, 'src-tauri', 'src', 'application', 'editor_command_service.rs'),
  ],
  [/\bserde(?:::|\b)/, /\bserde_json(?:::|\b)/, /\bSerialize\b/, /\bDeserialize\b/],
  'the semantic-mutation-fingerprint application boundary',
);

rejectRustProductionMatches(
  [join(projectRoot, 'src-tauri', 'src', 'application', 'mutation_replay.rs')],
  [
    /\bMutationRequestIdentity\b/,
    /\bEditorCommand\b/,
    /\bMutationPatch\b/,
    /\bCellValue\b/,
    /\bsha2\b/,
    /\bFingerprintWriter\b/,
  ],
  'the intent-agnostic mutation replay boundary',
);

const mutationIntentSource = readFileSync(
  join(projectRoot, 'src-tauri', 'src', 'application', 'mutation_intent.rs'),
  'utf8',
);
for (const requirement of [
  /enum\s+MutationIntent[\s\S]*Undo[\s\S]*Redo[\s\S]*Execute\(EditorCommand\)/,
  /fn\s+fingerprint\s*\(/,
  /fn\s+write_editor_command\s*\(/,
]) {
  if (!requirement.test(mutationIntentSource)) {
    violations.push(
      `src-tauri/src/application/mutation_intent.rs violates the canonical mutation intent boundary: ${requirement}`,
    );
  }
}

const mutationReplaySource = readFileSync(
  join(projectRoot, 'src-tauri', 'src', 'application', 'mutation_replay.rs'),
  'utf8',
);
for (const requirement of [
  /let\s+fingerprint\s*=\s*intent\.fingerprint\(base_revision\)/,
  /let\s+result\s*=\s*execute\(intent\)/,
]) {
  if (!requirement.test(mutationReplaySource)) {
    violations.push(
      `src-tauri/src/application/mutation_replay.rs violates the same-intent replay/execution boundary: ${requirement}`,
    );
  }
}

if (existsSync(join(rustRoot, 'application', 'runtime.rs'))) {
  violations.push('src-tauri/src/application/runtime.rs violates the top-level Rust composition-root boundary');
}

if (existsSync(join(rustRoot, 'adapters', 'search_index_scheduler.rs'))) {
  violations.push(
    'src-tauri/src/adapters/search_index_scheduler.rs violates the SearchIndexRuntime-owned scheduler boundary',
  );
}

rejectRustProductionMatches(
  [join(rustRoot, 'adapters', 'search_index_store.rs')],
  [/\bSearchQueryPlan\b/, /\bMAX_SEARCH_QUERY_BYTES\b/, /\bMAX_SEARCH_RESPONSE_BYTES\b/],
  'the query-semantics-free search index storage boundary',
);

rejectRustProductionMatches(
  [join(rustRoot, 'adapters', 'search_query_engine.rs')],
  [/\bMAX_SEARCH_RESPONSE_BYTES\b/, /\bserialized_json_bytes\b/, /crate::types(?:::|\b)/],
  'the transport-response-independent search query engine boundary',
);

for (const file of rustFiles.filter((candidate) => ![
  join(rustRoot, 'editor_protocol.rs'),
  join(rustRoot, 'protocol_projection', 'search.rs'),
].includes(candidate))) {
  if (/\bMAX_SEARCH_RESPONSE_BYTES\b/.test(rustProductionSource(file))) {
    violations.push(
      `${relative(projectRoot, file)} violates the protocol-projection-owned search response budget boundary`,
    );
  }
}

rejectRustProductionMatches(
  [join(rustRoot, 'adapters', 'search_index_runtime.rs')],
  [/pub\(crate\)\s+(?:struct\s+IndexScheduler\b|scheduler\s*:)/],
  'the encapsulated search scheduler state boundary',
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

if (existsSync(join(rustRoot, 'recent', 'types.rs'))) {
  violations.push(
    'src-tauri/src/recent/types.rs violates the separated recent persistence and RPC model boundary',
  );
}

rejectRustProductionMatches(
  [join(rustRoot, 'recent', 'model.rs')],
  [/\bserde(?:::|\b)/, /\bts_rs\b/, /\bSerialize\b/, /\bDeserialize\b/, /crate::types(?:::|\b)/],
  'the serialization-independent recent-file model boundary',
);

rejectRustProductionMatches(
  [
    join(rustRoot, 'recent', 'store.rs'),
    join(rustRoot, 'adapters', 'recent_file_adapter.rs'),
  ],
  [/\bts_rs\b/, /crate::types(?:::|\b)/, /\bAddRecentFileRequest\b/, /\bRecentFile\b/],
  'the RPC-independent recent-file persistence and adapter boundary',
);

rejectMatches(
  [join(rustRoot, 'types', 'typescript.rs')],
  [/crate::recent(?:::|\b)/],
  'the protocol-owned recent-file TypeScript declaration boundary',
);

const recentCommandSource = readFileSync(join(rustRoot, 'commands', 'recent.rs'), 'utf8');
for (const requirement of [
  /protocol_projection::add_recent_file_input/,
  /protocol_projection::recent_file/,
  /protocol_projection::recent_files/,
]) {
  if (!requirement.test(recentCommandSource)) {
    violations.push(
      `src-tauri/src/commands/recent.rs violates the explicit recent-file protocol mapping boundary: ${requirement}`,
    );
  }
}

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
  [/crate::document_data(?:::|\b)/, /\bDocumentData\b/, /\bDocumentSheet\b/],
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
  sourceFiles(join(projectRoot, 'src-tauri', 'src', 'document'), '.rs').filter(
    (file) => !relative(projectRoot, file).endsWith('/test_support.rs'),
  ),
  [/crate::io(?:::|\b)/],
  'the port-mediated Rust document backing boundary',
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

const documentProtocolProjectionSource = readFileSync(
  join(projectRoot, 'src-tauri', 'src', 'protocol_projection', 'document.rs'),
  'utf8',
);
for (const requirement of [
  /open_document_response[\s\S]*MAX_DOCUMENT_RESPONSE_BYTES/,
  /saved_document_response[\s\S]*MAX_DOCUMENT_RESPONSE_BYTES/,
  /response\.initial_region\s*=\s*None/,
  /serialized_json_bytes\s*\(\s*&?response\s*\)/,
]) {
  if (!requirement.test(documentProtocolProjectionSource)) {
    violations.push(
      `src-tauri/src/protocol_projection/document.rs violates the bounded whole-document response boundary: ${requirement}`,
    );
  }
}

rejectMatches(
  [join(rustRoot, 'protocol_projection.rs')],
  [/^use\s+crate::/m, /^(?:pub(?:\(crate\))?\s+)?fn\s+/m],
  'the declaration-only Rust protocol projection facade boundary',
);

rejectMatches(
  [
    join(rustRoot, 'protocol_projection', 'document.rs'),
    join(rustRoot, 'protocol_projection', 'editor.rs'),
    join(rustRoot, 'protocol_projection', 'file.rs'),
    join(rustRoot, 'protocol_projection', 'recent.rs'),
    join(rustRoot, 'protocol_projection', 'search.rs'),
    join(rustRoot, 'protocol_projection', 'update.rs'),
  ],
  [/super::(?:document|editor|file|recent|search|update)(?:::|\b)/],
  'the feature-isolated Rust protocol projection boundary',
);

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
  /\bcreateApplicationWorkspaceRuntime\b/,
  /\.provide\s*\(\s*applicationWorkspaceRuntimeKey\b/,
  /\.provide\s*\(\s*applicationExitCoordinatorKey\b/,
  /\.provide\s*\(\s*documentWorkspaceRuntimeKey\b/,
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
  sourceFiles(join(projectRoot, 'src-tauri', 'src', 'application'), '.rs'),
  [/\bApplicationRuntime\b/],
  'the narrow Rust application service dependency boundary',
);

rejectMatches(
  sourceFiles(join(projectRoot, 'src-tauri', 'src', 'application'), '.rs'),
  [
    /\btauri::(?:AppHandle|State)\b/,
    /crate::io(?:::|\b)/,
    /crate::adapters(?:::|\b)/,
  ],
  'the framework-independent Rust application service boundary',
);

rejectMatches(
  [join(projectRoot, 'src-tauri', 'src', 'runtime.rs')],
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
    join(projectRoot, 'src-tauri', 'src', 'adapters', 'search_query_engine.rs'),
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

const documentWorkspaceRuntimeSource = readFileSync(
  join(projectRoot, 'src', 'composables', 'documentWorkspaceRuntime.ts'),
  'utf8',
);
for (const requirement of [
  /new\s+WeakMap\b/,
  /\bcreateDocumentSessionCoordinator\b/,
  /\bcreateDocumentRegionCoordinator\b/,
  /\bcreatePendingCellSaveCoordinator\b/,
  /\bcreateSearchSessionCoordinator\b/,
  /\bcreateDocumentCommandBus\b/,
  /\bcreateDocumentPreparationCoordinator\b/,
  /sessionWorkflow\.waitForMutations\s*\(/,
  /pendingCellSaves\.waitForInFlightSave\s*\(/,
  /preparations\.waitForIdle\s*\(/,
  /regions\.waitForIdle\s*\(/,
]) {
  if (!requirement.test(documentWorkspaceRuntimeSource)) {
    violations.push(
      `src/composables/documentWorkspaceRuntime.ts violates the explicit document workspace ownership boundary: ${requirement}`,
    );
  }
}

const applicationWorkspaceRuntimeSource = readFileSync(
  join(projectRoot, 'src', 'composables', 'applicationWorkspaceRuntime.ts'),
  'utf8',
);
for (const requirement of [
  /new\s+WeakMap\b/,
  /\bcreateDocumentWorkspaceRuntime\b/,
  /\bcreateRecentFilesService\b/,
  /\bcreateUpdateCoordinator\b/,
  /\bcreateApplicationExitCoordinator\b/,
  /document\.dispose\s*\(/,
  /recentFiles\.dispose\s*\(/,
  /updateCoordinator\?\.dispose\s*\(/,
]) {
  if (!requirement.test(applicationWorkspaceRuntimeSource)) {
    violations.push(
      `src/composables/applicationWorkspaceRuntime.ts violates the explicit application workspace ownership boundary: ${requirement}`,
    );
  }
}

rejectMatches(
  [
    join(projectRoot, 'src', 'composables', 'useRecentFilesService.ts'),
    join(projectRoot, 'src', 'composables', 'useUpdateCoordinator.ts'),
  ],
  [/new\s+WeakMap\b/, /\bcreateRecentFilesService\b/, /\bcreateUpdateCoordinator\b/],
  'the centralized frontend application workspace ownership boundary',
);

rejectMatches(
  [join(projectRoot, 'src', 'composables', 'useHomeFileActions.ts')],
  [/\buseDocumentLifecycle\b/, /\brunDocumentLifecycle\b/],
  'the document-file-workflow-owned lifecycle boundary',
);

rejectMatches(
  [
    join(projectRoot, 'src', 'composables', 'useDocumentCommandBus.ts'),
    join(projectRoot, 'src', 'composables', 'useDocumentSessionCoordinator.ts'),
    join(projectRoot, 'src', 'composables', 'usePendingCellSaveCoordinator.ts'),
    join(projectRoot, 'src', 'composables', 'useSearchSessionCoordinator.ts'),
    join(projectRoot, 'src', 'composables', 'useDocumentFileCoordinator.ts'),
  ],
  [/new\s+WeakMap\b/],
  'the centralized frontend document workspace ownership boundary',
);

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
  ['src/composables/documentCommandBusAdapter.ts', /\brefreshEditorState\b/],
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
  /\bGENERATED_MAX_SET_CELL_CHANGES\b/,
  /\bGENERATED_MAX_CELL_TEXT_BYTES\b/,
  /\bGENERATED_MAX_MUTATION_TEXT_BYTES\b/,
  /\bGENERATED_MAX_SEARCH_QUERY_BYTES\b/,
  /\bGENERATED_MAX_DOCUMENT_RESPONSE_BYTES\b/,
  /\bGENERATED_MAX_SHEET_REGION_RESPONSE_BYTES\b/,
  /\bGENERATED_SHEET_REGION_TILE_ROWS\b/,
  /\bGENERATED_SHEET_REGION_TILE_COLUMNS\b/,
  /\bassertEditorResourcePolicyCompatibility\b/,
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
  /function\s+waitForIdle\b/,
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
  /\buseDocumentWorkspaceRuntime\b/,
  /workspace\.preparations\b/,
]) {
  if (!requirement.test(useDocumentFileCoordinatorSource)) {
    violations.push(
      `src/composables/useDocumentFileCoordinator.ts violates the workspace-owned document preparation boundary: ${requirement}`,
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
  join(projectRoot, 'src-tauri', 'src', 'runtime.rs'),
  'utf8',
);
if (!/DocumentOpenService::new\([\s\S]*work_budget\.clone\(\)[\s\S]*DocumentSaveService::new\([\s\S]*work_budget/m.test(documentWorkRuntimeSource)) {
  violations.push('src-tauri/src/runtime.rs does not share one document work budget across open and save services');
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
    join(projectRoot, 'src-tauri', 'src', 'adapters', 'search_query_engine.rs'),
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

rejectMatches(
  [join(projectRoot, 'src-tauri', 'src', 'adapters', 'search_index_runtime.rs')],
  [/search_query_adapter/],
  'the acyclic search runtime boundary',
);

const searchQueryEngineSource = readFileSync(
  join(projectRoot, 'src-tauri', 'src', 'adapters', 'search_query_engine.rs'),
  'utf8',
);
for (const requirement of [
  /struct\s+SearchQueryPlan\b/,
  /fn\s+execute_search\b/,
  /fn\s+scan_sheet_fallback\b/,
  /MAX_SEARCH_OUTCOME_RETAINED_BYTES/,
]) {
  if (!requirement.test(searchQueryEngineSource)) {
    violations.push(
      `src-tauri/src/adapters/search_query_engine.rs violates the extracted search-query engine boundary: ${requirement}`,
    );
  }
}

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
  join(projectRoot, 'src-tauri', 'src', 'runtime.rs'),
  'utf8',
);
for (const requirement of [/\bUpdateService\b/, /\bupdate_queries\b/]) {
  if (!requirement.test(applicationRuntimeSource)) {
    violations.push(
      `src-tauri/src/runtime.rs violates the explicitly-owned update runtime boundary: ${requirement}`,
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
  [/\bMAX_SEARCH_QUERY_BYTES\b/],
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
