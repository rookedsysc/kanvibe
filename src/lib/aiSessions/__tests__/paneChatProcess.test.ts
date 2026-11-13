import { describe, expect, it } from "vitest";
import { findPaneChatProcess, isPaneChatProviderProcess } from "../paneChatProcess";
const shell = "100 1 100 200 ttys010 Thu Nov 12 18:00:00 2026 zsh /bin/zsh";
const row = (pid: number, parent: number, command: string, args: string) => `${pid} ${parent} 200 200 ttys010 Thu Nov 12 19:00:00 2026 ${command} ${args}`;

describe("pane 입력을 소유한 CLI 프로세스", () => {
  it("버전 이름으로 노출된 공식 Claude native 실행 경로를 인식한다", () => {
    const shell = "40181 1 40181 40185 ttys138 Fri Nov 13 11:29:03 2026 zsh /bin/zsh";
    const native = '40185 40181 40185 40185 ttys138 Fri Nov 13 11:29:04 2026 2.1.323 /Users/rookedsysc/.local/bin/claude --bare --tools --strict-mcp-config --mcp-config {"mcpServers":{}} --model qa-local';
    expect(findPaneChatProcess(`${shell}\n${native}`, 40181)).toEqual({ pid: 40185, processStartTime: "Fri Nov 13 11:29:04 2026", provider: "claude" });
    expect(isPaneChatProviderProcess("2.1.323", native.split("2.1.323 ")[1], "claude")).toBe(true);
    expect(isPaneChatProviderProcess("2.1.323", "/Users/user/.local/share/claude/versions/2.1.323 --bare", "claude")).toBe(true);
  });
  it.each([
    ["2.1.323", "/usr/bin/echo /Users/user/.local/bin/claude --model qa-local"],
    ["2.1.323", "/tmp/2.1.323 --claude /Users/user/.local/bin/claude"],
    ["2.1.323", "/Users/user/.local/share/claude/versions/2.1.322 --bare"],
    ["2.1.323", "/tmp/claude/versions/2.1.323 --bare"],
    ["vim", "/Users/user/.local/bin/claude"],
    ["echo", "echo /Users/user/.local/bin/claude"],
  ])("명령 인자에만 Claude를 언급한 프로세스는 거부한다 (%s %s)", (command, args) => {
    expect(findPaneChatProcess([shell, row(200, 100, command, args)].join("\n"), 100)).toBeNull();
    expect(isPaneChatProviderProcess(command, args, "claude")).toBe(false);
  });
  it("interactive Codex가 실행한 다른 Codex를 입력 대상으로 고르지 않는다", () => {
    expect(findPaneChatProcess([shell, row(200, 100, "codex", "/usr/local/bin/codex"), row(210, 200, "sh", "sh -c codex exec"), row(220, 210, "codex", "/usr/local/bin/codex exec prompt")].join("\n"), 100)).toBeNull();
  });
  it("검증된 npm Node wrapper의 직접 자식인 native Codex는 실제 CLI로 고른다", () => {
    expect(findPaneChatProcess([shell, row(200, 100, "node", "node /usr/local/lib/node_modules/@openai/codex/bin/codex.js"), row(201, 200, "codex", "/usr/local/lib/node_modules/@openai/codex/vendor/aarch64-apple-darwin/codex/codex")].join("\n"), 100)).toMatchObject({ pid: 201, provider: "codex" });
  });
  it("wrapper 아래에 nested Codex까지 있으면 모호한 입력 대상을 거부한다", () => {
    expect(findPaneChatProcess([shell, row(200, 100, "node", "node /usr/local/lib/node_modules/@openai/codex/bin/codex.js"), row(201, 200, "codex", "/usr/local/lib/node_modules/@openai/codex/vendor/aarch64-apple-darwin/codex/codex"), row(220, 201, "codex", "/usr/local/bin/codex exec prompt")].join("\n"), 100)).toBeNull();
  });
});
