import { describe, expect, it } from "vitest";
import { buildCurlAuthHeader, buildFetchAuthHeaders } from "@/lib/hookAuth";

describe("hookAuth", () => {
  it("curl 인증 헤더는 불필요한 문자열 없이 생성한다", () => {
    expect(buildCurlAuthHeader("token-123")).toBe([
      '  -H "X-Kanvibe-Token: token-123" \\',
      "",
    ].join("\n"));
  });

  it("fetch 인증 헤더는 토큰을 포함한다", () => {
    expect(buildFetchAuthHeaders("token-123")).toContain('"X-Kanvibe-Token": "token-123"');
  });
});
