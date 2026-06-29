<script setup lang="ts">
import { ColumnHeaderCell, RowNumberCell } from "@/components/cell";

type ColumnItem = {
  index: number;
  title: string;
  left: number;
  width: number;
};

type RowItem = {
  index: number;
  top: number;
  height: number;
};

defineProps<{
  columns: ColumnItem[];
  rows: RowItem[];
  scrollLeft: number;
  scrollTop: number;
  totalColumnsWidth: number;
  totalRowsHeight: number;
}>();

const emit = defineEmits<{
  (e: "delete-row", index: number): void;
  (e: "delete-column", index: number): void;
}>();
</script>

<template>
  <div class="corner-cell">#</div>

  <div class="column-header-viewport">
    <div class="column-header-strip" :style="{ width: `${totalColumnsWidth}px` }">
      <div
        v-for="column in columns"
        :key="column.index"
        class="column-header-cell"
        :style="{
          left: `${column.left - scrollLeft}px`,
          width: `${column.width}px`,
        }"
      >
        <ColumnHeaderCell
          :column-index="column.index"
          :title="column.title"
          @delete="(index) => emit('delete-column', index)"
        />
      </div>
    </div>
  </div>

  <div class="row-header-viewport">
    <div class="row-header-strip" :style="{ height: `${totalRowsHeight}px` }">
      <div
        v-for="row in rows"
        :key="row.index"
        class="row-header-cell"
        :style="{
          top: `${row.top - scrollTop}px`,
          height: `${row.height}px`,
        }"
      >
        <RowNumberCell
          :row-index="row.index"
          @delete="(index) => emit('delete-row', index)"
        />
      </div>
    </div>
  </div>
</template>
