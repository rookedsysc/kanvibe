import { writeFile, mkdir, access, readFile } from "fs/promises";
import path from "path";
import { addAiToolPatternsToGitExclude } from "@/lib/gitExclude";

/**
 * OpenCode는 `.opencode/plugins/` 디렉토리에 TypeScript 플러그인을 배치하여 hooks를 등록한다.
 * message.updated(user) → progress, question.asked → pending, question.replied → progress, session.idle → review 상태를 전송한다.
 */

const PLUGIN_FILE_NAME = "kanvibe-plugin.ts";
const PLUGIN_DIR_NAME = "plugins";

/** OpenCode plugin TypeScript 파일 내용을 생성한다 */
function generatePluginScript(kanvibeUrl: string, projectName: string): string {
  return `import type { Plugin } from "@opencode-ai/plugin";

/**
 * KanVibe OpenCode Plugin
 * message.updated(user) → progress, question.asked → pending,
 * question.replied → progress, session.idle → review 상태 변경
 */
export const KanvibePlugin: Plugin = async ({ $ }) => {
  const KANVIBE_URL = "${kanvibeUrl}";
  const PROJECT_NAME = "${projectName}";

  async function getBranchName(): Promise<string | null> {
    try {
      const result = await $\`git rev-parse --abbrev-ref HEAD\`.quiet();
      const branch = result.text().trim();
      if (!branch || branch === "HEAD") return null;
      return branch;
    } catch {
      return null;
    }
  }

  async function updateStatus(status: string): Promise<void> {
    const branchName = await getBranchName();
    if (!branchName) return;

    try {
      await fetch(\`\${KANVIBE_URL}/api/hooks/status\`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ branchName, projectName: PROJECT_NAME, status }),
      });
    } catch {
      /* 네트워크 에러 무시 */
    }
  }

  return {
    event: async ({ event }) => {
      if (event.type === "message.updated") {
        const message = (event as any).properties?.message;
        if (message?.role === "user") {
          await updateStatus("progress");
        }
      }
      if (event.type === "question.asked") {
        await updateStatus("pending");
      }
      if (event.type === "question.replied") {
        await updateStatus("progress");
      }
      if (event.type === "session.idle") {
        await updateStatus("review");
      }
    },
  };
};
`;
}

/** 플러그인 파일에 kanvibe 관련 코드가 포함되어 있는지 확인한다 */
function hasKanvibePlugin(pluginContent: string): boolean {
  return pluginContent.includes("KanvibePlugin") && pluginContent.includes("/api/hooks/status");
}

/**
 * 지정된 repo에 OpenCode plugin을 설정한다.
 * `.opencode/plugins/kanvibe-plugin.ts` 파일을 생성한다.
 */
export async function setupOpenCodeHooks(
  repoPath: string,
  projectName: string,
  kanvibeUrl: string
): Promise<void> {
  const openCodeDir = path.join(repoPath, ".opencode");
  const pluginsDir = path.join(openCodeDir, PLUGIN_DIR_NAME);

  await mkdir(pluginsDir, { recursive: true });

  const pluginPath = path.join(pluginsDir, PLUGIN_FILE_NAME);
  await writeFile(pluginPath, generatePluginScript(kanvibeUrl, projectName), "utf-8");

  try {
    await addAiToolPatternsToGitExclude(repoPath);
  } catch (error) {
    console.error("git exclude 패턴 추가 실패:", error);
  }
}

export interface OpenCodeHooksStatus {
  installed: boolean;
  hasPlugin: boolean;
}

/** 지정된 repo의 OpenCode plugin 설치 상태를 확인한다 */
export async function getOpenCodeHooksStatus(repoPath: string): Promise<OpenCodeHooksStatus> {
  const openCodeDir = path.join(repoPath, ".opencode");
  const pluginsDir = path.join(openCodeDir, PLUGIN_DIR_NAME);
  const pluginPath = path.join(pluginsDir, PLUGIN_FILE_NAME);

  const pluginExists = await access(pluginPath)
    .then(() => true)
    .catch(() => false);

  let hasPlugin = false;
  if (pluginExists) {
    try {
      const content = await readFile(pluginPath, "utf-8");
      hasPlugin = hasKanvibePlugin(content);
    } catch {
      /* 파일 읽기 실패 */
    }
  }

  const installed = pluginExists && hasPlugin;

  return {
    installed,
    hasPlugin,
  };
}
