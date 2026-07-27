import { join } from 'node:path';

import { moduleDependenciesFromSource } from './frontend-dependency-graph.mjs';

export function dependencyGraphFixtureViolations({
  frontendRoot,
  rustRoot,
  frontendGraph,
  rustGraph,
}) {
  const violations = [];
  const frontendProbe = moduleDependenciesFromSource(
    join(frontendRoot, 'application', '__architecture_probe__.ts'),
    `
      import '../stores/documentSession';
      export { useDocumentSessionStore } from '@/stores/documentSession';
      void import('../stores/documentSession');
    `,
    frontendRoot,
  );
  const documentStore = join(frontendRoot, 'stores', 'documentSession.ts');
  if (
    frontendProbe.length !== 3
    || frontendProbe.some((dependency) => dependency.path !== documentStore)
  ) {
    violations.push(
      'architecture dependency parser does not normalize relative, aliased, and dynamic imports',
    );
  }

  const transitiveRoot = join(frontendRoot, 'application', '__transitive_probe__.ts');
  const transitiveBridge = join(frontendRoot, 'utils', '__transitive_bridge__.ts');
  const transitiveGraph = new Map([
    [transitiveRoot, moduleDependenciesFromSource(
      transitiveRoot,
      `import '../../src/utils/__transitive_bridge__.ts';`,
      frontendRoot,
    )],
    [transitiveBridge, moduleDependenciesFromSource(
      transitiveBridge,
      `export * from '../stores/documentSession';`,
      frontendRoot,
    )],
  ]);
  const transitivePath = frontendGraph.findForbiddenPath(
    transitiveRoot,
    (dependency) => dependency.path?.startsWith(`${join(frontendRoot, 'stores')}/`) ?? false,
    transitiveGraph,
  );
  if (!transitivePath || transitivePath.length !== 2) {
    violations.push('architecture dependency graph does not reject an indirect re-export bypass');
  }

  const rustProbe = rustGraph.dependenciesFromSource(
    join(rustRoot, 'adapters', '__architecture_probe__.rs'),
    `
      use crate::{
        adapters::search_index_runtime::SearchIndexRuntime as Runtime,
        application::{search_ports::SearchQueryPort as QueryPort},
      };
    `,
  );
  if (
    !rustProbe.includes(join(rustRoot, 'adapters', 'search_index_runtime.rs'))
    || !rustProbe.includes(join(rustRoot, 'application', 'search_ports.rs'))
  ) {
    violations.push(
      'Rust architecture dependency parser does not resolve grouped or aliased imports',
    );
  }

  return violations;
}
