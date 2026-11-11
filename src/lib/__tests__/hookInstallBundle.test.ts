// @vitest-environment node
import { execFile, execFileSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import http from "node:http";
import path from "node:path";
import { describe, it, expect, beforeEach } from "vitest";
import {
  buildHookInstallBootstrapCommand,
  buildHookInstallScript,
  clearHookInstallScripts,
  HOOK_INSTALL_SCRIPT_PATH,
  HOOK_INSTALL_TOKEN_HEADER,
  HOOK_INSTALL_SUCCESS_MARKER,
  issueHookInstallScript,
  readHookInstallScript,
  revokeHookInstallScript,
} from "@/lib/hookInstallBundle";

const hookFiles = [
  {
    filePath: "/home/user/project/.claude/hooks/kanvibe-stop-hook.sh",
    content: "#!/bin/bash\necho '작업 완료'\n",
    mode: 0o755,
  },
  {
    filePath: "/home/user/project/.claude/settings.json",
    content: '{\n  "hooks": {}\n}\n',
  },
];

beforeEach(() => {
  clearHookInstallScripts();
});

describe("buildHookInstallScript", () => {
  it("should write every provider file and mark completion", () => {
    // When
    const script = buildHookInstallScript(hookFiles);

    // Then
    expect(script).toContain("set -e");
    expect(script).toContain("mkdir -p '/home/user/project/.claude/hooks'");
    expect(script).toContain("> '/home/user/project/.claude/hooks/kanvibe-stop-hook.sh'");
    expect(script).toContain("chmod 755 '/home/user/project/.claude/hooks/kanvibe-stop-hook.sh'");
    expect(script).toContain("> '/home/user/project/.claude/settings.json'");
    expect(script.trimEnd().endsWith(`printf '%s\\n' '${HOOK_INSTALL_SUCCESS_MARKER}'`)).toBe(true);
  });

  it("should carry file content as base64 so quotes and newlines survive", () => {
    // When
    const script = buildHookInstallScript(hookFiles);

    // Then
    const encodedStopHook = Buffer.from(hookFiles[0].content, "utf-8").toString("base64");
    expect(script).toContain(`printf '%s' '${encodedStopHook}'`);
    expect(script).not.toContain("echo '작업 완료'");
  });

  it("should not set a mode for files that do not need one", () => {
    // When
    const script = buildHookInstallScript([hookFiles[1]]);

    // Then
    expect(script).not.toContain("chmod");
  });

  it("should produce the same script for the same files", () => {
    // When
    const first = buildHookInstallScript(hookFiles);
    const second = buildHookInstallScript(hookFiles);

    // Then
    expect(first).toBe(second);
  });
});

const CLIENT_ID = "1b9d6bcd-bbfd-4b2d-9b5d-ab8dfbbd4bed";
const sampleTicket = { token: "token-value", scriptSha256: "a".repeat(64) };

describe("issueHookInstallScript / readHookInstallScript", () => {
  it("should return the registered script for its token", () => {
    // Given
    const ticket = issueHookInstallScript(hookFiles);

    // When
    const script = readHookInstallScript(ticket.token);

    // Then
    expect(script).toBe(buildHookInstallScript(hookFiles));
  });

  it("should stay readable so a retried install can reuse the same token", () => {
    // Given
    const ticket = issueHookInstallScript(hookFiles);

    // When
    readHookInstallScript(ticket.token);

    // Then
    expect(readHookInstallScript(ticket.token)).not.toBeNull();
  });

  it("should stop serving the script once the install revokes its token", () => {
    // Given
    const ticket = issueHookInstallScript(hookFiles);

    // When
    revokeHookInstallScript(ticket.token);

    // Then
    expect(readHookInstallScript(ticket.token)).toBeNull();
  });

  it("should reject an unknown token", () => {
    // Given
    issueHookInstallScript(hookFiles);

    // When
    const script = readHookInstallScript("0".repeat(48));

    // Then
    expect(script).toBeNull();
  });

  it("should reject a missing token", () => {
    // Given
    issueHookInstallScript(hookFiles);

    // When & Then
    expect(readHookInstallScript(null)).toBeNull();
    expect(readHookInstallScript("")).toBeNull();
  });

  it("should issue an unguessable token per install", () => {
    // When
    const firstTicket = issueHookInstallScript(hookFiles);
    const secondTicket = issueHookInstallScript(hookFiles);

    // Then
    expect(firstTicket.token).not.toBe(secondTicket.token);
    expect(firstTicket.token).toMatch(/^[0-9a-f]{48}$/);
  });

  it("should issue a checksum that changes with the install payload", () => {
    // When
    const ticket = issueHookInstallScript(hookFiles);
    const otherTicket = issueHookInstallScript([hookFiles[1]]);

    // Then
    expect(ticket.scriptSha256).toMatch(/^[0-9a-f]{64}$/);
    expect(ticket.scriptSha256).not.toBe(otherTicket.scriptSha256);
  });

  it("should issue the checksum the remote computes for the script it restores", () => {
    // Given
    const ticket = issueHookInstallScript(hookFiles);
    const script = readHookInstallScript(ticket.token) ?? "";

    /** `$(...)`가 끝 개행을 떼고 원격이 `printf '%s\n'`으로 되살리는 과정을 그대로 재현한다 */
    // When
    const remoteChecksum = execFileSync(
      "sh",
      [
        "-c",
        `__downloaded=$(printf '%s' "$1");`
        + ` printf '%s\\n' "$__downloaded" | { sha256sum 2>/dev/null || shasum -a 256; } | cut -d' ' -f1`,
        "sh",
        script,
      ],
      { encoding: "utf-8" },
    ).trim();

    // Then
    expect(remoteChecksum).toBe(ticket.scriptSha256);
  });
});

describe("buildHookInstallBootstrapCommand", () => {
  it("원격 설치 소켓이 없으면 응답 가능한 TCP 주소에도 설치 토큰을 보내지 않는다", async () => {
    const workspace = mkdtempSync("/tmp/kvi-");
    const ticket = issueHookInstallScript([{
      filePath: path.join(workspace, "installed.sh"),
      content: "echo installed\n",
    }]);
    const received: string[] = [];
    const server = http.createServer((request, response) => {
      received.push(request.url ?? "");
      response.end(readHookInstallScript(ticket.token));
    });
    await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
    const address = server.address() as { port: number };

    try {
      const command = buildHookInstallBootstrapCommand(
        `http://127.0.0.1:${address.port}`,
        ticket,
        CLIENT_ID,
      ).replaceAll("/tmp/kv-$(id -u)", workspace);
      const exitCode = await new Promise<number>((resolve) => {
        execFile("sh", ["-c", command], { timeout: 5000 }, (error) => resolve(Number(error?.code ?? 0)));
      });

      expect(received).toEqual([]);
      expect(exitCode).not.toBe(0);
    } finally {
      await new Promise<void>((resolve) => server.close(() => resolve()));
      rmSync(workspace, { recursive: true, force: true });
    }
  });

  it("should keep the SSH command short instead of carrying the payload", () => {
    // Given
    const ticket = issueHookInstallScript(hookFiles);

    // When
    const command = buildHookInstallBootstrapCommand("http://192.168.0.10:9736", ticket, CLIENT_ID);

    // Then
    expect(Buffer.byteLength(command)).toBeLessThan(1500);
    expect(command).not.toContain("base64 -d");
    expect(command).toContain(`http://192.168.0.10:9736${HOOK_INSTALL_SCRIPT_PATH}'`);
  });

  it("설치 토큰은 URL이 아니라 헤더로 보내 접근 로그에 남지 않게 한다", () => {
    // Given
    const ticket = issueHookInstallScript(hookFiles);

    // When
    const command = buildHookInstallBootstrapCommand("http://192.168.0.10:9736", ticket, CLIENT_ID);

    // Then
    expect(command).not.toContain("?token=");
    expect(command).toContain(`-H '${HOOK_INSTALL_TOKEN_HEADER}: ${ticket.token}'`);
  });

  it("should keep the SSH command the same size no matter how large the install payload is", () => {
    // Given
    const bulkyFiles = Array.from({ length: 40 }, (_, index) => ({
      filePath: `/home/user/project/.claude/hooks/hook-${index}.sh`,
      content: "#!/bin/bash\n".concat("echo hook payload\n".repeat(200)),
      mode: 0o755,
    }));

    // When
    const smallCommand = buildHookInstallBootstrapCommand(
      "http://192.168.0.10:9736",
      issueHookInstallScript(hookFiles),
      CLIENT_ID,
    );
    const bulkyTicket = issueHookInstallScript(bulkyFiles);
    const bulkyCommand = buildHookInstallBootstrapCommand("http://192.168.0.10:9736", bulkyTicket, CLIENT_ID);

    // Then
    expect(Buffer.byteLength(readHookInstallScript(bulkyTicket.token) ?? "")).toBeGreaterThan(100_000);
    expect(bulkyCommand.length).toBe(smallCommand.length);
  });

  it("원격 설치 다운로드는 curl의 Unix 소켓을 사용한다", () => {
    // When
    const command = buildHookInstallBootstrapCommand("http://192.168.0.10:9736", sampleTicket, CLIENT_ID);

    // Then
    expect(command).toContain('curl --unix-socket "$__kanvibe_socket" --connect-timeout 3 --max-time 10 -fsSL');
  });

  it("should bound the download so an unreachable hook server falls back quickly", () => {
    // When
    const command = buildHookInstallBootstrapCommand("http://192.168.0.10:9736", sampleTicket, CLIENT_ID);

    // Then
    expect(command).toContain("--connect-timeout 3");
    expect(command).toContain("--max-time 10");
  });

  it("should download fully before executing so a truncated script never runs", () => {
    // When
    const command = buildHookInstallBootstrapCommand("http://192.168.0.10:9736", sampleTicket, CLIENT_ID);

    // Then
    expect(command).toContain(" && __kanvibe_install_script=$(");
    expect(command).toContain('printf \'%s\\n\' "$__kanvibe_install_script" | sh');
  });

  it("should execute the downloaded script only after its checksum matches", () => {
    // When
    const command = buildHookInstallBootstrapCommand("http://192.168.0.10:9736", sampleTicket, CLIENT_ID);

    // Then
    const checksumGuardIndex = command.indexOf(
      `[ "$__kanvibe_install_checksum" = '${sampleTicket.scriptSha256}' ]`,
    );
    const executionIndex = command.indexOf('printf \'%s\\n\' "$__kanvibe_install_script" | sh');
    expect(checksumGuardIndex).toBeGreaterThan(-1);
    expect(executionIndex).toBeGreaterThan(checksumGuardIndex);
    expect(command).toContain("sha256sum 2>/dev/null || shasum -a 256");
  });

  it("should not double the slash when the hook server url has a trailing slash", () => {
    // When
    const command = buildHookInstallBootstrapCommand("http://192.168.0.10:9736/", sampleTicket, CLIENT_ID);

    // Then
    expect(command).toContain("http://192.168.0.10:9736/api/install/hooks.sh'");
    expect(command).not.toContain("9736//api");
  });
});
