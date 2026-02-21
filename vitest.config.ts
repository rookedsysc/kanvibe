/** React 19에서는 React.act가 development 빌드에만 포함되므로 테스트 환경을 강제 설정한다 */
process.env.NODE_ENV = "test";

import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import path from "path";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    globals: true,
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
});
