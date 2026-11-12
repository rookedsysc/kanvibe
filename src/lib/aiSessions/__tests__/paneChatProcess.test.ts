import { describe, expect, it } from "vitest";
import { findPaneChatProcess } from "../paneChatProcess";
const shell = "100 1 100 200 ttys010 Thu Nov 12 18:00:00 2026 zsh /bin/zsh";
const row = (pid: number, parent: number, command: string, args: string) => `${pid} ${parent} 200 200 ttys010 Thu Nov 12 19:00:00 2026 ${command} ${args}`;

describe("pane 입력을 소유한 CLI 프로세스", () => {
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
