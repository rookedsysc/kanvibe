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
      { find: "@/app/actions/auth", replacement: resolvePath("./src/desktop/renderer/actions/auth.ts") },
      { find: "@/app/actions/kanban", replacement: resolvePath("./src/desktop/renderer/actions/kanban.ts") },
      { find: "@/app/actions/project", replacement: resolvePath("./src/desktop/renderer/actions/project.ts") },
      { find: "@/app/actions/appSettings", replacement: resolvePath("./src/desktop/renderer/actions/appSettings.ts") },
      { find: "@/app/actions/diff", replacement: resolvePath("./src/desktop/renderer/actions/diff.ts") },
      { find: "@/app/actions/paneLayout", replacement: resolvePath("./src/desktop/renderer/actions/paneLayout.ts") },
      { find: "@/i18n/navigation", replacement: resolvePath("./src/desktop/renderer/navigation.tsx") },
      { find: "@/hooks/useAutoRefresh", replacement: resolvePath("./src/desktop/renderer/hooks/useAutoRefresh.ts") },
      { find: "@/hooks/useProjectFilterParams", replacement: resolvePath("./src/desktop/renderer/hooks/useProjectFilterParams.ts") },
      { find: "@", replacement: resolvePath("./src") },
    ],
  },
});
