import { describe, expect, it } from 'vitest';

import { createDocumentRegionStagingBudget } from '@/application/documentRegionStagingBudget';
import {
  MAX_DOCUMENT_PROJECTION_RESIDENT_BYTES,
  MAX_REGION_STAGING_WIRE_BYTES,
} from '@/protocol/editorResourcePolicy';

describe('documentRegionStagingBudget', () => {
  it('bounds resident bytes across concurrent staging leases', () => {
    const budget = createDocumentRegionStagingBudget();
    const first = budget.acquire();
    const second = budget.acquire();
    first.reserve(MAX_DOCUMENT_PROJECTION_RESIDENT_BYTES - 1, 1);

    expect(() => second.reserve(2, 1)).toThrow(/resident bytes/);

    first.release();
    expect(() => second.reserve(MAX_DOCUMENT_PROJECTION_RESIDENT_BYTES, 1)).not.toThrow();
  });

  it('bounds wire bytes and releases a lease idempotently', () => {
    const budget = createDocumentRegionStagingBudget();
    const first = budget.acquire();
    const second = budget.acquire();
    first.reserve(1, MAX_REGION_STAGING_WIRE_BYTES);

    expect(() => second.reserve(1, 1)).toThrow(/wire bytes/);

    first.release();
    first.release();
    expect(() => second.reserve(1, MAX_REGION_STAGING_WIRE_BYTES)).not.toThrow();
  });
});
