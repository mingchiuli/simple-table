import { MAX_SHEET_REGION_RESPONSE_BYTES } from './generated';
import type {
  CellValue,
  EditorCommandContext,
  ReadOnlyRichProjection,
  SearchResult,
  SheetExtent,
  SheetRegion,
  SheetRegionMetadata,
  U64String,
} from './generated';

export type {
  CellValue,
  EditorCommandContext,
  SheetExtent,
  SheetRegion,
  U64String,
} from './generated';

export type DocumentSessionLifecycle = 'idle' | 'loading' | 'saving' | 'closing';

export const MAX_RESIDENT_REGION_BYTES = MAX_SHEET_REGION_RESPONSE_BYTES;

export type SheetLayoutState = {
  columnWidths: Record<number, number>;
  rowHeights: Record<number, number>;
};

export type MutationCommandContext = EditorCommandContext & {
  commandId: string;
};

export type LoadedSheetSlot = {
  state: 'loaded';
  name: string;
  extent: SheetExtent;
  layout: SheetLayoutState;
  blocks: SheetRegionBlock[];
  metadata: LoadedSheetRegionMetadata;
};

export type LoadedSheetRegionMetadata = {
  merges: NonNullable<SheetRegionMetadata['merges']>;
  rich: ReadOnlyRichProjection;
};

export type SheetRegionBlock = {
  key: string;
  region: SheetRegion;
  cells: Record<string, CellValue>;
  mergeAnchorCells: Record<string, CellValue>;
  metadata: SheetRegionMetadata;
  estimatedBytes: number;
};

export type UnloadedSheetSlot = {
  state: 'unloaded';
  name: string;
  extent: SheetExtent;
  layout: SheetLayoutState;
};

export type SheetSlot = LoadedSheetSlot | UnloadedSheetSlot;

export type DocumentProjection = {
  path: string;
  fileName: string;
  sheets: SheetSlot[];
};

export type DocumentSessionStateInput = {
  data: DocumentProjection;
  currentFilePath: string | null;
  documentId: U64String;
  revision: U64String;
  preferredSheetIndex: number;
  activatePreferredSheet: boolean;
  resetEditorCommandDepth: boolean;
  preserveResidentSheetOrder: boolean;
};

export type DocumentMutationStateInput = {
  data: DocumentProjection | null;
  documentId: U64String;
  revision: U64String;
  resyncRequired: boolean;
};

export type DocumentIdentityStateInput = {
  documentId: U64String;
  revision: U64String;
};

export type SearchSessionSnapshot = {
  searchResults: SearchResult[];
  searchResultsTruncated: boolean;
  searchQuery: string;
  isSearching: boolean;
};
