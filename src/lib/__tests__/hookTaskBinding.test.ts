import { describe, expect, it } from "vitest";
import {
  buildShellKanvibeStatusUpdater,
  buildShellTaskIdResolver,
  extractShellTaskId,
  getShellTaskIdBindingStatus,
  hasShellKanvibeStatusJsonPersistence,
} from "@/lib/hookTaskBinding";

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

  it("status updater는 target fan-out 없이 status.json에 상태만 저장한다", () => {
    const updater = buildShellKanvibeStatusUpdater("review");

    expect(updater).toContain("KANVIBE_STATUS=\"review\"");
    expect(updater).toContain("status.json");
    expect(updater).toContain("--git-common-dir");
    expect(updater).toContain("/info/exclude");
    expect(updater).toContain(".kanvibe/status.json");
    expect(updater).toContain("grep -qxF");
    expect(updater).toContain('"schemaVersion":1');
    expect(updater).toContain('"status":"%s"');
    expect(updater).toContain('"updatedAt":"%s"');
    expect(updater).toContain("${KANVIBE_TASK_STATE_FILE}");
    expect(updater).not.toContain("hooks-targets.json");
    expect(updater).not.toContain("task-state.json");
    expect(updater).not.toContain("KANVIBE_TARGET_ROWS");
    expect(updater).not.toContain("while IFS=");
    expect(updater).not.toContain("taskId, status");
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
