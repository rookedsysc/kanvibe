import { mkdtemp, mkdir, writeFile, rm } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { aggregateAiSessions, getAiSessionDetail } from "@/lib/aiSessions/aggregateAiSessions";
import { AI_SESSION_PROVIDERS } from "@/lib/aiSessions/providers";

let home: string;
const context = { worktreePath: "/worktree/integration", repoPath: "/worktree/integration" };
beforeEach(async () => {
  home = await mkdtemp(path.join(tmpdir(), "provider-history-"));
  vi.stubEnv("HOME", home);
  for (const name of ["HERMES_HOME", "PI_CODING_AGENT_DIR", "OMP_CODING_AGENT_DIR", "PRIME_AGENT_DIR", "COPILOT_HOME", "GROK_HOME", "DEVIN_HOME", "CLINE_SESSION_DATA_DIR", "KIMI_CODE_HOME", "XDG_DATA_HOME", "JCODE_HOME"]) vi.stubEnv(name, "");
});
afterEach(async () => { vi.unstubAllEnvs(); await rm(home, { recursive: true, force: true }); });
describe("provider History integration", () => {
  it("reports all 25 Orca History providers and opens an added provider through the public service", async () => {
    const folder = path.join(home, ".hermes/sessions");
    await mkdir(folder, { recursive: true });
    await writeFile(path.join(folder, "session_fixture.json"), JSON.stringify({ session_id: "hermes-integration", cwd: context.worktreePath, messages: [{ role: "user", content: "Question" }, { role: "assistant", content: "Answer" }] }));
    const result = await aggregateAiSessions(context);
    expect(new Set(result.sources.map((source) => source.provider))).toEqual(new Set(AI_SESSION_PROVIDERS));
    expect(result.sources).toHaveLength(25);
    expect(result.sessions.map((session) => session.id)).toEqual(["hermes-integration"]);
    const detail = await getAiSessionDetail(context, "hermes", "hermes-integration", result.sessions[0].sourceRef);
    expect(detail?.messages.map((message) => message.fullText)).toEqual(["Answer", "Question"]);
    expect(await getAiSessionDetail({ worktreePath: "/foreign", repoPath: "/foreign" }, "hermes", "hermes-integration", result.sessions[0].sourceRef)).toBeNull();
  });
});
