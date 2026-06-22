// 触摸设备检测
const isTouchDevice = ref(
  typeof window !== 'undefined' &&
    'ontouchstart' in window &&
    navigator.maxTouchPoints > 0
);

// 屏幕尺寸
const screenWidth = ref(typeof window !== 'undefined' ? window.screen.width : 1200);
const screenHeight = ref(typeof window !== 'undefined' ? window.screen.height : 800);

// 视口尺寸
const width = ref(typeof window !== 'undefined' ? window.innerWidth : 1200);

// 判断逻辑：移动端 vs 桌面端
// 移动端特征：有触摸 + 宽高比例接近正方形或竖屏（height >= width）
// 桌面端特征：无触摸 或 宽 > 高（横屏）
const isMobileDevice = computed(() => {
  // 有触摸能力且屏幕是竖屏或正方形
  return isTouchDevice.value && screenHeight.value >= screenWidth.value;
});

// 移动端内部判断：手机 vs 平板
// 手机：宽度 < 768
// 平板：宽度 >= 768
const isMobile = computed(() => isMobileDevice.value && screenWidth.value < 768);
const isTablet = computed(() => isMobileDevice.value && screenWidth.value >= 768);

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
    screenWidth.value = window.screen.width;
    screenHeight.value = window.screen.height;
  }, 100);
};

export function usePlatform() {
  onMounted(() => {
    width.value = window.innerWidth;
    screenWidth.value = window.screen.width;
    screenHeight.value = window.screen.height;
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
    platform,
    isMobileOrTablet,
  };
}
