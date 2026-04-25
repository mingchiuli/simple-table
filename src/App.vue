<script setup lang="ts">
import { RouterView, useRouter } from "vue-router";
import { usePlatform } from "./composables/usePlatform";
import { getCurrent } from "@tauri-apps/plugin-deep-link";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import "./styles/base.css";
import "./styles/platform.css";
import { onMounted, onUnmounted } from "vue";

const { platform } = usePlatform();
const router = useRouter();

let unlistenDeepLink: (() => void) | null = null;

onMounted(async () => {
  // 始终注册后续 deep link 事件监听（single_instance / 运行期触发的协议链接）
  unlistenDeepLink = await listen<string>("deep-link-received", (event) => {
    handleDeepLink(event.payload);
  });

  // 启动时通过 deep link 打开
  const startUrls = await getCurrent();
  if (startUrls && startUrls.length > 0) {
    handleDeepLink(startUrls[0]);
    return;
  }

  // macOS 文件关联：Rust 端缓存的待处理链接
  const pendingUrl = await invoke<string | null>("get_pending_deep_link");
  if (pendingUrl) {
    handleDeepLink(pendingUrl);
  }
});

onUnmounted(() => {
  if (unlistenDeepLink) {
    unlistenDeepLink();
    unlistenDeepLink = null;
  }
});

function handleDeepLink(url: string) {
  try {
    const parsed = new URL(url);

    if (parsed.protocol === "simpletable:") {
      const filePath = parsed.searchParams.get("file");
      if (filePath) {
        router.push({ name: "table", query: { file: filePath } });
      }
    } else if (parsed.protocol === "file:") {
      // macOS file association: file:///path/to/file.xlsx → /path/to/file.xlsx
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
