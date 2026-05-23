import { ref, onMounted, onUnmounted } from 'vue';

const isDark = ref(false);
let mediaQuery: MediaQueryList | null = null;
let listenerRefCount = 0;

const handleChange = (e: MediaQueryListEvent | MediaQueryList) => {
  isDark.value = e.matches;
  if (isDark.value) {
    document.documentElement.classList.add('dark');
  } else {
    document.documentElement.classList.remove('dark');
  }
};

// Initialize immediately to avoid flash of wrong theme
if (typeof window !== 'undefined') {
  mediaQuery = window.matchMedia('(prefers-color-scheme: dark)');
  isDark.value = mediaQuery.matches;
  handleChange(mediaQuery);
}

export function useDark() {
  onMounted(() => {
    if (mediaQuery && listenerRefCount === 0) {
      mediaQuery.addEventListener('change', handleChange);
    }
    listenerRefCount++;
  });

  onUnmounted(() => {
    listenerRefCount = Math.max(0, listenerRefCount - 1);
    if (listenerRefCount === 0 && mediaQuery) {
      mediaQuery.removeEventListener('change', handleChange);
    }
  });

  return {
    isDark,
  };
}