import type { U64String } from '@/types/documentRuntime';

export const ZERO_U64: U64String = '0';
const U64_MAX = 18_446_744_073_709_551_615n;
const CANONICAL_U64 = /^(0|[1-9][0-9]*)$/;

export function isU64String(value: unknown): value is U64String {
  if (typeof value !== 'string' || !CANONICAL_U64.test(value)) return false;
  try {
    return BigInt(value) <= U64_MAX;
  } catch {
    return false;
  }
}

export function assertU64String(value: unknown, field: string): asserts value is U64String {
  if (!isU64String(value)) {
    throw new TypeError(`${field} must be a canonical unsigned 64-bit decimal string`);
  }
}

export function compareU64(left: U64String, right: U64String): number {
  const leftValue = BigInt(left);
  const rightValue = BigInt(right);
  return leftValue < rightValue ? -1 : leftValue > rightValue ? 1 : 0;
}

export function isNextU64(value: U64String, previous: U64String): boolean {
  return BigInt(value) === BigInt(previous) + 1n;
}

export function maxU64(left: U64String, right: U64String): U64String {
  return compareU64(left, right) >= 0 ? left : right;
}
