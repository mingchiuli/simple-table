import { describe, expect, it } from 'vitest';
import { assertU64String, compareU64, isNextU64, isU64String, maxU64 } from '@/utils/u64';

describe('u64 helpers', () => {
  it('compares values beyond the JavaScript safe integer range without losing precision', () => {
    const lower = '9007199254740992';
    const higher = '9007199254740993';

    expect(compareU64(higher, lower)).toBe(1);
    expect(compareU64(lower, higher)).toBe(-1);
    expect(isNextU64(higher, lower)).toBe(true);
    expect(maxU64(lower, higher)).toBe(higher);
  });

  it('supports the full unsigned 64-bit range', () => {
    expect(compareU64('18446744073709551615', '18446744073709551614')).toBe(1);
    expect(isNextU64('18446744073709551615', '18446744073709551614')).toBe(true);
  });
});

describe('u64 wire validation', () => {
  it('accepts only canonical values in the Rust u64 range', () => {
    expect(isU64String('0')).toBe(true);
    expect(isU64String('18446744073709551615')).toBe(true);
    expect(isU64String('01')).toBe(false);
    expect(isU64String('-1')).toBe(false);
    expect(isU64String('18446744073709551616')).toBe(false);
    expect(() => assertU64String(1, 'revision')).toThrow(TypeError);
  });
});
