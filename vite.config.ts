import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "path";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "es2021",
    minify: true,
    rollupOptions: {
      input: {
        main: path.resolve(__dirname, "index.html"),
        island: path.resolve(__dirname, "island.html"),
      },
    },
  },
});
