import { readFileSync } from 'node:fs';
import { relative } from 'node:path';

export function createRustDependencyGraph(rustRoot, rustFiles) {
  const modulesByPath = new Map(
    rustFiles.map((file) => [rustModuleSegments(rustRoot, file).join('::'), file]),
  );

  function resolveModule(segments) {
    for (let length = segments.length; length > 0; length -= 1) {
      const file = modulesByPath.get(segments.slice(0, length).join('::'));
      if (file) return file;
    }
    return null;
  }

  function dependenciesFromSource(file, source) {
    const tokens = rustTokens(source);
    const currentModule = rustModuleSegments(rustRoot, file);
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
        const dependency = resolveModule(modulePath);
        if (dependency && dependency !== file) dependencies.add(dependency);
      }
    }
    return [...dependencies];
  }

  const dependencies = new Map(
    rustFiles.map((file) => [file, dependenciesFromSource(file, rustProductionSource(file))]),
  );
  const externalDependencies = new Map(
    rustFiles.map((file) => [file, externalDependenciesFromSource(rustProductionSource(file))]),
  );

  return {
    dependencies,
    externalDependencies,
    dependenciesFromSource,
    externalDependenciesFromSource,
    findForbiddenPath: (start, forbidden) => findForbiddenPath(start, dependencies, forbidden),
    cycles: () => dependencyCycles(dependencies, (dependency) => dependency),
  };
}

function externalDependenciesFromSource(source) {
  const tokens = rustTokens(source);
  const dependencies = new Set();
  for (let index = 0; index < tokens.length - 1; index += 1) {
    const root = tokens[index];
    if (root === 'use') {
      addExternalDependency(dependencies, tokens[index + 1]);
    } else if (root === 'extern' && tokens[index + 1] === 'crate') {
      addExternalDependency(dependencies, tokens[index + 2]);
    }
    if (
      isRustIdentifier(root)
      && tokens[index + 1] === '::'
    ) {
      addExternalDependency(dependencies, root);
    }
  }
  return [...dependencies];
}

function addExternalDependency(dependencies, candidate) {
  if (
    isRustIdentifier(candidate)
    && !['crate', 'self', 'super', 'std', 'core', 'alloc'].includes(candidate)
  ) {
    dependencies.add(candidate);
  }
}

export function rustProductionSource(file) {
  return readFileSync(file, 'utf8')
    .split(/\n#\[cfg\(test\)\]\nmod tests\s*\{/)[0]
    .replace(/#\[cfg\(test\)\]\s*(?:pub(?:\([^)]*\))?\s+)?use[\s\S]*?;/g, '');
}

function rustModuleSegments(rustRoot, file) {
  const segments = relative(rustRoot, file).replace(/\.rs$/, '').split('/');
  if (segments.at(-1) === 'lib') return [];
  if (segments.at(-1) === 'mod') segments.pop();
  return segments;
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

function findForbiddenPath(start, graph, forbidden) {
  const visited = new Set([start]);
  const pending = [{ file: start, path: [] }];
  while (pending.length > 0) {
    const current = pending.shift();
    for (const dependency of graph.get(current.file) ?? []) {
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

function dependencyCycles(graph, targetOf) {
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
      const target = targetOf(dependency);
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
