import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { mockExecGit } = vi.hoisted(() => ({ mockExecGit: vi.fn() }));

vi.mock("@/lib/gitOperations", () => ({
  execGit: (...args: unknown[]) => mockExecGit(...args),
}));

import {
  fetchGitHubOwnerIconDataUrl,
  parseGitHubRepositoryReference,
  resolveProjectIconDataUrl,
} from "@/lib/githubProjectIcon";

function createIconResponse(contentType: string, bytes: Uint8Array): Response {
  return {
    ok: true,
    headers: { get: () => contentType },
    arrayBuffer: async () => bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength),
  } as unknown as Response;
}

describe("githubProjectIcon", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.spyOn(console, "warn").mockImplementation(() => {});
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("여러 형식의 GitHub remote URL에서 owner/repository를 읽어낸다", () => {
    expect(parseGitHubRepositoryReference("https://github.com/rookedsysc/kanvibe.git"))
      .toEqual({ owner: "rookedsysc", repository: "kanvibe" });
    expect(parseGitHubRepositoryReference("git@github.com:rookedsysc/kanvibe.git\n"))
      .toEqual({ owner: "rookedsysc", repository: "kanvibe" });
    expect(parseGitHubRepositoryReference("ssh://git@github.com/rookedsysc/kanvibe"))
      .toEqual({ owner: "rookedsysc", repository: "kanvibe" });
    expect(parseGitHubRepositoryReference("https://github.com/rookedsysc/kanvibe/"))
      .toEqual({ owner: "rookedsysc", repository: "kanvibe" });
  });

  it("GitHub이 아니거나 owner 형식이 어긋나는 remote는 아이콘 대상이 아니다", () => {
    expect(parseGitHubRepositoryReference("https://gitlab.com/team/app.git")).toBeNull();
    expect(parseGitHubRepositoryReference("")).toBeNull();
    expect(parseGitHubRepositoryReference("https://github.com/-bad-owner/app.git")).toBeNull();
    expect(parseGitHubRepositoryReference("https://github.com/own er/app.git")).toBeNull();
  });

  it("GitHub 아바타를 data URL로 변환한다", async () => {
    const iconBytes = new Uint8Array([137, 80, 78, 71]);
    const mockFetch = vi.fn().mockResolvedValue(createIconResponse("image/png", iconBytes));
    vi.stubGlobal("fetch", mockFetch);

    await expect(fetchGitHubOwnerIconDataUrl("rookedsysc")).resolves.toBe(
      `data:image/png;base64,${Buffer.from(iconBytes).toString("base64")}`,
    );
    expect(mockFetch.mock.calls[0][0]).toContain("https://github.com/rookedsysc.png");
  });

  it("이미지가 아닌 응답이나 네트워크 실패는 아이콘 없음으로 처리한다", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(createIconResponse("text/html", new Uint8Array([1]))));
    await expect(fetchGitHubOwnerIconDataUrl("rookedsysc")).resolves.toBeNull();

    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("offline")));
    await expect(fetchGitHubOwnerIconDataUrl("rookedsysc")).resolves.toBeNull();
  });

  it("remote가 없는 저장소는 아이콘 없이 등록되게 한다", async () => {
    mockExecGit.mockRejectedValue(new Error("no origin"));
    const mockFetch = vi.fn();
    vi.stubGlobal("fetch", mockFetch);

    await expect(resolveProjectIconDataUrl("/repo", null)).resolves.toBeNull();
    expect(mockFetch).not.toHaveBeenCalled();
  });

  it("origin remote를 읽어 아이콘 data URL을 확보한다", async () => {
    mockExecGit.mockResolvedValue("git@github.com:rookedsysc/kanvibe.git\n");
    const iconBytes = new Uint8Array([1, 2, 3]);
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(createIconResponse("image/png", iconBytes)));

    await expect(resolveProjectIconDataUrl("/remote/repo", "remote-host")).resolves.toBe(
      `data:image/png;base64,${Buffer.from(iconBytes).toString("base64")}`,
    );
    expect(mockExecGit).toHaveBeenCalledWith(
      "git -C '/remote/repo' config --get remote.origin.url",
      "remote-host",
    );
  });
});
