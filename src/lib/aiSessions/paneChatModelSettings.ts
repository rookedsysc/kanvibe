/** 슬라이더의 한 칸. id는 CLI 명령에 그대로 쓰는 값이다 */
export interface PaneChatEffortLevel {
  id: string;
  label: string;
}

export interface PaneChatModelOption {
  /** CLI의 `/model` 명령에 그대로 쓰는 값. Claude는 별칭, Codex는 slug다 */
  id: string;
  label: string;
  /** 낮은 단계부터. 비어 있으면 이 모델은 effort를 고를 수 없다 */
  effortLevels: PaneChatEffortLevel[];
  supportsFast: boolean;
}

/** pane에서 돌고 있는 CLI의 현재 모델 설정과 고를 수 있는 선택지 */
export interface PaneChatModelSettings {
  /** [models] 중 하나의 id. 목록에 없는 모델을 쓰고 있으면 그 모델도 목록에 넣어 준다 */
  model: string | null;
  effort: string | null;
  /** 기록에서 알아내지 못했으면 null이다 */
  isFast: boolean | null;
  models: PaneChatModelOption[];
}

export type PaneChatModelSettingChange =
  | { kind: "model"; model: string }
  | { kind: "effort"; effort: string }
  | { kind: "fast"; isFast: boolean }
  /** effort를 그 모델의 기본값으로 되돌린다 */
  | { kind: "reset" };

/** 요청으로 들어온 값이 변경 하나의 모양을 갖췄는지 확인한다. 값이 지금 고를 수 있는 것인지는 명령을 만들 때 따진다 */
export function parsePaneChatModelSettingChange(value: unknown): PaneChatModelSettingChange | null {
  const change = asRecord(value);
  switch (change?.kind) {
    case "model": return typeof change.model === "string" ? { kind: "model", model: change.model } : null;
    case "effort": return typeof change.effort === "string" ? { kind: "effort", effort: change.effort } : null;
    case "fast": return typeof change.isFast === "boolean" ? { kind: "fast", isFast: change.isFast } : null;
    case "reset": return { kind: "reset" };
    default: return null;
  }
}

export interface PaneChatModelState {
  /** CLI가 기록에 남긴 모델 이름. Claude는 `claude-opus-5-5` 같은 전체 ID다 */
  model: string | null;
  effort: string | null;
  isFast: boolean | null;
}

const EFFORT_LABELS: Record<string, string> = { low: "Light", medium: "Medium", high: "High", xhigh: "Extra High", max: "Max", ultra: "Ultra" };

function toEffortLevels(ids: string[]): PaneChatEffortLevel[] {
  return ids.map((id) => ({ id, label: EFFORT_LABELS[id] ?? id }));
}

/**
 * Claude Code가 `/model`로 받는 별칭과 그 모델군의 선택지.
 * 계정별 실제 목록은 CLI 밖에서 읽을 방법이 없어, 공식 문서의 별칭과 단계 표를 그대로 옮긴다.
 */
const CLAUDE_MODEL_FAMILIES: { id: string; label: string; supportsFast: boolean }[] = [
  { id: "fable", label: "Fable", supportsFast: true },
  { id: "opus", label: "Opus", supportsFast: true },
  { id: "sonnet", label: "Sonnet", supportsFast: false },
  { id: "haiku", label: "Haiku", supportsFast: false },
];
const COMMAND_OUTPUT_TAG = "<local-command-stdout>";
const CLAUDE_EFFORT_LEVELS = toEffortLevels(["low", "medium", "high", "xhigh", "max"]);

function asRecord(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : null;
}

function asString(value: unknown): string | null {
  return typeof value === "string" && value.length > 0 ? value : null;
}

