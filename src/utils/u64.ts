import type { U64String } from '@/types';

export const ZERO_U64: U64String = '0';

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
