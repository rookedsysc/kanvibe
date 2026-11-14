#!/usr/bin/env node
const path = require("node:path");
const { builtinModules } = require("node:module");

async function buildRemoteTerminalAgent() {
  const { build } = await import("vite");
  await build({
    configFile: false,
    logLevel: "warn",
    // The standalone helper uses ws's JavaScript implementation, with no ambient native addons.
    define: { "process.env.WS_NO_BUFFER_UTIL": "true", "process.env.WS_NO_UTF_8_VALIDATE": "true" },
    resolve: { conditions: ["node", "import"], mainFields: ["module", "main"] },
    build: {
      target: "node18", outDir: path.join(__dirname, "..", "build", "remote"), emptyOutDir: true,
      lib: { entry: path.join(__dirname, "..", "src", "lib", "remoteTerminalAgent.ts"), formats: ["cjs"], fileName: () => "terminal-agent.cjs" },
      rollupOptions: { external: ["bufferutil", "utf-8-validate", ...builtinModules, ...builtinModules.map((name) => `node:${name}`)] },
      minify: false,
    },
  });
  await build({
    configFile: false,
    logLevel: "warn",
    build: {
      target: "node18", outDir: path.join(__dirname, "..", "build", "remote"), emptyOutDir: false,
      lib: { entry: path.join(__dirname, "..", "src", "lib", "remoteHostServer.ts"), formats: ["cjs"], fileName: () => "host-server.cjs" },
      rollupOptions: { external: [...builtinModules, ...builtinModules.map((name) => `node:${name}`)] },
      minify: false,
    },
  });
}
if (require.main === module) buildRemoteTerminalAgent().catch((error) => { console.error(error); process.exitCode = 1; });
module.exports = { buildRemoteTerminalAgent };
