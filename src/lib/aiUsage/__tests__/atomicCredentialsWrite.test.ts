import { mkdtemp, readFile, readdir, rm, stat, writeFile } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { writeCredentialsAtomically } from "@/lib/aiUsage/atomicCredentialsWrite";

describe("writeCredentialsAtomically", () => {
  let workingDirectory: string;

  beforeEach(async () => {
    workingDirectory = await mkdtemp(path.join(tmpdir(), "kanvibe-credentials-"));
  });

  afterEach(async () => {
    await rm(workingDirectory, { recursive: true, force: true });
  });

  it("기존 파일을 새 내용으로 바꾸고 소유자 전용 권한으로 남긴다", async () => {
    const targetPath = path.join(workingDirectory, ".credentials.json");
    await writeFile(targetPath, JSON.stringify({ claudeAiOauth: { accessToken: "old" } }), "utf-8");

    await writeCredentialsAtomically(targetPath, JSON.stringify({ claudeAiOauth: { accessToken: "new" } }));

    expect(JSON.parse(await readFile(targetPath, "utf-8"))).toEqual({
      claudeAiOauth: { accessToken: "new" },
    });
    expect((await stat(targetPath)).mode & 0o777).toBe(0o600);
  });

  it("쓰기가 끝나면 임시 파일을 남기지 않는다", async () => {
    const targetPath = path.join(workingDirectory, ".credentials.json");

    await writeCredentialsAtomically(targetPath, "{}");

    expect(await readdir(workingDirectory)).toEqual([".credentials.json"]);
  });

  /** 임시 파일 쓰기가 실패하면 rename에 도달하지 못하므로 원본이 그대로 남아야 한다 */
  it("쓰기에 실패하면 예외를 던지고 기존 파일을 건드리지 않는다", async () => {
    const targetPath = path.join(workingDirectory, "missing-directory", ".credentials.json");
    const survivingPath = path.join(workingDirectory, ".credentials.json");
    await writeFile(survivingPath, "original", "utf-8");

    await expect(writeCredentialsAtomically(targetPath, "replacement")).rejects.toThrow();

    expect(await readFile(survivingPath, "utf-8")).toBe("original");
  });
});
