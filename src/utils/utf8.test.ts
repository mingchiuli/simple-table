import { describe, expect, it } from 'vitest';
import { truncateUtf8, utf8ByteLength } from '@/utils/utf8';

describe('UTF-8 helpers', () => {
  it('counts multi-byte text using wire bytes', () => {
    expect(utf8ByteLength('a中🙂')).toBe(8);
  });

  it('truncates without splitting a Unicode character', () => {
    expect(truncateUtf8('a中🙂b', 5)).toBe('a中');
    expect(truncateUtf8('a中🙂b', 8)).toBe('a中🙂');
  });
});
