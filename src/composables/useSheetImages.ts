import { getImageBytes, getSheetImages } from '@/api';
import { runtimeSheetImage } from '@/application/documentProjectionProtocol';
import { useDocumentSessionStore } from '@/stores/documentSession';
import type { U64String } from '@/types/protocol';
import type { SheetImage } from '@/types/documentRuntime';

const PAGE_SIZE = 256;
const MAX_CATALOG_IMAGES = 4096;
const MAX_CACHE_ENTRIES = 32;
const MAX_CACHE_BYTES = 64 * 1024 * 1024;

type CachedImage = {
  url: string;
  byteLength: number;
};

const blobCache = new Map<string, CachedImage>();
let cachedBytes = 0;
let cachedDocumentId: U64String | null = null;

function clearBlobCache() {
  for (const cached of blobCache.values()) URL.revokeObjectURL(cached.url);
  blobCache.clear();
  cachedBytes = 0;
}

function activateDocument(documentId: U64String) {
  if (cachedDocumentId === documentId) return;
  clearBlobCache();
  cachedDocumentId = documentId;
}

function cachedUrl(mediaId: string): string | undefined {
  const cached = blobCache.get(mediaId);
  if (!cached) return undefined;
  blobCache.delete(mediaId);
  blobCache.set(mediaId, cached);
  return cached.url;
}

function cacheBlob(mediaId: string, mimeType: string, bytes: ArrayBuffer): string {
  const existing = cachedUrl(mediaId);
  if (existing) return existing;

  const url = URL.createObjectURL(new Blob([bytes], { type: mimeType }));
  blobCache.set(mediaId, { url, byteLength: bytes.byteLength });
  cachedBytes += bytes.byteLength;
  while (blobCache.size > MAX_CACHE_ENTRIES || cachedBytes > MAX_CACHE_BYTES) {
    const oldest = blobCache.entries().next().value as [string, CachedImage] | undefined;
    if (!oldest) break;
    blobCache.delete(oldest[0]);
    cachedBytes -= oldest[1].byteLength;
    URL.revokeObjectURL(oldest[1].url);
  }
  return url;
}

export function useSheetImages(sheetIndex: Ref<number>) {
  const document = useDocumentSessionStore();
  const images = shallowRef<SheetImage[]>([]);
  const imageUrls = shallowRef<Readonly<Record<string, string>>>({});
  const loading = ref(false);
  const loadingMedia = new Map<string, number>();
  let requestSequence = 0;

  async function refresh(): Promise<void> {
    const context = document.currentCommandContext();
    const targetSheet = sheetIndex.value;
    const sequence = ++requestSequence;
    if (!context) {
      clearBlobCache();
      cachedDocumentId = null;
      images.value = [];
      imageUrls.value = {};
      return;
    }

    activateDocument(context.documentId);
    loading.value = true;
    try {
      const catalog: SheetImage[] = [];
      let offset = 0;
      while (catalog.length < MAX_CATALOG_IMAGES) {
        const page = await getSheetImages(context, targetSheet, offset, PAGE_SIZE);
        catalog.push(...page.items.map(runtimeSheetImage));
        if (page.nextOffset === undefined) break;
        offset = page.nextOffset;
      }
      if (!isCurrent(sequence, context.documentId, context.baseRevision, targetSheet)) return;
      images.value = catalog;
      updateUrlSnapshot();
    } catch (error) {
      if (isCurrent(sequence, context.documentId, context.baseRevision, targetSheet)) {
        images.value = [];
        imageUrls.value = {};
        console.error('Failed to load sheet images:', error);
      }
    } finally {
      if (sequence === requestSequence) loading.value = false;
    }
  }

  async function loadImageAssets(imageIds: string[]): Promise<void> {
    const context = document.currentCommandContext();
    const targetSheet = sheetIndex.value;
    const sequence = requestSequence;
    if (!context) return;
    const requested = new Set(imageIds);
    const pendingByMedia = new Map<string, SheetImage>();
    for (const image of images.value) {
      if (!requested.has(image.id) || !image.renderable) continue;
      if (cachedUrl(image.mediaId) || loadingMedia.get(image.mediaId) === sequence) continue;
      pendingByMedia.set(image.mediaId, image);
    }
    const pending = [...pendingByMedia.values()];
    for (let index = 0; index < pending.length; index += 4) {
      await Promise.all(pending.slice(index, index + 4).map(async (image) => {
        loadingMedia.set(image.mediaId, sequence);
        try {
          const bytes = await getImageBytes(context, targetSheet, image.id);
          if (!isCurrent(sequence, context.documentId, context.baseRevision, targetSheet)) return;
          cacheBlob(image.mediaId, image.mimeType, bytes);
        } catch (error) {
          console.error(`Failed to load image ${image.id}:`, error);
        } finally {
          if (loadingMedia.get(image.mediaId) === sequence) {
            loadingMedia.delete(image.mediaId);
          }
        }
      }));
      if (!isCurrent(sequence, context.documentId, context.baseRevision, targetSheet)) return;
      updateUrlSnapshot();
    }
  }

  function updateUrlSnapshot() {
    const urls: Record<string, string> = {};
    for (const image of images.value) {
      const cached = blobCache.get(image.mediaId);
      if (cached) urls[image.id] = cached.url;
    }
    imageUrls.value = urls;
  }

  function isCurrent(
    sequence: number,
    documentId: U64String,
    revision: U64String,
    targetSheet: number,
  ): boolean {
    return sequence === requestSequence
      && document.documentId === documentId
      && document.revision === revision
      && sheetIndex.value === targetSheet;
  }

  watch(
    () => [document.documentId, document.revision, sheetIndex.value] as const,
    () => void refresh(),
    { immediate: true },
  );

  onScopeDispose(() => {
    requestSequence += 1;
    loadingMedia.clear();
    clearBlobCache();
    cachedDocumentId = null;
  });

  return { images, imageUrls, loading, refresh, loadImageAssets };
}