/** Claude Code 기록의 끝부분에서 가장 최근 응답이 쓴 모델과 effort를 읽는다 */
export function readClaudeModelState(records: unknown[]): PaneChatModelState {
  const state: PaneChatModelState = { model: null, effort: null, isFast: null };
  for (const entry of records) {
    const record = asRecord(entry);
    const message = asRecord(record?.message);
    // 설정 명령의 결과는 다음 응답이 오기 전에도 기록에 남으므로, 바꾼 직후의 값을 여기서 먼저 안다
    const commandOutput = record?.type === "user" ? asString(message?.content) : null;
    if (commandOutput?.includes(COMMAND_OUTPUT_TAG)) {
      state.model = commandOutput.match(/Set model to `([^`]+)`/)?.[1] ?? state.model;
      state.effort = commandOutput.match(/Set effort level to (\w+)/)?.[1] ?? state.effort;
      continue;
    }
    const model = asString(message?.model);
    // CLI가 스스로 만든 안내 응답은 모델을 쓰지 않는다
    if (record?.type !== "assistant" || !model || model === "<synthetic>") continue;
    state.model = asString(record.requestedModel) ?? model;
    state.effort = asString(record.effort) ?? asString(record.perTurnEffort) ?? state.effort;
    const speed = asString(asRecord(message?.usage)?.speed);
    if (speed) state.isFast = speed !== "standard";
  }
  return state;
}

/** Codex 기록의 끝부분에서 가장 최근에 적용된 모델·effort·속도 등급을 읽는다 */
export function readCodexModelState(records: unknown[]): PaneChatModelState {
  const state: PaneChatModelState = { model: null, effort: null, isFast: null };
  for (const entry of records) {
    const record = asRecord(entry);
    const payload = asRecord(record?.payload);
    if (record?.type === "turn_context") {
      state.model = asString(payload?.model) ?? state.model;
      state.effort = asString(payload?.effort) ?? state.effort;
      continue;
    }
    // 턴 사이에 바꾼 설정은 이 이벤트로만 남는다. 속도 등급도 여기에만 실린다
    const settings = record?.type === "event_msg" && payload?.type === "thread_settings_applied" ? asRecord(payload.thread_settings) : null;
    if (!settings) continue;
    state.model = asString(settings.model) ?? state.model;
    state.effort = asString(settings.reasoning_effort) ?? state.effort;
    state.isFast = settings.service_tier === "priority";
  }
  return state;
}

/** Codex가 계정 기준으로 받아 둔 모델 캐시(`~/.codex/models_cache.json`)에서 고를 수 있는 모델을 읽는다 */
export function parseCodexModelCatalog(cache: unknown): PaneChatModelOption[] {
  const models = asRecord(cache)?.models;
  if (!Array.isArray(models)) return [];
  const listed = models.map(asRecord).filter((model): model is Record<string, unknown> => model?.visibility === "list" && asString(model.slug) !== null);
  listed.sort((left, right) => Number(left.priority ?? 0) - Number(right.priority ?? 0));
  return listed.map((model) => ({
    id: model.slug as string,
    label: asString(model.display_name) ?? model.slug as string,
    effortLevels: toEffortLevels((Array.isArray(model.supported_reasoning_levels) ? model.supported_reasoning_levels : [])
      .map((level) => asString(asRecord(level)?.effort))
      .filter((effort): effort is string => effort !== null)),
    supportsFast: Array.isArray(model.service_tiers) && model.service_tiers.some((tier) => asRecord(tier)?.id === "priority"),
  }));
}

function buildClaudeModelSettings(state: PaneChatModelState): PaneChatModelSettings {
  const models: PaneChatModelOption[] = CLAUDE_MODEL_FAMILIES.map((family) => ({ ...family, effortLevels: CLAUDE_EFFORT_LEVELS }));
  const family = state.model ? CLAUDE_MODEL_FAMILIES.find((candidate) => state.model!.toLowerCase().includes(candidate.id)) : null;
  return { ...state, model: family?.id ?? state.model, models: withCurrentModel(models, family?.id ?? state.model) };
}

/** 목록에 없는 모델을 쓰고 있어도 화면이 현재 모델을 보여 줄 수 있게 한다. 고를 수 있는 옵션은 알 수 없으므로 비워 둔다 */
function withCurrentModel(models: PaneChatModelOption[], current: string | null): PaneChatModelOption[] {
  if (!current || models.some((model) => model.id === current)) return models;
  return [{ id: current, label: current, effortLevels: [], supportsFast: false }, ...models];
}

export function buildPaneChatModelSettings(provider: "claude" | "codex", state: PaneChatModelState, codexCatalog: PaneChatModelOption[]): PaneChatModelSettings {
  if (provider === "claude") return buildClaudeModelSettings(state);
  return { ...state, models: withCurrentModel(codexCatalog, state.model) };
}

/**
 * 설정 변경 하나를 CLI에 칠 슬래시 명령으로 바꾼다. 지금 고를 수 없는 값이면 null이다.
 *
 * 명령에 들어가는 값은 반드시 선택지 목록에 있는 것만 쓴다. 요청에 실려 온 문자열을 그대로 터미널에 치지 않기 위해서다.
 */
export function buildPaneChatModelCommand(provider: "claude" | "codex", settings: PaneChatModelSettings, change: PaneChatModelSettingChange): string | null {
  const currentModel = settings.models.find((model) => model.id === settings.model) ?? null;
  switch (change.kind) {
    case "model": {
      const model = settings.models.find((candidate) => candidate.id === change.model);
      return model ? `/model ${model.id}` : null;
    }
    case "effort": {
      const level = currentModel?.effortLevels.find((candidate) => candidate.id === change.effort);
      if (!level) return null;
      return provider === "claude" ? `/effort ${level.id}` : `/model --reasoning=${level.id}`;
    }
    case "fast": {
      if (!currentModel?.supportsFast) return null;
      if (provider === "claude") return `/fast ${change.isFast ? "on" : "off"}`;
      return `/model --speed=${change.isFast ? "priority" : "standard"}`;
    }
    case "reset": {
      if (!currentModel || currentModel.effortLevels.length === 0) return null;
      // Codex는 모델만 다시 고르면 그 모델의 기본 effort로 돌아간다
      return provider === "claude" ? "/effort auto" : `/model ${currentModel.id}`;
    }
  }
}
