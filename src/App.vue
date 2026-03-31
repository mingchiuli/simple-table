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
  // Check if app was started via deep link (getCurrent works for initial startup)
  const startUrls = await getCurrent();
  if (startUrls && startUrls.length > 0) {
    handleDeepLink(startUrls[0]);
    return;
  }

  // Check for pending deep link from macOS file association (stored in Rust state)
  const pendingUrl = await invoke<string | null>("get_pending_deep_link");
  if (pendingUrl) {
    handleDeepLink(pendingUrl);
    return;
  }

  // Listen for deep links from single_instance (desktop - Windows/Linux)
  const unlisten = await listen<string>("deep-link-received", (event) => {
    handleDeepLink(event.payload);
  });
  unlistenDeepLink = unlisten;
});

onUnmounted(() => {
  if (unlistenDeepLink) {
    unlistenDeepLink();
    unlistenDeepLink = null;
  }
});

function handleDeepLink(url: string) {
  console.log("Deep link received:", url);
  console.log("Platform:", platform);
  try {
    const parsed = new URL(url);
    console.log("Protocol:", parsed.protocol);
    console.log("Host:", parsed.host);
    console.log("Search params:", Object.fromEntries(parsed.searchParams));

    if (parsed.protocol === "simpletable:") {
      const filePath = parsed.searchParams.get("file");
      console.log("File path:", filePath);
      if (filePath) {
        router.push({ name: "table", query: { file: filePath } });
      }
    } else if (parsed.protocol === "file:") {
      // macOS file association: file:///path/to/file.xlsx → /path/to/file.xlsx
      const filePath = decodeURIComponent(parsed.pathname);
      console.log("File path from association:", filePath);
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
.app-root {
  padding-top: env(safe-area-inset-top);
  padding-bottom: env(safe-area-inset-bottom);
  padding-left: env(safe-area-inset-left);
  padding-right: env(safe-area-inset-right);
}
</style>
