import { defineConfig } from "vite";

// Temporule 前端构建配置：端口与 tauri.conf.json 的 devUrl 保持一致。
// 前端源码全部位于 src/（含入口 index.html），产物输出到 app/dist。
export default defineConfig({
  root: "src",
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    outDir: "../dist",
    target: "es2021",
    emptyOutDir: true,
  },
});
