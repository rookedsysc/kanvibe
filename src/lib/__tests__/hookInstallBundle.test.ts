// @vitest-environment node
import { describe, it, expect, beforeEach } from "vitest";
import {
  buildHookInstallBootstrapCommand,
  buildHookInstallScript,
  clearHookInstallScripts,
  HOOK_INSTALL_SCRIPT_PATH,
  HOOK_INSTALL_SUCCESS_MARKER,
  issueHookInstallScript,
  readHookInstallScript,
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

describe("issueHookInstallScript / readHookInstallScript", () => {
  it("should return the registered script for its token", () => {
    // Given
    const token = issueHookInstallScript(hookFiles);

    // When
    const script = readHookInstallScript(token);

    // Then
    expect(script).toBe(buildHookInstallScript(hookFiles));
  });

  it("should stay readable so a retried install can reuse the same token", () => {
    // Given
    const token = issueHookInstallScript(hookFiles);

    // When
    readHookInstallScript(token);

    // Then
    expect(readHookInstallScript(token)).not.toBeNull();
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
    const firstToken = issueHookInstallScript(hookFiles);
    const secondToken = issueHookInstallScript(hookFiles);

    // Then
    expect(firstToken).not.toBe(secondToken);
    expect(firstToken).toMatch(/^[0-9a-f]{48}$/);
  });
});

describe("buildHookInstallBootstrapCommand", () => {
  it("should keep the SSH command short instead of carrying the payload", () => {
    // Given
    const token = issueHookInstallScript(hookFiles);

    // When
    const command = buildHookInstallBootstrapCommand("http://192.168.0.10:9736", token);

    // Then
    expect(Buffer.byteLength(command)).toBeLessThan(400);
    expect(command).not.toContain("base64 -d");
    expect(command).toContain(`http://192.168.0.10:9736${HOOK_INSTALL_SCRIPT_PATH}?token=${token}`);
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
    );
    const bulkyToken = issueHookInstallScript(bulkyFiles);
    const bulkyCommand = buildHookInstallBootstrapCommand("http://192.168.0.10:9736", bulkyToken);

    // Then
    expect(Buffer.byteLength(readHookInstallScript(bulkyToken) ?? "")).toBeGreaterThan(100_000);
    expect(bulkyCommand.length).toBe(smallCommand.length);
  });

  it("should fall back to wget when curl is missing", () => {
    // When
    const command = buildHookInstallBootstrapCommand("http://192.168.0.10:9736", "token-value");

    // Then
    expect(command).toContain("curl -fsSL");
    expect(command).toContain("wget -qO-");
  });

  it("should download fully before executing so a truncated script never runs", () => {
    // When
    const command = buildHookInstallBootstrapCommand("http://192.168.0.10:9736", "token-value");

    // Then
    expect(command).toMatch(/^__kanvibe_install_script=\$\(/);
    expect(command).toContain('printf \'%s\\n\' "$__kanvibe_install_script" | sh');
  });

  it("should not double the slash when the hook server url has a trailing slash", () => {
    // When
    const command = buildHookInstallBootstrapCommand("http://192.168.0.10:9736/", "token-value");

    // Then
    expect(command).toContain("http://192.168.0.10:9736/api/install/hooks.sh?token=token-value");
    expect(command).not.toContain("9736//api");
  });
});
