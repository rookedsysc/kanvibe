import path from "path";
import { describe, expect, it } from "vitest";
import {
  AI_PROVIDER_CONFIG_DIR_SPECS,
  isValidAccountName,
  toAccountConfigDir,
  toAccountNameFromRoot,
  toDefaultAccountRoot,
  toNamedAccountRoot,
} from "@/lib/aiUsage/providerConfigDir";

const FAKE_HOME = "/home/tester";

describe("toAccountConfigDir", () => {
  it("루트가 곧 config dir인 provider는 루트를 그대로 쓴다", () => {
    const claudeRoot = path.join(FAKE_HOME, ".claude-work");

    expect(toAccountConfigDir(AI_PROVIDER_CONFIG_DIR_SPECS.claude, claudeRoot)).toBe(claudeRoot);
    expect(toAccountConfigDir(AI_PROVIDER_CONFIG_DIR_SPECS.codex, claudeRoot)).toBe(claudeRoot);
  });

  it("Gemini는 GEMINI_CLI_HOME이 루트를 받으므로 그 아래 .gemini가 config dir다", () => {
    const geminiRoot = path.join(FAKE_HOME, ".gemini-work");

    expect(toAccountConfigDir(AI_PROVIDER_CONFIG_DIR_SPECS.gemini, geminiRoot)).toBe(
      path.join(geminiRoot, ".gemini"),
    );
  });
});

describe("toDefaultAccountRoot", () => {
  it("Claude와 Codex의 기본 루트는 홈 아래 전용 디렉터리다", () => {
    expect(toDefaultAccountRoot(AI_PROVIDER_CONFIG_DIR_SPECS.claude, FAKE_HOME)).toBe(
      path.join(FAKE_HOME, ".claude"),
    );
    expect(toDefaultAccountRoot(AI_PROVIDER_CONFIG_DIR_SPECS.codex, FAKE_HOME)).toBe(
      path.join(FAKE_HOME, ".codex"),
    );
  });

  it("Gemini의 기본 루트는 홈 자체이고 config dir만 한 겹 아래다", () => {
    const defaultRoot = toDefaultAccountRoot(AI_PROVIDER_CONFIG_DIR_SPECS.gemini, FAKE_HOME);

    expect(defaultRoot).toBe(FAKE_HOME);
    expect(toAccountConfigDir(AI_PROVIDER_CONFIG_DIR_SPECS.gemini, defaultRoot)).toBe(
      path.join(FAKE_HOME, ".gemini"),
    );
  });
});

describe("계정 이름과 루트", () => {
  it("계정 이름으로 만든 루트에서 같은 이름을 되읽는다", () => {
    const accountRoot = toNamedAccountRoot(AI_PROVIDER_CONFIG_DIR_SPECS.gemini, "work", FAKE_HOME);

    expect(accountRoot).toBe(path.join(FAKE_HOME, ".gemini-work"));
    expect(toAccountNameFromRoot(AI_PROVIDER_CONFIG_DIR_SPECS.gemini, accountRoot)).toBe("work");
  });

  it("기본 루트에는 사용자가 붙인 이름이 없다", () => {
    const defaultRoot = toDefaultAccountRoot(AI_PROVIDER_CONFIG_DIR_SPECS.claude, FAKE_HOME);

    expect(toAccountNameFromRoot(AI_PROVIDER_CONFIG_DIR_SPECS.claude, defaultRoot)).toBeNull();
  });
});

describe("isValidAccountName", () => {
  it("홈 아래 디렉터리 이름으로 안전한 값만 받는다", () => {
    expect(isValidAccountName("work")).toBe(true);
    expect(isValidAccountName("work-2")).toBe(true);
    expect(isValidAccountName("work_2.a")).toBe(true);
  });

  it("경로를 벗어나거나 디렉터리를 숨기는 값은 거절한다", () => {
    expect(isValidAccountName("../escape")).toBe(false);
    expect(isValidAccountName("work/child")).toBe(false);
    expect(isValidAccountName("with space")).toBe(false);
    expect(isValidAccountName(".hidden")).toBe(false);
    expect(isValidAccountName("")).toBe(false);
  });
});
