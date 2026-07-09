import { describe, expect, it } from "vitest";
import {
  DEFAULT_SPREADSHEET_EXTENSION,
  baseNameWithoutExtension,
  extensionFromName,
  supportedSpreadsheetExtension,
} from "@/utils/fileFormats";

describe("fileFormats", () => {
  it("recognizes supported spreadsheet extensions case-insensitively", () => {
    expect(supportedSpreadsheetExtension("book.XLSX")).toBe("xlsx");
    expect(supportedSpreadsheetExtension("/tmp/data.csv")).toBe("csv");
    expect(supportedSpreadsheetExtension("macro.xlsm")).toBeNull();
  });

  it("exposes xlsx as the default extension without treating unsupported extensions as supported", () => {
    expect(DEFAULT_SPREADSHEET_EXTENSION).toBe("xlsx");
    expect(extensionFromName("untitled")).toBeNull();
    expect(extensionFromName("book.bin")).toBe("bin");
    expect(supportedSpreadsheetExtension("book.bin")).toBeNull();
  });

  it("strips only the last filename extension", () => {
    expect(baseNameWithoutExtension("/tmp/report.final.xlsx")).toBe("report.final");
    expect(baseNameWithoutExtension(".hidden")).toBe(".hidden");
    expect(baseNameWithoutExtension("")).toBe("untitled");
  });
});
