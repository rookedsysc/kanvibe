import path from "node:path";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const resolvePath = (relativePath: string) => path.resolve(__dirname, relativePath);

export default defineConfig({
  base: "./",
  plugins: [react()],
  server: {
    host: "127.0.0.1",
    port: 5173,
  },
  build: {
    outDir: "build/renderer",
    emptyOutDir: true,
  },
  resolve: {
    alias: [
      { find: "@", replacement: resolvePath("./src") },
    ],
  },
});
