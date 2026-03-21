<script setup lang="ts">
import { RouterView } from "vue-router";
import { usePlatform } from "./composables/usePlatform";
import "./styles/base.css";
import "./styles/platform.css";

const { platform } = usePlatform();
</script>

<template>
  <div :class="['app-root', platform]">
    <RouterView />
  </div>
</template>

<style>
.app-root {
  padding-top: 0;
  padding-left: env(safe-area-inset-left);
  padding-right: env(safe-area-inset-right);
}
</style>
