import { execFile } from "node:child_process";
import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
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
