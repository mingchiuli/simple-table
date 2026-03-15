<script setup lang="ts">
import { RouterView } from "vue-router";
import { usePlatform } from "./composables/usePlatform";
import "./styles/desktop.css";
import "./styles/mobile.css";

const { platform } = usePlatform();
</script>

<template>
  <div :class="platform">
    <RouterView />
  </div>
</template>

<style>
* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

body {
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial,
    sans-serif;
  background-color: #f5f7fa;
}
</style>
