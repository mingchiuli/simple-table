<script setup lang="ts">
import { usePlatform } from "./composables/usePlatform";
import { getCurrent, onOpenUrl } from "@tauri-apps/plugin-deep-link";
import { listen } from "@tauri-apps/api/event";
import "./styles/base.css";
import "./styles/platform.css";

const { platform } = usePlatform();

// 暗黑模式：跟随系统偏好
const dark = window.matchMedia('(prefers-color-scheme: dark)');
document.documentElement.classList.toggle('dark', dark.matches);
dark.addEventListener('change', e => document.documentElement.classList.toggle('dark', e.matches));

const router = useRouter();

let unlistenDeepLink: (() => void) | null = null;
let unlistenOpenUrl: (() => void) | null = null;

onMounted(async () => {
  // 单实例模式：第二个实例传递文件路径给第一个实例
  unlistenDeepLink = await listen<string>("deep-link-received", (event) => {
    handleDeepLink(event.payload);
  });

  // macOS 文件关联：监听运行时触发的事件
  unlistenOpenUrl = await onOpenUrl((urls) => {
    if (urls.length > 0) {
      handleDeepLink(urls[0]);
    }
  });

  // 启动时检查：Windows/Linux 通过命令行参数启动
  const startUrls = await getCurrent();
  if (startUrls && startUrls.length > 0) {
    handleDeepLink(startUrls[0]);
  }
});

onUnmounted(() => {
  if (unlistenDeepLink) {
    unlistenDeepLink();
  }
  if (unlistenOpenUrl) {
    unlistenOpenUrl();
  }
});

function handleDeepLink(url: string) {
  try {
    // macOS: file:///path/to/file.xlsx
    // Windows/Linux: C:\path\to\file.xlsx 或 /path/to/file.xlsx
    let filePath: string;

    if (url.startsWith("file:")) {
      const parsed = new URL(url);
      filePath = decodeURIComponent(parsed.pathname);
    } else {
      // Windows/Linux 直接传递文件路径
      filePath = url;
    }

    router.push({ name: "table", query: { file: filePath } });
  } catch (e) {
    console.error("Failed to parse deep link:", e);
  }
}
</script>

<template>
  <div :class="['app-root', platform]">
    <RouterView />
  </div>
</template>

<style>

</style>
