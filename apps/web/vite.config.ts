import { defineConfig } from "vite";
import { resolve } from "node:path";

export default defineConfig({
  server: { port: 5173 },
  build: {
    rollupOptions: {
      input: {
        // 対局の画面。
        index: resolve(__dirname, "index.html"),
        // 卓の見た目だけを確かめる画面。**対局を挟まずに副露5種を見られる。**
        preview: resolve(__dirname, "preview.html"),
      },
    },
  },
});
