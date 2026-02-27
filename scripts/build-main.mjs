/**
 * Electron main process를 esbuild로 번들링한다.
 * src/entities, src/lib 등의 공유 코드를 함께 번들링하되,
 * native 모듈은 external로 처리한다.
 */
import { build } from "esbuild";
import path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const rootDir = path.resolve(__dirname, "..");

const nativeExternals = [
  "electron",
  "better-sqlite3",
  "node-pty",
  "ssh2",
  "electron-store",
  "electron-serve",
  "electron-updater",
];

async function buildMain() {
  /** background.ts를 번들링한다 */
  await build({
    entryPoints: [path.join(rootDir, "main/background.ts")],
    bundle: true,
    platform: "node",
    target: "node20",
    outfile: path.join(rootDir, "electron/background.js"),
    external: nativeExternals,
    sourcemap: true,
    tsconfig: path.join(rootDir, "tsconfig.json"),
    define: {
      "process.env.NODE_ENV": '"production"',
    },
  });

  /** preload.ts를 번들링한다 */
  await build({
    entryPoints: [path.join(rootDir, "main/preload.ts")],
    bundle: true,
    platform: "node",
    target: "node20",
    outfile: path.join(rootDir, "electron/preload.js"),
    external: ["electron"],
    sourcemap: true,
    tsconfig: path.join(rootDir, "tsconfig.json"),
  });

  console.log("[build-main] Electron main process build complete.");
}

buildMain().catch((error) => {
  console.error("[build-main] Build failed:", error);
  process.exit(1);
});
