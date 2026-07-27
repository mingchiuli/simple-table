import { isMobile as isMobileOS } from '@/platform/runtime';

// 触摸设备检测
const isTouchDevice = ref(
  typeof window !== 'undefined' &&
    ('ontouchstart' in window || navigator.maxTouchPoints > 0)
);

// 视口尺寸
const width = ref(typeof window !== 'undefined' ? window.innerWidth : 1200);
const height = ref(typeof window !== 'undefined' ? window.innerHeight : 800);
const isMobileRuntime = ref(typeof window !== 'undefined' ? isMobileOS() : false);

// 判断逻辑：移动端 vs 桌面端
// Tauri 的 Android/iOS 平台优先，避免手机横屏被误判成桌面端。
// 浏览器预览或触屏桌面再用触摸能力 + 视口宽度兜底。
const isMobileDevice = computed(() => {
  return isMobileRuntime.value || (isTouchDevice.value && width.value < 900);
});

// 移动端内部判断：手机 vs 平板
// 手机：短边 < 768
// 平板：短边 >= 768
const isMobile = computed(() => isMobileDevice.value && Math.min(width.value, height.value) < 768);
const isTablet = computed(() => isMobileDevice.value && Math.min(width.value, height.value) >= 768);

// 桌面端
const isDesktop = computed(() => !isMobileDevice.value);

const platform = computed(() =>
  isMobile.value ? 'mobile' : isTablet.value ? 'tablet' : 'desktop'
);
const isMobileOrTablet = computed(() => isMobile.value || isTablet.value);

let resizeTimeout: ReturnType<typeof setTimeout> | null = null;
let listenerRefCount = 0;

const handleResize = () => {
  if (resizeTimeout) {
    clearTimeout(resizeTimeout);
  }
  resizeTimeout = setTimeout(() => {
    width.value = window.innerWidth;
    height.value = window.innerHeight;
  }, 100);
};

export function usePlatform() {
  onMounted(() => {
    width.value = window.innerWidth;
    height.value = window.innerHeight;
    isMobileRuntime.value = isMobileOS();
    if (listenerRefCount === 0 && typeof window !== 'undefined') {
      window.addEventListener('resize', handleResize);
    }
    listenerRefCount++;
  });

  onUnmounted(() => {
    listenerRefCount = Math.max(0, listenerRefCount - 1);
    if (listenerRefCount === 0) {
      if (typeof window !== 'undefined') {
        window.removeEventListener('resize', handleResize);
      }
      if (resizeTimeout) {
        clearTimeout(resizeTimeout);
        resizeTimeout = null;
      }
    }
  });

  return {
    width,
    isTouchDevice,
    isMobile,
    isTablet,
    isDesktop,
    isMobileRuntime,
    platform,
    isMobileOrTablet,
  };
}
