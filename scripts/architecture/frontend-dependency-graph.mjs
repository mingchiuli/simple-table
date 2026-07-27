import { existsSync, readFileSync } from 'node:fs';
import { dirname, extname, join, resolve } from 'node:path';
import { parse as parseVueSfc } from '@vue/compiler-sfc';
import * as ts from 'typescript';

export function createFrontendDependencyGraph(frontendRoot, frontendFiles, onParseError) {
  const dependencies = new Map(
    frontendFiles.map((file) => [
      file,
      frontendModuleDependencies(file, frontendRoot, onParseError),
    ]),
  );
  return {
    dependencies,
    findForbiddenPath: (start, forbidden, graph = dependencies) =>
      findForbiddenPath(start, graph, forbidden),
    cycles: (graph = dependencies) => dependencyCycles(graph),
  };
}

export function moduleDependenciesFromSource(file, source, frontendRoot) {
  const parsed = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const dependencies = [];

  function visit(node) {
    if (
      (ts.isImportDeclaration(node) || ts.isExportDeclaration(node))
      && node.moduleSpecifier
      && ts.isStringLiteralLike(node.moduleSpecifier)
    ) {
      dependencies.push(resolveFrontendDependency(file, node.moduleSpecifier.text, frontendRoot));
    } else if (
      ts.isCallExpression(node)
      && node.expression.kind === ts.SyntaxKind.ImportKeyword
      && node.arguments.length === 1
      && ts.isStringLiteralLike(node.arguments[0])
    ) {
      dependencies.push(resolveFrontendDependency(file, node.arguments[0].text, frontendRoot));
    }
    ts.forEachChild(node, visit);
  }

  visit(parsed);
  return dependencies;
}

function frontendModuleDependencies(file, frontendRoot, onParseError) {
  const source = readFileSync(file, 'utf8');
  if (extname(file) !== '.vue') {
    return moduleDependenciesFromSource(file, source, frontendRoot);
  }
  const { descriptor, errors } = parseVueSfc(source, { filename: file });
  if (errors.length > 0) onParseError(file);
  const script = [descriptor.script?.content, descriptor.scriptSetup?.content]
    .filter(Boolean)
    .join('\n');
  return moduleDependenciesFromSource(file, script, frontendRoot);
}

function resolveFrontendDependency(fromFile, specifier, frontendRoot) {
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

function findForbiddenPath(start, graph, forbidden) {
  const visited = new Set([start]);
  const pending = [{ file: start, path: [] }];
  while (pending.length > 0) {
    const current = pending.shift();
    for (const dependency of graph.get(current.file) ?? []) {
      const path = [...current.path, { from: current.file, dependency }];
      if (forbidden(dependency)) return path;
      if (dependency.path && graph.has(dependency.path) && !visited.has(dependency.path)) {
        visited.add(dependency.path);
        pending.push({ file: dependency.path, path });
      }
    }
  }
  return null;
}

function dependencyCycles(graph) {
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
