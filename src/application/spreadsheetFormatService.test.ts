import { describe, expect, it, vi } from 'vitest';

import { createSpreadsheetFormatService } from '@/application/spreadsheetFormatService';

describe('spreadsheetFormatService', () => {
  it('loads spreadsheet format options through its port once', async () => {
    const getSpreadsheetFormatOptions = vi.fn().mockResolvedValue({
      defaultExtension: 'xlsx',
      supportedExtensions: ['xlsx', 'csv'],
    });
    const formats = createSpreadsheetFormatService({ getSpreadsheetFormatOptions });

    await expect(formats.defaultSpreadsheetExtension()).resolves.toBe('xlsx');
    await expect(formats.supportedSpreadsheetExtensions()).resolves.toEqual(['xlsx', 'csv']);
    expect(getSpreadsheetFormatOptions).toHaveBeenCalledTimes(1);
  });

  it('does not permanently cache failed format loading', async () => {
    const getSpreadsheetFormatOptions = vi.fn()
      .mockRejectedValueOnce(new Error('temporarily unavailable'))
      .mockResolvedValueOnce({
        defaultExtension: 'xlsx',
        supportedExtensions: ['xlsx', 'csv'],
      });
    const formats = createSpreadsheetFormatService({ getSpreadsheetFormatOptions });

    await expect(formats.defaultSpreadsheetExtension()).rejects.toThrow(
      'temporarily unavailable',
    );
    await expect(formats.defaultSpreadsheetExtension()).resolves.toBe('xlsx');
    expect(getSpreadsheetFormatOptions).toHaveBeenCalledTimes(2);
  });
});
