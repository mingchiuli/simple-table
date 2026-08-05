<script setup lang="ts">
import { Delete, PictureFilled } from '@element-plus/icons-vue';
import type { ImageAnchor, SheetImage } from '@/types/documentRuntime';
import {
  imageAnchorForRect,
  imageAnchorRect,
  resizeImageRect,
  type ImageRect,
} from '@/table-geometry/imageGeometry';

type PointerInteraction = {
  imageId: string;
  mode: 'move' | 'resize';
  pointerId: number;
  startX: number;
  startY: number;
  initial: ImageRect;
};

const props = defineProps<{
  images: SheetImage[];
  imageUrls: Readonly<Record<string, string>>;
  selectedImageId: string | null;
  canMoveResize: boolean;
  canDelete: boolean;
  getColumnOffset: (colIndex: number) => number;
  getRowOffset: (rowIndex: number) => number;
  getColumnIndexAt: (left: number) => number;
  getRowIndexAt: (top: number) => number;
  viewportLeft: number;
  viewportTop: number;
  viewportWidth: number;
  viewportHeight: number;
}>();

const emit = defineEmits<{
  (e: 'select', imageId: string | null): void;
  (e: 'update', imageId: string, anchor: ImageAnchor): void;
  (e: 'delete', imageId: string): void;
  (e: 'request-assets', imageIds: string[]): void;
}>();

const preview = shallowRef<{ imageId: string; rect: ImageRect } | null>(null);
let interaction: PointerInteraction | null = null;

function imageRect(image: SheetImage): ImageRect {
  if (preview.value?.imageId === image.id) return preview.value.rect;
  return imageAnchorRect(image.anchor, props);
}

const visibleImages = computed(() => props.images.filter((image) => {
  const rect = imageRect(image);
  const overscan = 240;
  return rect.left + rect.width >= props.viewportLeft - overscan
    && rect.left <= props.viewportLeft + props.viewportWidth + overscan
    && rect.top + rect.height >= props.viewportTop - overscan
    && rect.top <= props.viewportTop + props.viewportHeight + overscan;
}));

watch(
  () => visibleImages.value.map((image) => image.id),
  (imageIds) => emit('request-assets', imageIds),
  { immediate: true },
);

function imageStyle(image: SheetImage) {
  const rect = imageRect(image);
  return {
    left: `${rect.left}px`,
    top: `${rect.top}px`,
    width: `${rect.width}px`,
    height: `${rect.height}px`,
    zIndex: 10 + image.zIndex,
  };
}

function beginInteraction(event: PointerEvent, image: SheetImage, mode: 'move' | 'resize') {
  if (!props.canMoveResize || !image.renderable) return;
  if (event.pointerType === 'mouse' && event.button !== 0) return;
  event.preventDefault();
  event.stopPropagation();
  emit('select', image.id);
  interaction = {
    imageId: image.id,
    mode,
    pointerId: event.pointerId,
    startX: event.clientX,
    startY: event.clientY,
    initial: imageRect(image),
  };
  preview.value = { imageId: image.id, rect: { ...interaction.initial } };
  window.addEventListener('pointermove', handlePointerMove);
  window.addEventListener('pointerup', finishInteraction);
  window.addEventListener('pointercancel', cancelInteraction);
}

function handlePointerMove(event: PointerEvent) {
  if (!interaction || event.pointerId !== interaction.pointerId) return;
  event.preventDefault();
  const deltaX = event.clientX - interaction.startX;
  const deltaY = event.clientY - interaction.startY;
  const initial = interaction.initial;
  let rect: ImageRect;
  if (interaction.mode === 'move') {
    rect = {
      ...initial,
      left: Math.max(0, initial.left + deltaX),
      top: Math.max(0, initial.top + deltaY),
    };
  } else {
    rect = resizeImageRect(initial, deltaX, deltaY);
  }
  preview.value = { imageId: interaction.imageId, rect };
}

function finishInteraction(event: PointerEvent) {
  if (!interaction || event.pointerId !== interaction.pointerId) return;
  const completed = interaction;
  const rect = preview.value?.rect ?? completed.initial;
  cleanupInteraction();
  if (sameRect(rect, completed.initial)) return;
  emit('update', completed.imageId, imageAnchorForRect(rect, props));
}

