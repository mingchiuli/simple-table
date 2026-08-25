# Sort And Filter Semantics

Sorting and filtering are engine operations exposed through the shared Rust
protocol. Pending debounced cell edits are flushed before either operation.

## Data Region

- The selected non-empty cell determines a contiguous current region.
- Blank rows and columns bound the region; blanks inside the rectangle remain
  part of it.
- The first row is always the header. Only body rows are sorted or hidden.
- A region with no body row is rejected.
- Regions intersecting merged cells, drawings, or images are rejected. Native
  workbook structures whose row relationships cannot be moved safely, such as
  tables, pivots, charts, validations, conditional formatting, comments, or an
  existing native auto-filter, also block sorting.

## Sort Order

Single-column sorts are stable. Equal keys retain their previous row order.
Values use this fixed category order in both directions:

1. numbers
2. text
3. booleans
4. formula errors
5. blanks

The direction reverses values inside a category, not the category order.
Blanks therefore remain last for both ascending and descending sorts.

Text is compared by its Unicode lowercase value and then by the original text.
This gives deterministic case-insensitive grouping without locale-dependent
results. Formula keys use the current cached result; formula errors form the
error category and a formula with a blank cached result is blank.

The complete row segment moves with its key, including cell formatting, style,
and hyperlinks. Relative formula references translate by the row movement;
absolute axes remain fixed. Sorting rejects formulas that are unparseable or
contain unsupported external, table, named-range, or 3D references.

## Filter Rules

Each sheet has at most one current-region filter with zero or more column
conditions. Conditions use AND semantics and support equals, not-equals,
contains, blank, and not-blank. Text matching uses the same Unicode lowercase
folding as sorting. Clearing one column keeps the other conditions; clearing
the sheet filter restores every body row.

Filters store physical hidden-row indexes in engine state. The virtual grid
maps visible ordinals back to physical rows, so edits and selection always use
real workbook coordinates.

## History, Save, And Scale

Sorting physically changes workbook row order, marks the document dirty, is
saved to XLSX, XLSM, or CSV, and records compact forward/inverse permutations
plus moved formula text for undo and redo. It does not write an OOXML
`sortState` instruction.

Filters are session-only state. They share the same ordered undo/redo timeline
as document edits but do not mark the document dirty and are not written to the
workbook. Reopening a file starts with no filters and, as with all edits, no
undo/redo history.

Sorting is not limited to 1,024 rows. Sort keys are prepared once, a stable row
permutation is calculated in memory, and row cycles apply it without cloning
the full table into history. The existing limits remain 250,000 rows per sheet
and 2,000,000 dense cell slots. The 1,024-row constant only bounds one
contiguous or sparse physical-row read request used by viewport loading.
