import { execFile } from "node:child_process";
import { accessSync, constants, statSync } from "node:fs";
import { chmod, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { delimiter, join } from "node:path";
import { tmpdir } from "node:os";
import { promisify } from "node:util";
import { describe, expect, it } from "vitest";
import {
  buildShellKanvibeStatusUpdater,
  buildShellTaskIdResolver,
  extractShellTaskId,
  getShellTaskIdBindingStatus,
  hasShellKanvibeStatusJsonPersistence,
  hasShellKanvibeTargetFanout,
} from "@/lib/hookTaskBinding";

const execFileAsync = promisify(execFile);

/**
 * `process.env.PATH`에서 실제 실행 파일 경로를 찾는다. shell alias/function이 아닌
 * 진짜 바이너리만 sandbox PATH로 노출해 node 없는 hook 환경을 재현하기 위해 사용한다.
 */
function resolveRealBinary(name: string): string | null {
  const dirs = (process.env.PATH ?? "").split(delimiter).filter(Boolean);
  for (const dir of dirs) {
    const candidate = join(dir, name);
    try {
      if (!statSync(candidate).isFile()) continue;
      accessSync(candidate, constants.X_OK);
      return candidate;
    } catch {
      /* 다음 후보 탐색 */
    }
  }
  return null;
}

describe("hookTaskBinding", () => {
  it("shell resolver는 hook 파일 안에 task id를 직접 고정한다", () => {
    // Given

    // When
    const resolver = buildShellTaskIdResolver("fallback-task");

    // Then
    expect(resolver).toBe('TASK_ID="fallback-task"');
    expect(resolver).not.toContain(".kanvibe/task-id");
  });

  it("shell script에서 고정된 task id를 읽고 없으면 null을 반환한다", () => {
    // Given
    const script = [
      "#!/bin/bash",
      'TASK_ID="task-123"',
      'echo "$TASK_ID"',
    ].join("\n");

    // When
    const taskId = extractShellTaskId(script);

    // Then
    expect(taskId).toBe("task-123");
    expect(extractShellTaskId('#!/bin/bash\necho "$TASK_ID"')).toBeNull();
  });

  it("shell task id resolver는 double-quoted shell 특수 문자를 escape하고 다시 읽을 수 있다", () => {
    // Given
    const taskId = 'task-"quoted"-\\-`cmd`-$HOME';

    // When
    const resolver = buildShellTaskIdResolver(taskId);

    // Then
    expect(resolver).toBe('TASK_ID="task-\\"quoted\\"-\\\\-\\`cmd\\`-\\$HOME"');
    expect(extractShellTaskId(resolver)).toBe(taskId);
  });

  it("status updater는 status.json 저장 후 targets.json의 모든 대상에 fan-out 한다", () => {
    const updater = buildShellKanvibeStatusUpdater("review");

    expect(updater).toContain("KANVIBE_STATUS=\"review\"");
    expect(updater).toContain("status.json");
    expect(updater).toContain("targets.json");
    expect(updater).toContain("KANVIBE_TARGETS_FILE");
    expect(updater).toContain("--git-common-dir");
    expect(updater).toContain("/info/exclude");
    expect(updater).toContain(".kanvibe/");
    expect(updater).toContain("grep -qxF");
    expect(updater).toContain('"schemaVersion":1');
    expect(updater).toContain('"status":"%s"');
    expect(updater).toContain('"updatedAt":"%s"');
    expect(updater).toContain("${KANVIBE_TASK_STATE_FILE}");
    expect(updater).toContain("KANVIBE_TARGET_URL");
    expect(updater).toContain("KANVIBE_TARGET_TASK_ID");
    expect(updater).toContain("while IFS=");
    expect(updater).not.toContain("hooks-targets.json");
    expect(updater).not.toContain("task-state.json");
  });

  it("target fan-out 판정은 targets.json fan-out 없는 hook을 거부한다", () => {
    const updater = buildShellKanvibeStatusUpdater("review");
    const updaterWithoutTargets = updater.replace(/\nKANVIBE_TARGETS_FILE=[\s\S]*$/, "");

    expect(hasShellKanvibeTargetFanout(updater)).toBe(true);
    expect(hasShellKanvibeTargetFanout(updaterWithoutTargets)).toBe(false);
  });

  it("생성된 shell updater는 targets.json 대상별 task id로 status POST를 fan-out 한다", async () => {
    const repoPath = await mkdtemp(join(tmpdir(), "kanvibe-hook-fanout-"));

    try {
      const hookDir = join(repoPath, ".claude", "hooks");
      const fakeBinDir = join(repoPath, "bin");
      await mkdir(join(repoPath, ".kanvibe"), { recursive: true });
      await mkdir(hookDir, { recursive: true });
      await mkdir(fakeBinDir, { recursive: true });

      await writeFile(
        join(repoPath, ".kanvibe", "targets.json"),
        JSON.stringify({
          schemaVersion: 1,
          targets: [
            { url: "http://127.0.0.1:9736/", taskId: "local-task" },
            { url: "http://127.0.0.1:9736", taskId: "dev-task" },
          ],
        }),
        "utf8",
      );

      const curlLogPath = join(repoPath, "curl.log");
      const fakeCurlPath = join(fakeBinDir, "curl");
      await writeFile(
        fakeCurlPath,
        [
          "#!/usr/bin/env node",
          "const fs = require('fs');",
          "fs.appendFileSync(process.env.KANVIBE_CURL_LOG, JSON.stringify(process.argv.slice(2)) + '\\n');",
        ].join("\n"),
        "utf8",
      );
      await chmod(fakeCurlPath, 0o755);

      const scriptPath = join(hookDir, "kanvibe-test-hook.sh");
      await writeFile(
        scriptPath,
        [
          "#!/bin/bash",
          'KANVIBE_URL="http://fallback:9736"',
          buildShellTaskIdResolver("fallback-task"),
          buildShellKanvibeStatusUpdater("review"),
        ].join("\n"),
        "utf8",
      );
      await chmod(scriptPath, 0o755);

      await execFileAsync("bash", [scriptPath], {
        cwd: repoPath,
        env: {
          ...process.env,
          KANVIBE_CURL_LOG: curlLogPath,
          PATH: `${fakeBinDir}:${process.env.PATH ?? ""}`,
        },
      });

      const status = JSON.parse(await readFile(join(repoPath, ".kanvibe", "status.json"), "utf8")) as { status?: string };
      expect(status.status).toBe("review");

      const curlCalls = (await readFile(curlLogPath, "utf8"))
        .trim()
        .split("\n")
        .map((line) => JSON.parse(line) as string[]);
      const postUrls = curlCalls.map((args) => args.find((arg) => arg.endsWith("/api/hooks/status")));
      const payloads = curlCalls.map((args) => JSON.parse(args[args.indexOf("-d") + 1]) as { taskId: string; status: string });

      expect(postUrls).toEqual([
        "http://127.0.0.1:9736/api/hooks/status",
        "http://127.0.0.1:9736/api/hooks/status",
      ]);
      expect(payloads).toEqual([
        { taskId: "local-task", status: "review" },
        { taskId: "dev-task", status: "review" },
      ]);
    } finally {
      await rm(repoPath, { recursive: true, force: true });
    }
  });

  it("node가 PATH에 없어도 targets.json 모든 대상에 fan-out 한다", async () => {
    const repoPath = await mkdtemp(join(tmpdir(), "kanvibe-hook-nonode-"));

    try {
      const hookDir = join(repoPath, ".claude", "hooks");
      const sandboxBinDir = join(repoPath, "sandbox-bin");
      await mkdir(join(repoPath, ".kanvibe"), { recursive: true });
      await mkdir(hookDir, { recursive: true });
      await mkdir(sandboxBinDir, { recursive: true });

      await writeFile(
        join(repoPath, ".kanvibe", "targets.json"),
        JSON.stringify({
          schemaVersion: 1,
          targets: [
            { url: "http://127.0.0.1:9736/", taskId: "local-task" },
            { url: "http://127.0.0.1:9736", taskId: "dev-task" },
          ],
        }),
        "utf8",
      );

      // hook이 사용하는 POSIX 도구만 sandbox PATH로 노출한다. node는 의도적으로 제외해
      // node 미설치 hook 환경(예: Codex CLI)을 재현한다.
      const requiredTools = ["bash", "grep", "awk", "sed", "date", "mkdir", "touch", "dirname"];
      for (const tool of requiredTools) {
        const real = resolveRealBinary(tool);
        if (!real) {
          throw new Error(`테스트에 필요한 ${tool} 바이너리를 PATH에서 찾을 수 없습니다`);
        }
        await symlink(real, join(sandboxBinDir, tool));
      }
      const gitBinary = resolveRealBinary("git");
      if (gitBinary) {
        await symlink(gitBinary, join(sandboxBinDir, "git"));
      }

      // fake curl을 node가 아닌 POSIX shell script로 작성해 node 의존성을 제거한다.
      const curlLogPath = join(repoPath, "curl.log");
      const fakeCurlPath = join(sandboxBinDir, "curl");
      await writeFile(
        fakeCurlPath,
        [
          "#!/bin/sh",
          'printf \'%s\\n\' "$*" >> "$KANVIBE_CURL_LOG"',
        ].join("\n"),
        "utf8",
      );
      await chmod(fakeCurlPath, 0o755);

      const scriptPath = join(hookDir, "kanvibe-test-hook.sh");
      await writeFile(
        scriptPath,
        [
          "#!/bin/bash",
          'KANVIBE_URL="http://fallback:9736"',
          buildShellTaskIdResolver("fallback-task"),
          buildShellKanvibeStatusUpdater("review"),
        ].join("\n"),
        "utf8",
      );
      await chmod(scriptPath, 0o755);

      const bashBinary = resolveRealBinary("bash");
      await execFileAsync(bashBinary!, [scriptPath], {
        cwd: repoPath,
        env: {
          // process.env를 펼치지 않아 node가 들어 있는 디렉터리를 PATH에서 배제한다.
          PATH: sandboxBinDir,
          HOME: repoPath,
          KANVIBE_CURL_LOG: curlLogPath,
        },
      });

      const curlCalls = (await readFile(curlLogPath, "utf8"))
        .trim()
        .split("\n")
        .filter((line) => line.length > 0);
      const postedTaskIds = curlCalls
        .map((line) => line.match(/"taskId":\s*"([^"]+)"/)?.[1])
        .filter((value): value is string => Boolean(value));

      expect(postedTaskIds).toEqual(["local-task", "dev-task"]);
      expect(curlCalls.every((line) => line.includes("http://127.0.0.1:9736/api/hooks/status"))).toBe(true);
    } finally {
      await rm(repoPath, { recursive: true, force: true });
    }
  });

  it("status json persistence 판정은 legacy status.md hook을 거부한다", () => {
    const updater = buildShellKanvibeStatusUpdater("review");
    const legacyUpdater = updater.replaceAll("status.json", "status.md");

    expect(hasShellKanvibeStatusJsonPersistence(updater)).toBe(true);
    expect(hasShellKanvibeStatusJsonPersistence(legacyUpdater)).toBe(false);
  });

  it("status json persistence 판정은 common exclude 갱신 없는 hook을 거부한다", () => {
    const updater = buildShellKanvibeStatusUpdater("review");
    const updaterWithoutCommonExclude = updater.replace(
      /\nKANVIBE_GIT_COMMON_DIR=[\s\S]*?fi\n(?=\nmkdir -p)/,
      "",
    );

    expect(hasShellKanvibeStatusJsonPersistence(updaterWithoutCommonExclude)).toBe(false);
  });

  it("모든 shell hook이 같은 현재 task id를 가리키는지 판정한다", () => {
    // Given
    const scripts = [
      ['TASK_ID="task-123"', 'curl -d "{\\"taskId\\": \\"${TASK_ID}\\"}"'].join("\n"),
      ['TASK_ID="task-123"', 'curl -d "{\\"taskId\\": \\"${TASK_ID}\\"}"'].join("\n"),
    ];

    // When
    const status = getShellTaskIdBindingStatus(scripts, "task-123");
    const statusWithoutExpectedTask = getShellTaskIdBindingStatus(scripts);

    // Then
    expect(status).toEqual({
      hasTaskIdBinding: true,
      hasExpectedTaskId: true,
      boundTaskId: "task-123",
    });
    expect(statusWithoutExpectedTask).toEqual({
      hasTaskIdBinding: true,
      hasExpectedTaskId: true,
      boundTaskId: "task-123",
    });
  });

  it("shell hook의 task id가 현재 task와 다르면 expected 판정을 실패시킨다", () => {
    // Given
    const scripts = [
      ['TASK_ID="task-123"', 'curl -d "{\\"taskId\\": \\"${TASK_ID}\\"}"'].join("\n"),
      ['TASK_ID="task-123"', 'curl -d "{\\"taskId\\": \\"${TASK_ID}\\"}"'].join("\n"),
    ];

    // When
    const status = getShellTaskIdBindingStatus(scripts, "task-999");

    // Then
    expect(status).toEqual({
      hasTaskIdBinding: true,
      hasExpectedTaskId: false,
      boundTaskId: "task-123",
    });
  });

  it("shell hook끼리 서로 다른 task id를 가지거나 비어 있으면 binding 판정을 실패시킨다", () => {
    // Given
    const scripts = [
      ['TASK_ID="task-123"', 'curl -d "{\\"taskId\\": \\"${TASK_ID}\\"}"'].join("\n"),
      ['TASK_ID="task-999"', 'curl -d "{\\"taskId\\": \\"${TASK_ID}\\"}"'].join("\n"),
    ];

    // When
    const status = getShellTaskIdBindingStatus(scripts, "task-123");
    const emptyStatus = getShellTaskIdBindingStatus([], "task-123");

    // Then
    expect(status).toEqual({
      hasTaskIdBinding: false,
      hasExpectedTaskId: false,
      boundTaskId: null,
    });
    expect(emptyStatus).toEqual({
      hasTaskIdBinding: false,
      hasExpectedTaskId: false,
      boundTaskId: null,
    });
  });
});
