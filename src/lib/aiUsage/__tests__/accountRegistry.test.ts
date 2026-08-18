import path from "path";
import { describe, expect, it } from "vitest";
import {
  addAccountRegistration,
  parseAccountRegistrations,
  removeAccountRegistration,
  serializeAccountRegistrations,
  type AiAccountRegistration,
} from "@/lib/aiUsage/accountRegistry";

const FAKE_HOME = "/home/tester";

function createRegistration(): AiAccountRegistration {
  return {
    provider: "claude",
    accountRoot: path.join(FAKE_HOME, ".claude-work"),
    accountName: "work",
  };
}

describe("parseAccountRegistrations", () => {
  it("저장한 목록을 그대로 되읽는다", () => {
    const registrations = [createRegistration()];

    expect(parseAccountRegistrations(serializeAccountRegistrations(registrations)))
      .toEqual(registrations);
  });

  it("저장된 값이 없으면 빈 목록이다", () => {
    expect(parseAccountRegistrations(null)).toEqual([]);
  });

  it("깨진 JSON은 고쳐 쓰지 않고 버린다", () => {
    expect(parseAccountRegistrations("{not json")).toEqual([]);
  });

  it("해석할 수 없는 항목만 버리고 나머지는 살린다", () => {
    const registration = createRegistration();
    const raw = JSON.stringify([
      registration,
      { provider: "unknown", accountRoot: "/x", accountName: "x" },
      { provider: "claude", accountRoot: "/y", accountName: "../escape" },
      { provider: "codex", accountName: "no-root" },
    ]);

    expect(parseAccountRegistrations(raw)).toEqual([registration]);
  });
});

describe("addAccountRegistration", () => {
  it("provider 규칙대로 계정 루트를 만들어 등록한다", () => {
    const registrations = addAccountRegistration([], "gemini", "work", FAKE_HOME);

    expect(registrations).toEqual([
      { provider: "gemini", accountRoot: path.join(FAKE_HOME, ".gemini-work"), accountName: "work" },
    ]);
  });

  it("같은 계정을 다시 등록해도 항목이 늘지 않는다", () => {
    const first = addAccountRegistration([], "claude", "work", FAKE_HOME);
    const second = addAccountRegistration(first, "claude", "work", FAKE_HOME);

    expect(second).toHaveLength(1);
  });
});

describe("removeAccountRegistration", () => {
  it("루트가 같은 항목만 지운다", () => {
    const registrations = addAccountRegistration(
      addAccountRegistration([], "claude", "work", FAKE_HOME),
      "claude",
      "personal",
      FAKE_HOME,
    );

    const remaining = removeAccountRegistration(
      registrations,
      path.join(FAKE_HOME, ".claude-work"),
    );

    expect(remaining.map((registration) => registration.accountName)).toEqual(["personal"]);
  });
});
