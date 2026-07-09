<script setup lang="ts">
import { usePlatform } from "./composables/usePlatform";
import { useDark } from "@/composables/useDark";
import { useDeepLinks } from "@/composables/useDeepLinks";
import "./styles/base.css";
import "./styles/platform.css";

const { platform } = usePlatform();
useDark();

const router = useRouter();
useDeepLinks(router);
</script>

<template>
  <div :class="['app-root', platform]">
    <RouterView />
  </div>
</template>
