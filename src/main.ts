import { createApp } from "vue";
import { createPinia } from "pinia";
import "./assets/style.css";
import App from "./App.vue";



window.addEventListener('keydown', (e) => {
  // Ctrl+P (打印预览 - 刚才杀伤力最大的那个！)
  // Ctrl+F (搜索框 - 刚才跳出来的那个！) [cite: 2026-03-17]
  // Ctrl+S (网页另存为)
  // Ctrl+R / F5 (强制刷新，会导致 App 状态丢失)
  if (
    e.ctrlKey && ['p', 'f', 's', 'r'].includes(e.key.toLowerCase()) || 
    e.key === 'F5'
  ) {
    e.preventDefault();
    e.stopPropagation();
    console.log(`[AstralGuard] Blocked browser hotkey: ${e.key}`);
  }
}, { capture: true }); // 使用捕获模式，确保在其他逻辑前拦截
const app = createApp(App);
app.use(createPinia());
app.mount("#app");
// disabled所有会导致穿帮的 WebView2 默认快捷键
