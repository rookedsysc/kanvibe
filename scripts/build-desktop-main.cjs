#!/usr/bin/env node

const { execFileSync } = require("node:child_process");
const { mkdtempSync, readdirSync, rmSync, statSync, writeFileSync } = require("node:fs");
const os = require("node:os");
const path = require("node:path");

function collectTypeScriptFiles(relativeRoot, files = []) {
  const absoluteRoot = path.join(process.cwd(), relativeRoot);

  for (const entry of readdirSync(absoluteRoot)) {
    const relativePath = path.join(relativeRoot, entry);
    const absolutePath = path.join(process.cwd(), relativePath);
    const stats = statSync(absolutePath);

    if (stats.isDirectory()) {
      if (entry === "__tests__") {
        continue;
      }

      collectTypeScriptFiles(relativePath, files);
      continue;
    }

    if (!entry.endsWith(".ts") || entry.endsWith(".test.ts")) {
      continue;
    }

    files.push(absolutePath);
  }

  return files;
}

function main() {
  const tempDir = mkdtempSync(path.join(os.tmpdir(), "kanvibe-desktop-build-"));
  const tempConfigPath = path.join(tempDir, "tsconfig.json");
  const files = [
    ...collectTypeScriptFiles("src/desktop/main"),
    ...collectTypeScriptFiles("src/entities"),
    ...collectTypeScriptFiles("src/lib"),
  ];

  const tempConfig = {
    extends: path.join(process.cwd(), "tsconfig.json"),
    compilerOptions: {
      noEmit: false,
      incremental: false,
      module: "commonjs",
      moduleResolution: "node",
      outDir: path.join(process.cwd(), "build", "main"),
      rootDir: process.cwd(),
      baseUrl: process.cwd(),
    },
    include: [],
    exclude: [],
    files,
  };

  try {
    writeFileSync(tempConfigPath, `${JSON.stringify(tempConfig, null, 2)}\n`);
    execFileSync("pnpm", ["exec", "tsc", "-p", tempConfigPath], {
      stdio: "inherit",
      env: process.env,
    });
  } finally {
    rmSync(tempDir, { recursive: true, force: true });
  }
}

main();
