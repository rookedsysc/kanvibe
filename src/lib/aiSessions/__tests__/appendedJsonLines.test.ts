import { appendFile, mkdtemp, rename, rm, writeFile } from "fs/promises";
import { tmpdir } from "os";
import path from "path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { readAppendedJsonLines } from "@/lib/aiSessions/shared";

let directory: string;
let filePath: string;
const line = (value: unknown) => `${JSON.stringify(value)}\n`;

beforeEach(async () => {
  directory = await mkdtemp(path.join(tmpdir(), "kv-appended-jsonl-"));
  filePath = path.join(directory, "session.jsonl");
});
afterEach(async () => { await rm(directory, { recursive: true, force: true }); });

describe("readAppendedJsonLines", () => {
  it("바뀌지 않은 파일은 같은 배열과 같은 줄 객체를 다시 돌려준다", async () => {
    await writeFile(filePath, line({ id: 1 }) + line({ id: 2 }));
    const first = await readAppendedJsonLines(filePath);
    const firstLine = first[0];
    const second = await readAppendedJsonLines(filePath);
    expect(second).toBe(first);
    expect(second[0]).toBe(firstLine);
    expect(second).toEqual([{ id: 1 }, { id: 2 }]);
  });

  it("뒤에 붙은 줄만 읽어 같은 배열에 이어 붙이고 앞쪽 줄은 다시 만들지 않는다", async () => {
    await writeFile(filePath, line({ id: 1 }));
    const first = await readAppendedJsonLines(filePath);
    const firstLine = first[0];
    await appendFile(filePath, line({ id: 2 }) + line({ id: 3 }));
    const second = await readAppendedJsonLines(filePath);
    expect(second).toBe(first);
    expect(second[0]).toBe(firstLine);
    expect(second).toEqual([{ id: 1 }, { id: 2 }, { id: 3 }]);
  });

  it("중간까지만 쓰인 마지막 줄은 건너뛰고, 그 줄이 완성되면 한 번만 싣는다", async () => {
    await writeFile(filePath, `${line({ id: 1 })}{"id":2,"text":"hal`);
    const lines = await readAppendedJsonLines(filePath);
    expect(lines).toEqual([{ id: 1 }]);
    await appendFile(filePath, `f"}\n`);
    expect(await readAppendedJsonLines(filePath)).toEqual([{ id: 1 }, { id: 2, text: "half" }]);
    expect(await readAppendedJsonLines(filePath)).toHaveLength(2);
  });

  it("줄바꿈 없이 끝난 온전한 마지막 줄도 싣고, 뒤늦게 줄바꿈이 붙어도 다시 싣지 않는다", async () => {
    await writeFile(filePath, `${line({ id: 1 })}${JSON.stringify({ id: 2 })}`);
    expect(await readAppendedJsonLines(filePath)).toEqual([{ id: 1 }, { id: 2 }]);
    await appendFile(filePath, `\n${line({ id: 3 })}`);
    expect(await readAppendedJsonLines(filePath)).toEqual([{ id: 1 }, { id: 2 }, { id: 3 }]);
  });

  it("빈 줄과 해석할 수 없는 줄은 건너뛴다", async () => {
    await writeFile(filePath, `${line({ id: 1 })}\nnot json\n${line({ id: 2 })}`);
    expect(await readAppendedJsonLines(filePath)).toEqual([{ id: 1 }, { id: 2 }]);
  });

  it("여러 바이트 글자가 든 줄 뒤에 붙은 줄도 정확한 위치에서 이어 읽는다", async () => {
    await writeFile(filePath, line({ text: "한글 기록 🙂" }));
    await readAppendedJsonLines(filePath);
    await appendFile(filePath, line({ text: "다음 줄" }));
    expect(await readAppendedJsonLines(filePath)).toEqual([{ text: "한글 기록 🙂" }, { text: "다음 줄" }]);
  });

  it("파일이 짧아지면 처음부터 다시 읽어 새 내용만 담은 새 배열을 돌려준다", async () => {
    await writeFile(filePath, line({ id: 1 }) + line({ id: 2 }));
    const first = await readAppendedJsonLines(filePath);
    await writeFile(filePath, line({ id: 9 }));
    const second = await readAppendedJsonLines(filePath);
    expect(second).not.toBe(first);
    expect(second).toEqual([{ id: 9 }]);
  });

  it("같은 자리에서 더 긴 다른 내용으로 다시 쓰이면 이어 붙이지 않고 처음부터 다시 읽는다", async () => {
    await writeFile(filePath, line({ id: 1 }));
    const first = await readAppendedJsonLines(filePath);
    await writeFile(filePath, line({ id: 7 }) + line({ id: 8 }));
    const second = await readAppendedJsonLines(filePath);
    expect(second).not.toBe(first);
    expect(second).toEqual([{ id: 7 }, { id: 8 }]);
  });

  it("다른 파일로 바꿔치기되면 처음부터 다시 읽는다", async () => {
    await writeFile(filePath, line({ id: 1 }));
    const first = await readAppendedJsonLines(filePath);
    const replacement = path.join(directory, "replacement.jsonl");
    await writeFile(replacement, line({ id: 1 }) + line({ id: 5 }));
    await rename(replacement, filePath);
    const second = await readAppendedJsonLines(filePath);
    expect(second).not.toBe(first);
    expect(second).toEqual([{ id: 1 }, { id: 5 }]);
  });

  it("같은 파일을 동시에 읽어도 붙은 줄을 두 번 싣지 않는다", async () => {
    await writeFile(filePath, line({ id: 1 }));
    await readAppendedJsonLines(filePath);
    await appendFile(filePath, line({ id: 2 }));
    const [left, right] = await Promise.all([readAppendedJsonLines(filePath), readAppendedJsonLines(filePath)]);
    expect(left).toBe(right);
    expect(left).toEqual([{ id: 1 }, { id: 2 }]);
  });

  it("없는 파일은 읽기 오류로 알린다", async () => {
    await expect(readAppendedJsonLines(path.join(directory, "missing.jsonl"))).rejects.toMatchObject({ code: "ENOENT" });
  });
});
