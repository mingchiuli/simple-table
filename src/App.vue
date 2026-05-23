<script setup lang="ts">
import { RouterView, useRouter } from "vue-router";
import { usePlatform } from "./composables/usePlatform";
import { useDark } from "./composables/useDark";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import "./styles/base.css";
import "./styles/platform.css";
import { onMounted, onUnmounted } from "vue";

const { platform } = usePlatform();
useDark(); // Auto-sync with system dark mode preference
const router = useRouter();

let unlistenDeepLink: (() => void) | null = null;

onMounted(async () => {
  // 单实例模式：第二个实例传递文件路径给第一个实例
  unlistenDeepLink = await listen<string>("deep-link-received", (event) => {
    handleDeepLink(event.payload);
  });

  // macOS 文件关联：双击打开文件
  const pendingUrl = await invoke<string | null>("get_pending_deep_link");
  if (pendingUrl) {
    handleDeepLink(pendingUrl);
  }
});

onUnmounted(() => {
  if (unlistenDeepLink) {
    unlistenDeepLink();
  }
});

function handleDeepLink(url: string) {
  try {
    const parsed = new URL(url);

    // macOS file association: file:///path/to/file.xlsx → /path/to/file.xlsx
    if (parsed.protocol === "file:") {
      const filePath = decodeURIComponent(parsed.pathname);
      router.push({ name: "table", query: { file: filePath } });
    }
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
