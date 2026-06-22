import "element-plus/dist/index.css";
import "element-plus/theme-chalk/dark/css-vars.css";
import "@/styles/base.css";
import "@/styles/platform.css";
import * as ElementPlusIconsVue from "@element-plus/icons-vue";
import App from "@/App.vue";
import router from "@/router";

const app = createApp(App);

for (const [key, component] of Object.entries(ElementPlusIconsVue)) {
  app.component(key, component);
}

app.use(createPinia());
app.use(router);
app.mount("#app");
