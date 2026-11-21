import { describe, expect, it } from "vitest";
import { buildPaneChatModelCommand, buildPaneChatModelSettings, parseCodexModelCatalog, readClaudeModelState, readCodexModelState, type PaneChatModelSettings } from "../paneChatModelSettings";

const claudeTurn = (model: string, effort: string, speed = "standard") => ({ type: "assistant", effort, requestedModel: model, message: { model, usage: { speed } } });
const claudeCommandOutput = (text: string) => ({ type: "user", message: { content: `<local-command-stdout>${text}</local-command-stdout>` } });
const codexCache = {
  models: [
    { slug: "gpt-hidden", display_name: "Hidden", visibility: "hide", priority: 1, supported_reasoning_levels: [{ effort: "low" }], service_tiers: [] },
    { slug: "gpt-luna", display_name: "GPT Luna", visibility: "list", priority: 3, supported_reasoning_levels: [], service_tiers: [] },
    { slug: "gpt-sol", display_name: "GPT Sol", visibility: "list", priority: 2, supported_reasoning_levels: [{ effort: "low" }, { effort: "high" }, { effort: "ultra" }], service_tiers: [{ id: "priority", name: "Fast" }] },
  ],
};
const codexSettings = (overrides: Partial<PaneChatModelSettings> = {}): PaneChatModelSettings =>
  ({ ...buildPaneChatModelSettings("codex", { model: "gpt-sol", effort: "high", isFast: false }, parseCodexModelCatalog(codexCache)), ...overrides });
const claudeSettings = (model: string | null = "claude-opus-5-5") => buildPaneChatModelSettings("claude", { model, effort: "medium", isFast: false }, []);

describe("Claude Code 기록의 모델 설정", () => {
  it("가장 최근 응답이 쓴 모델과 effort를 읽는다", () => {
    expect(readClaudeModelState([claudeTurn("claude-opus-5-5", "medium"), claudeTurn("claude-fable-5-2", "high")]))
      .toEqual({ model: "claude-fable-5-2", effort: "high", isFast: false });
  });

  it("설정 명령의 결과는 다음 응답이 오기 전에도 반영한다", () => {
    const state = readClaudeModelState([
      claudeTurn("claude-opus-5-5", "medium"),
      claudeCommandOutput("Set model to `Fable 5.2` and saved as your default for new sessions"),
      claudeCommandOutput("Set effort level to high (saved as your default for new sessions)"),
    ]);
    expect(state).toMatchObject({ model: "Fable 5.2", effort: "high" });
    expect(buildPaneChatModelSettings("claude", state, []).model).toBe("fable");
  });

  it("CLI가 스스로 만든 안내 응답과 응답이 없는 기록에서는 값을 지어내지 않는다", () => {
    expect(readClaudeModelState([{ type: "assistant", message: { model: "<synthetic>" } }, { type: "user", message: { content: "안녕" } }]))
      .toEqual({ model: null, effort: null, isFast: null });
  });

  it("표준이 아닌 속도로 응답했으면 빠른 모드로 본다", () => {
    expect(readClaudeModelState([claudeTurn("claude-opus-5-5", "medium", "fast")]).isFast).toBe(true);
  });
});

describe("Codex 기록의 모델 설정", () => {
  it("턴 사이에 바꾼 설정을 턴 시작 기록보다 나중 값으로 읽는다", () => {
    const state = readCodexModelState([
      { type: "turn_context", payload: { model: "gpt-sol", effort: "low" } },
      { type: "event_msg", payload: { type: "thread_settings_applied", thread_settings: { model: "gpt-sol", reasoning_effort: "ultra", service_tier: "priority" } } },
    ]);
    expect(state).toEqual({ model: "gpt-sol", effort: "ultra", isFast: true });
  });

  it("속도 등급이 실리지 않는 턴 시작 기록만으로는 빠른 모드를 단정하지 않는다", () => {
    expect(readCodexModelState([{ type: "turn_context", payload: { model: "gpt-sol", effort: "low" } }])).toEqual({ model: "gpt-sol", effort: "low", isFast: null });
  });
});