function cancelInteraction() {
  cleanupInteraction();
}

function cleanupInteraction() {
  interaction = null;
  preview.value = null;
  window.removeEventListener('pointermove', handlePointerMove);
  window.removeEventListener('pointerup', finishInteraction);
  window.removeEventListener('pointercancel', cancelInteraction);
}

function sameRect(left: ImageRect, right: ImageRect): boolean {
  return Math.abs(left.left - right.left) < 0.5
    && Math.abs(left.top - right.top) < 0.5
    && Math.abs(left.width - right.width) < 0.5
    && Math.abs(left.height - right.height) < 0.5;
}

function selectImage(event: PointerEvent, imageId: string) {
  event.stopPropagation();
  emit('select', imageId);
}

function deleteSelected(event: Event, imageId: string) {
  event.preventDefault();
  event.stopPropagation();
  if (props.canDelete) emit('delete', imageId);
}

function handleKeydown(event: KeyboardEvent, imageId: string) {
  if ((event.key === 'Delete' || event.key === 'Backspace') && props.canDelete) {
    deleteSelected(event, imageId);
  }
}

onScopeDispose(cleanupInteraction);
</script>

<template>
  <div class="image-layer">
    <div
      v-for="image in visibleImages"
      :key="image.id"
      class="sheet-image"
      :class="{ 'is-selected': selectedImageId === image.id }"
      :style="imageStyle(image)"
      tabindex="0"
      @pointerdown="selectImage($event, image.id)"
      @dblclick.stop
      @keydown="handleKeydown($event, image.id)"
    >
      <img
        v-if="image.renderable && imageUrls[image.id]"
        :src="imageUrls[image.id]"
        alt=""
        draggable="false"
        @pointerdown="beginInteraction($event, image, 'move')"
      />
      <div v-else class="image-placeholder" @pointerdown="beginInteraction($event, image, 'move')">
        <el-icon><PictureFilled /></el-icon>
        <span>Image unavailable</span>
      </div>

      <template v-if="selectedImageId === image.id">
        <button
          v-if="canDelete"
          class="image-delete"
          type="button"
          title="Delete image"
          @pointerdown.stop
          @click="deleteSelected($event, image.id)"
        >
          <el-icon><Delete /></el-icon>
        </button>
        <button
          v-if="canMoveResize && image.renderable"
          class="image-resize"
          type="button"
          title="Resize image"
          aria-label="Resize image"
          @pointerdown="beginInteraction($event, image, 'resize')"
        />
      </template>
    </div>
  </div>
</template>

<style scoped>
.image-layer {
  position: absolute;
  inset: 0;
  z-index: 8;
  pointer-events: none;
}

.sheet-image {
  position: absolute;
  box-sizing: border-box;
  pointer-events: auto;
  outline: none;
  touch-action: none;
  user-select: none;
}

.sheet-image img,
.image-placeholder {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: contain;
}

.sheet-image.is-selected {
  box-shadow: 0 0 0 2px var(--el-color-primary);
}

.image-placeholder {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 4px;
  min-width: 72px;
  min-height: 48px;
  padding: 6px;
  overflow: hidden;
  color: var(--el-text-color-secondary);
  background: var(--el-fill-color-light);
  border: 1px dashed var(--el-border-color);
  font-size: 12px;
  text-align: center;
}

.image-delete,
.image-resize {
  position: absolute;
  z-index: 2;
  display: grid;
  place-items: center;
  width: 26px;
  height: 26px;
  padding: 0;
  color: var(--el-text-color-primary);
  background: var(--el-bg-color);
  border: 1px solid var(--el-border-color);
}

.image-delete {
  top: -13px;
  right: -13px;
  border-radius: 50%;
  cursor: pointer;
}

.image-resize {
  right: -7px;
  bottom: -7px;
  width: 14px;
  height: 14px;
  background: var(--el-color-primary);
  border: 2px solid var(--el-bg-color);
  border-radius: 2px;
  cursor: nwse-resize;
  touch-action: none;
}
</style>
