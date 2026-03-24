<script setup lang="ts">
import { RouterView, useRouter } from "vue-router";
import { usePlatform } from "./composables/usePlatform";
import { getCurrent, onOpenUrl } from "@tauri-apps/plugin-deep-link";
import { listen } from "@tauri-apps/api/event";
import "./styles/base.css";
import "./styles/platform.css";
import { onMounted } from "vue";

const { platform } = usePlatform();
const router = useRouter();

onMounted(async () => {
  // Check if app was started via deep link
  const startUrls = await getCurrent();
  if (startUrls && startUrls.length > 0) {
    handleDeepLink(startUrls[0]);
  }

  // Listen for deep links from onOpenUrl (mobile)
  await onOpenUrl((urls) => {
    if (urls.length > 0) {
      handleDeepLink(urls[0]);
    }
  });

  // Listen for deep links from single_instance (desktop)
  await listen<string>("deep-link-received", (event) => {
    handleDeepLink(event.payload);
  });
});

function handleDeepLink(url: string) {
  console.log("Deep link received:", url);
  console.log("Platform:", platform);
  // Parse simpletable://open?file=/path/to/file
  try {
    const parsed = new URL(url);
    console.log("Protocol:", parsed.protocol);
    console.log("Host:", parsed.host);
    console.log("Search params:", Object.fromEntries(parsed.searchParams));
    if (parsed.protocol === "simpletable:") {
      const filePath = parsed.searchParams.get("file");
      const content = parsed.searchParams.get("content");
      console.log("File path:", filePath);
      console.log("Content:", content);
      if (filePath) {
        router.push({ name: "table", query: { file: filePath } });
      } else if (content) {
        // Mobile might pass content directly (base64 or raw)
        router.push({ name: "table", query: { content } });
      }
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
  padding-top: 0;
  padding-bottom: env(safe-area-inset-bottom);
  padding-left: env(safe-area-inset-left);
  padding-right: env(safe-area-inset-right);
}
</style>