describe("고를 수 있는 모델 목록", () => {
  it("Codex 캐시에서 숨긴 모델을 빼고 우선순위대로 늘어놓는다", () => {
    expect(parseCodexModelCatalog(codexCache)).toEqual([
      { id: "gpt-sol", label: "GPT Sol", effortLevels: [{ id: "low", label: "Light" }, { id: "high", label: "High" }, { id: "ultra", label: "Ultra" }], supportsFast: true },
      { id: "gpt-luna", label: "GPT Luna", effortLevels: [], supportsFast: false },
    ]);
  });

  it.each([null, "not-json", { models: "none" }])("형식이 다른 캐시(%j)는 빈 목록으로 다룬다", (cache) => {
    expect(parseCodexModelCatalog(cache)).toEqual([]);
  });

  it("Claude는 전체 모델 ID를 별칭으로 바꾸고 빠른 모드는 지원하는 모델군에만 연다", () => {
    const settings = claudeSettings("claude-sonnet-5-5");
    expect(settings.model).toBe("sonnet");
    expect(settings.models.map((model) => [model.id, model.supportsFast])).toEqual([["fable", true], ["opus", true], ["sonnet", false], ["haiku", false]]);
  });

  it("목록에 없는 모델을 쓰고 있으면 옵션 없이 현재 모델로만 넣는다", () => {
    const settings = buildPaneChatModelSettings("codex", { model: "gpt-private", effort: null, isFast: null }, parseCodexModelCatalog(codexCache));
    expect(settings.models[0]).toEqual({ id: "gpt-private", label: "gpt-private", effortLevels: [], supportsFast: false });
  });
});

describe("설정 변경을 CLI 명령으로 바꾸기", () => {
  it.each([
    ["claude", { kind: "model", model: "sonnet" }, "/model sonnet"],
    ["claude", { kind: "effort", effort: "xhigh" }, "/effort xhigh"],
    ["claude", { kind: "fast", isFast: true }, "/fast on"],
    ["claude", { kind: "fast", isFast: false }, "/fast off"],
    ["claude", { kind: "reset" }, "/effort auto"],
    ["codex", { kind: "model", model: "gpt-luna" }, "/model gpt-luna"],
    ["codex", { kind: "effort", effort: "ultra" }, "/model --reasoning=ultra"],
    ["codex", { kind: "fast", isFast: true }, "/model --speed=priority"],
    ["codex", { kind: "fast", isFast: false }, "/model --speed=standard"],
    ["codex", { kind: "reset" }, "/model gpt-sol"],
  ] as const)("%s의 %j는 %s를 친다", (provider, change, command) => {
    expect(buildPaneChatModelCommand(provider, provider === "claude" ? claudeSettings() : codexSettings(), change)).toBe(command);
  });

  it.each([
    ["목록에 없는 모델", codexSettings(), { kind: "model", model: "gpt-sol; rm -rf /" }],
    ["현재 모델에 없는 단계", codexSettings(), { kind: "effort", effort: "medium" }],
    ["effort가 없는 모델의 단계", codexSettings({ model: "gpt-luna" }), { kind: "effort", effort: "low" }],
    ["빠른 모드가 없는 모델의 토글", codexSettings({ model: "gpt-luna" }), { kind: "fast", isFast: true }],
    ["effort가 없는 모델의 되돌리기", codexSettings({ model: "gpt-luna" }), { kind: "reset" }],
    ["현재 모델을 모르는 상태의 단계", codexSettings({ model: null }), { kind: "effort", effort: "low" }],
  ] as const)("%s는 아무 명령도 만들지 않는다", (_, settings, change) => {
    expect(buildPaneChatModelCommand("codex", settings, change)).toBeNull();
  });
});
