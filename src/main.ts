import "element-plus/dist/index.css";
import "element-plus/theme-chalk/dark/css-vars.css";
import "@/styles/base.css";
import "@/styles/platform.css";
import { createPinia } from "pinia";
import { createApp } from "vue";
import App from "@/App.vue";
import router from "@/router";

const app = createApp(App);

app.use(createPinia());
app.use(router);
app.mount("#app");
