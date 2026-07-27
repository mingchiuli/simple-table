import { readdirSync } from 'node:fs';
import { extname, join } from 'node:path';

export function sourceFiles(directory, extension) {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return sourceFiles(path, extension);
    if (extname(entry.name) !== extension || entry.name.includes('.test.')) return [];
    return [path];
  });
}
