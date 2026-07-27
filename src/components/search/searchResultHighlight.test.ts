import { describe, expect, it } from 'vitest';
import { buildSearchHighlightSegments } from '@/components/search/searchResultHighlight';

describe('searchResultHighlight', () => {
  it('highlights regex metacharacters as literal text and centers the snippet', () => {
    const result = buildSearchHighlightSegments('prefix long a.b suffix', 'a.b');

    expect(result.find((part) => part.highlighted)?.text).toBe('a.b');
    expect(result.map((part) => part.text).join('')).toBe('...ng a.b suf...');
  });

  it('returns HTML metacharacters as text segments for Vue to escape', () => {
    const result = buildSearchHighlightSegments('before <tag> after', '<tag>');

    expect(result.find((part) => part.highlighted)?.text).toBe('<tag>');
    expect(result.map((part) => part.text).join('')).toBe('...e <tag> af...');
  });

  it('preserves original offsets when lowercase expansion changes string length', () => {
    const result = buildSearchHighlightSegments(`${'İ'.repeat(12)}needle`, 'needle');

    expect(result.find((part) => part.highlighted)?.text).toBe('needle');
    expect(result.map((part) => part.text).join('')).toContain('needle');
  });
});
