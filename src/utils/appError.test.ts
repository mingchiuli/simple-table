import { describe, expect, it } from "vitest";
import { appErrorMessage, isAppErrorCode } from "@/utils/appError";

describe("appError", () => {
  it("reads structured backend errors", () => {
    const error = {
      code: "file_not_found",
      message: "File not found: /tmp/missing.xlsx",
    };

    expect(isAppErrorCode(error, "file_not_found")).toBe(true);
    expect(appErrorMessage(error)).toBe("File not found: /tmp/missing.xlsx");
  });

  it("keeps useful messages for ordinary errors and strings", () => {
    expect(appErrorMessage(new Error("disk full"))).toBe("Error: disk full");
    expect(appErrorMessage("legacy failure")).toBe("legacy failure");
    expect(isAppErrorCode(new Error("file_not_found"), "file_not_found")).toBe(false);
  });
});
