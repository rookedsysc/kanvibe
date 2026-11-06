/** React 19에서는 React.act가 development 빌드에만 포함되므로 테스트 환경을 강제 설정한다 */
(process.env as { [key: string]: string | undefined }).NODE_ENV = "test";

import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react-swc";
import path from "path";

const nodeTestFiles = [
  "src/lib/aiUsage/**/*.test.ts",
  "docs-site/**/*.test.js",
];

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    globals: true,
    // QA 중에도 앱과 다른 작업이 쓸 CPU·메모리를 남긴다.
    maxWorkers: 2,
    maxConcurrency: 2,
    projects: [
      {
        extends: true,
        test: {
          name: "node",
          environment: "node",
          include: nodeTestFiles,
        },
      },
      {
        extends: true,
        test: {
          name: "dom",
          exclude: nodeTestFiles,
        },
      },
    ],
    exclude: [
      "dist/**",
      "qa-results/**",
      "src-tauri/**",
      ".tooling/**",
      ".opencode/**",
      ".claude/**",
      ".gemini/**",
      ".codex/**",
      ".omc/**",
      /** docs-site 같은 하위 워크스페이스에 설치된 의존성의 테스트 파일까지 걷히지 않도록 모든 깊이를 제외한다 */
      "**/node_modules/**",
      "tests/e2e/**",
    ],
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
});
