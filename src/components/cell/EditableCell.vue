<script setup lang="ts">
const modelValue = defineModel<string>({ required: true });

const props = withDefaults(defineProps<{
  autoFocus?: boolean;
  minHeight?: number;
}>(), {
  autoFocus: true,
  minHeight: 72,
});

const emit = defineEmits<{
  (e: 'commit'): void;
  (e: 'cancel'): void;
}>();

const textareaRef = ref<HTMLTextAreaElement | null>(null);
let isCancelling = false;

function focusWithoutScroll() {
  if (!props.autoFocus || !textareaRef.value) return;

  const textarea = textareaRef.value;
  const scrollContainer = textarea.closest('.el-table-v2__body') ?? document.documentElement;
  const { scrollTop, scrollLeft } = scrollContainer;

  textarea.focus({ preventScroll: true });
  textarea.select();

  requestAnimationFrame(() => {
    scrollContainer.scrollTo({ top: scrollTop, left: scrollLeft, behavior: 'instant' });
  });
}

function handleKeydown(event: KeyboardEvent) {
  if (event.key === 'Enter' && !event.altKey && !event.shiftKey && !event.metaKey && !event.ctrlKey) {
    event.preventDefault();
    emit('commit');
    return;
  }

  if (event.key === 'Escape') {
    event.preventDefault();
    isCancelling = true;
    emit('cancel');
  }
}

function handleBlur() {
  if (isCancelling) {
    isCancelling = false;
    return;
  }
  emit('commit');
}

onMounted(focusWithoutScroll);
</script>

<template>
  <textarea
    ref="textareaRef"
    v-model="modelValue"
    class="cell-textarea"
    spellcheck="false"
    :style="{ minHeight: `${Math.max(36, minHeight)}px` }"
    @keydown="handleKeydown"
    @blur="handleBlur"
  />
</template>

<style scoped>
.cell-textarea {
  display: block;
  width: 100%;
  min-width: 0;
  resize: none;
  border: 1px solid var(--el-color-primary);
  box-shadow: inset 0 0 0 1px var(--el-color-primary);
  background: var(--el-bg-color);
  color: var(--el-text-color-primary);
  font: inherit;
  line-height: 1.35;
  padding: 6px 8px;
  outline: none;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

@media (pointer: coarse) {
  .cell-textarea {
    font-size: 16px;
    padding: 8px;
  }
}
</style>
