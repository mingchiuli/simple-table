import {
  MAX_DOCUMENT_PROJECTION_RESIDENT_BYTES,
  MAX_REGION_STAGING_WIRE_BYTES,
} from '@/resourcePolicy/editorMemoryPolicy';

export type RegionStagingLease = {
  reserve(residentBytes: number, wireBytes: number): void;
  release(): void;
};

type RegionStagingUsage = {
  residentBytes: number;
  wireBytes: number;
};

export function createDocumentRegionStagingBudget() {
  const usage: RegionStagingUsage = {
    residentBytes: 0,
    wireBytes: 0,
  };

  function acquire(): RegionStagingLease {
    let leasedResidentBytes = 0;
    let leasedWireBytes = 0;
    let released = false;

    return {
      reserve(residentBytes: number, wireBytes: number) {
        if (released) throw new Error('Region staging lease is already released');
        if (!isByteCount(residentBytes) || !isByteCount(wireBytes)) {
          throw new RegionStagingLimitError('Region staging byte counts must be finite integers');
        }
        const nextResidentBytes = usage.residentBytes + residentBytes;
        const nextWireBytes = usage.wireBytes + wireBytes;
        if (nextResidentBytes > MAX_DOCUMENT_PROJECTION_RESIDENT_BYTES) {
          throw new RegionStagingLimitError(
            `Region staging requires ${nextResidentBytes} resident bytes; maximum is ${MAX_DOCUMENT_PROJECTION_RESIDENT_BYTES}`,
          );
        }
        if (nextWireBytes > MAX_REGION_STAGING_WIRE_BYTES) {
          throw new RegionStagingLimitError(
            `Region staging requires ${nextWireBytes} wire bytes; maximum is ${MAX_REGION_STAGING_WIRE_BYTES}`,
          );
        }
        usage.residentBytes = nextResidentBytes;
        usage.wireBytes = nextWireBytes;
        leasedResidentBytes += residentBytes;
        leasedWireBytes += wireBytes;
      },
      release() {
        if (released) return;
        released = true;
        usage.residentBytes = Math.max(0, usage.residentBytes - leasedResidentBytes);
        usage.wireBytes = Math.max(0, usage.wireBytes - leasedWireBytes);
      },
    };
  }

  return { acquire };
}

export class RegionStagingLimitError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'RegionStagingLimitError';
  }
}

function isByteCount(value: number): boolean {
  return Number.isSafeInteger(value) && value >= 0;
}
