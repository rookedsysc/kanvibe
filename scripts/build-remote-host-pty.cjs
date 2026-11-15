// 원격 전용 native runtime. desktop의 기존 node-pty 의존성 버전은 유지한다.
const { createHash } = require("node:crypto");
const fs = require("node:fs/promises");
const path = require("node:path");
const https = require("node:https");
const { execFileSync } = require("node:child_process");

const VERSION = "1.2.0-beta.14";
const INTEGRITY = "XORU9BQgpxVgqr7WivjJ17mLenOHUgKKWzuZZNaw3NDYgHc/wPJQMSaoLDrpEgqV6aU1nNwil1o/OqYj6lWmUA==";
const URL = `https://registry.npmjs.org/node-pty/-/node-pty-${VERSION}.tgz`;

async function buildRemoteHostPty(outDirectory) {
  const cache = path.join(__dirname, "..", "node_modules", ".cache", "kanvibe-host-pty");
  await fs.mkdir(cache, { recursive: true });
  const archivePath = path.join(cache, `node-pty-${VERSION}.tgz`);
  let archive;
  try { archive = await fs.readFile(archivePath); }
  catch (error) {
    if (error.code !== "ENOENT") throw error;
    archive = await new Promise((resolve, reject) => {
      const request = https.get(URL, (response) => {
        if (response.statusCode !== 200) { response.resume(); reject(new Error(`PTY artifact HTTP ${response.statusCode}`)); return; }
        const chunks = []; let size = 0;
        response.on("data", (chunk) => { size += chunk.length; if (size > 32 * 1024 * 1024) request.destroy(new Error("PTY archive too large")); else chunks.push(chunk); });
        response.on("end", () => resolve(Buffer.concat(chunks))); response.on("error", reject);
      });
      request.setTimeout(30_000, () => request.destroy(new Error("PTY artifact download timed out"))); request.on("error", reject);
    });
  }
  if (createHash("sha512").update(archive).digest("base64") !== INTEGRITY) throw new Error("Remote node-pty npm integrity mismatch");
  await fs.writeFile(archivePath, archive);
  const extracted = await fs.mkdtemp(path.join(cache, "extract-"));
  try {
    execFileSync("tar", ["-xzf", archivePath, "-C", extracted, "package/lib", "package/prebuilds", "package/LICENSE", "package/package.json"]);
    const packageRoot = path.join(extracted, "package");
    const file = async (name, executable = false) => {
      const bytes = await fs.readFile(path.join(packageRoot, name));
      return { path: name, sha256: createHash("sha256").update(bytes).digest("hex"), data: bytes.toString("base64"), mode: executable ? 0o700 : 0o600 };
    };
    const common = [];
    for (const name of ["package.json", "LICENSE", ...["index", "utils", "terminal", "eventEmitter2", "unixTerminal"].map((name) => `lib/${name}.js`)]) common.push(await file(name));
    const platforms = {};
    for (const platform of ["darwin-arm64", "darwin-x64", "linux-arm64", "linux-x64"]) {
      platforms[platform] = [await file(`prebuilds/${platform}/pty.node`)];
      if (platform.startsWith("darwin")) platforms[platform].push(await file(`prebuilds/${platform}/spawn-helper`, true));
    }
    await fs.mkdir(outDirectory, { recursive: true });
    await fs.writeFile(path.join(outDirectory, "host-pty-artifacts.json"), JSON.stringify({ version: VERSION, integrity: `sha512-${INTEGRITY}`, common, platforms }));
  } finally { await fs.rm(extracted, { recursive: true, force: true }); }
}
module.exports = { buildRemoteHostPty };
