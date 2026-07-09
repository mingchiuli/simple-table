export function decodeFileNameSegment(segment: string): string {
  try {
    return decodeURIComponent(segment);
  } catch {
    return segment;
  }
}

export function fileNameFromPathLike(name: string, fallback = "untitled"): string {
  const withoutHash = name.split("#", 1)[0] ?? name;
  const withoutQuery = withoutHash.split("?", 1)[0] ?? withoutHash;
  const normalized = withoutQuery.replace(/\\/g, "/").replace(/\/+$/, "");
  const segment = normalized.split("/").pop() ?? "";
  const decoded = decodeFileNameSegment(segment);
  const decodedSegment = decoded.replace(/\\/g, "/").split("/").pop() ?? "";
  return decodedSegment || fallback;
}

export function filePathFromDeepLinkTarget(target: string): string {
  if (!target.match(/^file:/i)) {
    return target;
  }

  const parsed = new URL(target);
  let path = decodeFileNameSegment(parsed.pathname);

  if (parsed.hostname && parsed.hostname !== "localhost") {
    path = `//${parsed.hostname}${path}`;
  } else if (/^\/[A-Za-z]:($|[\\/])/.test(path)) {
    path = path.slice(1);
  }

  return path;
}

export function extensionFromName(name: string): string | null {
  const fileName = fileNameFromPathLike(name, "");
  const dotIndex = fileName.lastIndexOf(".");
  if (dotIndex <= 0 || dotIndex === fileName.length - 1) {
    return null;
  }
  return fileName.slice(dotIndex + 1).toLowerCase();
}

export function supportedSpreadsheetExtension(
  name: string,
  supportedExtensions: readonly string[]
): string | null {
  const extension = extensionFromName(name);
  return extension && supportedExtensions.includes(extension) ? extension : null;
}

export function baseNameWithoutExtension(name: string): string {
  const fileName = fileNameFromPathLike(name);
  const dotIndex = fileName.lastIndexOf(".");
  if (dotIndex <= 0) {
    return fileName;
  }
  return fileName.slice(0, dotIndex) || "untitled";
}

export function isUntitledSpreadsheet(name: string): boolean {
  return baseNameWithoutExtension(name).startsWith("untitled");
}
