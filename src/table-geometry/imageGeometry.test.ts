import { describe, expect, it } from 'vitest';
import {
  EMU_PER_PIXEL,
  imageAnchorForRect,
  imageAnchorRect,
  resizeImageRect,
  type ImageGridGeometry,
} from '@/table-geometry/imageGeometry';

const geometry: ImageGridGeometry = {
  getColumnOffset: (col) => col * 100,
  getRowOffset: (row) => row * 25,
  getColumnIndexAt: (left) => Math.floor(left / 100),
  getRowIndexAt: (top) => Math.floor(top / 25),
};

describe('imageGeometry', () => {
  it('converts one-cell anchors and pixel rectangles in both directions', () => {
    const rect = imageAnchorRect({
      type: 'OneCell',
      data: {
        from: {
          row: 2,
          col: 1,
          rowOffsetEmu: 5 * EMU_PER_PIXEL,
          colOffsetEmu: 10 * EMU_PER_PIXEL,
        },
        widthEmu: 160 * EMU_PER_PIXEL,
        heightEmu: 90 * EMU_PER_PIXEL,
      },
    }, geometry);

    expect(rect).toEqual({ left: 110, top: 55, width: 160, height: 90 });
    expect(imageAnchorForRect(rect, geometry)).toEqual({
      type: 'OneCell',
      data: {
        from: {
          row: 2,
          col: 1,
          rowOffsetEmu: 5 * EMU_PER_PIXEL,
          colOffsetEmu: 10 * EMU_PER_PIXEL,
        },
        widthEmu: 160 * EMU_PER_PIXEL,
        heightEmu: 90 * EMU_PER_PIXEL,
      },
    });
  });

  it('converts two-cell anchors to a display rectangle', () => {
    expect(imageAnchorRect({
      type: 'TwoCell',
      data: {
        from: { row: 1, col: 1, rowOffsetEmu: 0, colOffsetEmu: 0 },
        to: {
          row: 4,
          col: 3,
          rowOffsetEmu: 5 * EMU_PER_PIXEL,
          colOffsetEmu: 20 * EMU_PER_PIXEL,
        },
      },
    }, geometry)).toEqual({ left: 100, top: 25, width: 220, height: 80 });
  });

  it('keeps the aspect ratio while resizing', () => {
    const resized = resizeImageRect(
      { left: 10, top: 20, width: 200, height: 100 },
      40,
      10,
    );
    expect(resized).toEqual({ left: 10, top: 20, width: 240, height: 120 });
  });
});
