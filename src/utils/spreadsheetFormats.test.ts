import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/api", () => ({
  getSpreadsheetFormatOptions: vi.fn(),
}));

describe("spreadsheetFormats", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
  });

  it("loads spreadsheet format options from the backend once", async () => {
    const api = await import("@/api");
    vi.mocked(api.getSpreadsheetFormatOptions).mockResolvedValue({
      defaultExtension: "xlsx",
      supportedExtensions: ["xlsx", "csv"],
    });

    const formats = await import("@/utils/spreadsheetFormats");

    await expect(formats.defaultSpreadsheetExtension()).resolves.toBe("xlsx");
    await expect(formats.supportedSpreadsheetExtensions()).resolves.toEqual(["xlsx", "csv"]);
    await expect(formats.spreadsheetDialogFilters()).resolves.toEqual([
      { name: "Spreadsheet", extensions: ["xlsx", "csv"] },
    ]);
    expect(api.getSpreadsheetFormatOptions).toHaveBeenCalledTimes(1);
  });

  it("does not permanently cache failed backend format loading", async () => {
    const api = await import("@/api");
    vi.mocked(api.getSpreadsheetFormatOptions)
      .mockRejectedValueOnce(new Error("temporarily unavailable"))
      .mockResolvedValueOnce({
        defaultExtension: "xlsx",
        supportedExtensions: ["xlsx", "csv"],
      });

    const formats = await import("@/utils/spreadsheetFormats");

    await expect(formats.defaultSpreadsheetExtension()).rejects.toThrow(
      "temporarily unavailable"
    );
    await expect(formats.defaultSpreadsheetExtension()).resolves.toBe("xlsx");
    expect(api.getSpreadsheetFormatOptions).toHaveBeenCalledTimes(2);
  });
});
