export const AI_SESSION_PROVIDER_LABELS = {
  "claude": "Claude",
  "codebuddy": "CodeBuddy",
  "qoder": "Qoder",
  "codex": "Codex",
  "hermes": "Hermes",
  "pi": "Pi",
  "omp": "OMP",
  "prime-agent": "Prime Agent",
  "cursor": "Cursor",
  "gemini": "Gemini",
  "antigravity": "Antigravity",
  "rovo": "Rovo Dev",
  "copilot": "GitHub Copilot",
  "opencode": "OpenCode",
  "opencode2": "OpenCode 2",
  "zcode": "ZCode",
  "grok": "Grok",
  "openclaw": "OpenClaw",
  "devin": "Devin",
  "droid": "Droid",
  "cline": "Cline",
  "kimi": "Kimi",
  "muse": "Muse",
  "jcode": "Jcode",
  "kiro": "Kiro",
} as const;

export type AiSessionProvider = keyof typeof AI_SESSION_PROVIDER_LABELS;

export const AI_SESSION_PROVIDERS = Object.keys(AI_SESSION_PROVIDER_LABELS) as AiSessionProvider[];

export function isAiSessionProvider(value: unknown): value is AiSessionProvider {
  return typeof value === "string" && Object.hasOwn(AI_SESSION_PROVIDER_LABELS, value);
}
