import { ref, computed, onMounted, onUnmounted } from 'vue';

const width = ref(typeof window !== 'undefined' ? window.innerWidth : 1200);
const isTouchDevice = ref(
  typeof window !== 'undefined' &&
    'ontouchstart' in window &&
    navigator.maxTouchPoints > 0
);

const isMobile = computed(() => width.value < 768);
const isTablet = computed(() => width.value >= 768 && width.value < 1024);
const isDesktop = computed(() => width.value >= 1024);
const platform = computed(() =>
  isMobile.value ? 'mobile' : isTablet.value ? 'tablet' : 'desktop'
);
const isMobileOrTablet = computed(() => isMobile.value || isTablet.value);

let resizeTimeout: ReturnType<typeof setTimeout> | null = null;

const handleResize = () => {
  if (resizeTimeout) {
    clearTimeout(resizeTimeout);
  }
  resizeTimeout = setTimeout(() => {
    width.value = window.innerWidth;
  }, 100);
};

export function usePlatform() {
  onMounted(() => {
    width.value = window.innerWidth;
    window.addEventListener('resize', handleResize);
  });

  onUnmounted(() => {
    window.removeEventListener('resize', handleResize);
    if (resizeTimeout) {
      clearTimeout(resizeTimeout);
    }
  });

  return {
    width,
    isTouchDevice,
    isMobile,
    isTablet,
    isDesktop,
    platform,
    isMobileOrTablet,
  };
}
