import net from "node:net";
import path from "node:path";
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";
import { afterEach, describe, expect, it } from "vitest";

const requireFromRoot = createRequire(path.join(process.cwd(), "package.json"));
const scriptPath = path.join(process.cwd(), "scripts", "run-desktop-dev.cjs");
const { resolveDevServerPort } = requireFromRoot(scriptPath) as {
  resolveDevServerPort: (preferredPort?: number) => Promise<number>;
};

const openedServers: net.Server[] = [];

function occupyPort(port: number): Promise<void> {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    openedServers.push(server);
    server.on("error", reject);
    server.listen(port, "127.0.0.1", () => resolve());
  });
}

function allocateFreePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.listen(0, "127.0.0.1", () => {
      const address = server.address();
      const port = typeof address === "object" && address ? address.port : null;
      server.close(() => (port ? resolve(port) : reject(new Error("no port"))));
    });
  });
}

afterEach(async () => {
  await Promise.all(openedServers.splice(0).map((server) => new Promise<void>((resolve) => {
    server.close(() => resolve());
  })));
});

describe("run-desktop-dev 개발 서버 포트", () => {
  it("선호 포트가 비어 있으면 그 포트를 그대로 쓴다", async () => {
    const freePort = await allocateFreePort();

    expect(await resolveDevServerPort(freePort)).toBe(freePort);
  });

  it("선호 포트를 다른 프로세스가 쓰고 있으면 비어 있는 다른 포트를 고른다", async () => {
    const takenPort = await allocateFreePort();
    await occupyPort(takenPort);

    const resolvedPort = await resolveDevServerPort(takenPort);

    expect(resolvedPort).not.toBe(takenPort);
    /** 고른 포트가 실제로 비어 있어야 vite가 뜬다 */
    await expect(occupyPort(resolvedPort)).resolves.toBeUndefined();
  });

  it("Electron에 넘기는 주소를 확정한 포트로 만들고 vite가 포트를 갈아치우지 못하게 막는다", () => {
    const source = readFileSync(scriptPath, "utf8");

    expect(source).toContain("--strictPort");
    expect(source).toContain("const devServerUrl = `http://${DEV_SERVER_HOST}:${devServerPort}`");
    expect(source).toContain("KANVIBE_RENDERER_URL: devServerUrl");
    /** 주소를 상수로 굳혀 두면 vite가 옮겨 간 포트를 다시 놓친다 */
    expect(source).not.toContain('"http://127.0.0.1:5173"');
  });
});
