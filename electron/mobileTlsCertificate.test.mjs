import { mkdtempSync, rmSync, statSync } from "node:fs";
import https from "node:https";
import { tmpdir } from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

const require = createRequire(import.meta.url);
const { loadOrCreateMobileTlsCertificate } = require("./mobileTlsCertificate.js");

describe("loadOrCreateMobileTlsCertificate", () => {
  let parentDirectory;
  let directory;

  beforeEach(() => {
    parentDirectory = mkdtempSync(path.join(tmpdir(), "kanvibe-tls-"));
    directory = path.join(parentDirectory, "mobile-tls");
  });

  afterEach(() => {
    rmSync(parentDirectory, { recursive: true, force: true });
  });

  it("개인키는 소유자만 읽을 수 있게 만든다", async () => {
    await loadOrCreateMobileTlsCertificate(directory);
    expect(statSync(path.join(directory, "private-key.pem")).mode & 0o777).toBe(0o600);
    expect(statSync(directory).mode & 0o777).toBe(0o700);
  });

  it("다시 불러도 같은 인증서를 돌려줘 기기의 지문 고정이 깨지지 않는다", async () => {
    const first = await loadOrCreateMobileTlsCertificate(directory);
    const second = await loadOrCreateMobileTlsCertificate(directory);
    expect(second).toEqual(first);
  });

  it("만든 인증서로 TLS 서버를 띄워 요청을 받을 수 있다", async () => {
    const certificate = await loadOrCreateMobileTlsCertificate(directory);
    const server = https.createServer(certificate, (_request, response) => response.end("ok"));
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));

    const body = await new Promise((resolve, reject) => {
      https
        .get({ host: "127.0.0.1", port: server.address().port, rejectUnauthorized: false }, (response) => {
          response.setEncoding("utf8");
          response.on("data", resolve);
        })
        .on("error", reject);
    });
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));

    expect(body).toBe("ok");
  });
});
