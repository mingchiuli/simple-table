import { describe, expect, it } from "vitest";
import {
  baseNameWithoutExtension,
  decodeFileNameSegment,
  extensionFromName,
  filePathFromDeepLinkTarget,
  fileNameFromPathLike,
  supportedSpreadsheetExtension,
} from "@/utils/fileFormats";

describe("fileFormats", () => {
  it("recognizes supported spreadsheet extensions case-insensitively", () => {
    const supported = ["xlsx", "csv"];
    expect(supportedSpreadsheetExtension("book.XLSX", supported)).toBe("xlsx");
    expect(supportedSpreadsheetExtension("/tmp/data.csv", supported)).toBe("csv");
    expect(supportedSpreadsheetExtension("macro.xlsm", supported)).toBeNull();
  });

  it("does not treat unsupported extensions as supported", () => {
    const supported = ["xlsx", "csv"];
    expect(extensionFromName("untitled")).toBeNull();
    expect(extensionFromName("book.bin")).toBe("bin");
    expect(supportedSpreadsheetExtension("book.bin", supported)).toBeNull();
  });

  it("strips only the last filename extension", () => {
    expect(baseNameWithoutExtension("/tmp/report.final.xlsx")).toBe("report.final");
    expect(baseNameWithoutExtension(".hidden")).toBe(".hidden");
    expect(baseNameWithoutExtension("")).toBe("untitled");
  });

  it("extracts display filenames from path-like strings safely", () => {
    expect(fileNameFromPathLike("C:\\Users\\me\\report%20final.xlsx")).toBe(
      "report final.xlsx"
    );
    expect(fileNameFromPathLike("content://provider/folder/report.csv?token=1")).toBe(
      "report.csv"
    );
    expect(
      fileNameFromPathLike(
        "content://provider/document/primary%3ADownload%2Freports%2Fscore.xlsx"
      )
    ).toBe("score.xlsx");
    expect(fileNameFromPathLike("/tmp/folder/")).toBe("folder");
    expect(fileNameFromPathLike("", "unknown")).toBe("unknown");
  });

  it("does not throw on malformed percent escapes in filenames", () => {
    expect(decodeFileNameSegment("100% complete.xlsx")).toBe("100% complete.xlsx");
    expect(fileNameFromPathLike("/tmp/100% complete.xlsx")).toBe("100% complete.xlsx");
    expect(extensionFromName("/tmp/100% complete.xlsx")).toBe("xlsx");
  });

  it("normalizes file deep links into platform paths", () => {
    expect(filePathFromDeepLinkTarget("file:///Users/me/report%20final.xlsx")).toBe(
      "/Users/me/report final.xlsx"
    );
    expect(filePathFromDeepLinkTarget("FILE:///Users/me/report.xlsx")).toBe(
      "/Users/me/report.xlsx"
    );
    expect(filePathFromDeepLinkTarget("file:///C:/Users/me/report.xlsx")).toBe(
      "C:/Users/me/report.xlsx"
    );
    expect(filePathFromDeepLinkTarget("file://server/share/report.xlsx")).toBe(
      "//server/share/report.xlsx"
    );
    expect(filePathFromDeepLinkTarget("C:\\Users\\me\\report.xlsx")).toBe(
      "C:\\Users\\me\\report.xlsx"
    );
  });
});
