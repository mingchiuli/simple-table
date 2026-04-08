<script setup lang="ts">
import { ref, onMounted } from 'vue';

const props = withDefaults(defineProps<{
  modelValue: string;
  autoFocus?: boolean;
}>(), {
  autoFocus: true
});

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void;
  (e: 'blur'): void;
}>();

const inputRef = ref<InstanceType<typeof import('element-plus').ElInput> | null>(null);

onMounted(() => {
  if (!props.autoFocus) return;

  // 获取 ElInput 内部原生 input 元素，而非通过 querySelector 查找
  // 这样可以避免 ElInput 封装层干扰 preventScroll 选项
  const nativeInput = inputRef.value?.ref as HTMLInputElement | undefined;
  if (!nativeInput) return;

  // 记录当前滚动位置，focus 后恢复
  // 原因：虚拟列表滚动时可能触发 EditableCell 重建，focus 会导致浏览器自动滚动
  const scrollContainer = nativeInput.closest('.el-table-v2__body') ?? document.documentElement;
  const { scrollTop, scrollLeft } = scrollContainer;

  nativeInput.focus({ preventScroll: true });

  // requestAnimationFrame 确保在浏览器完成 focus 滚动后再恢复位置
  requestAnimationFrame(() => {
    scrollContainer.scrollTo({ top: scrollTop, left: scrollLeft, behavior: 'instant' });
  });
});
</script>

<template>
  <el-input
    ref="inputRef"
    :model-value="modelValue"
    class="cell-input"
    @input="emit('update:modelValue', $event)"
    @blur="emit('blur')"
  />
</template>

<style scoped>
.cell-input {
  width: 100%;
  font-size: 16px;
}

:deep(.cell-input .el-input__wrapper) {
  box-shadow: none;
}

:deep(.cell-input .el-input__wrapper:hover) {
  box-shadow: 0 0 0 1px #409eff inset;
}
</style>
