import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { access, readFile } from "node:fs/promises";
import { discardPaneChatAttachment, resolvePaneChatAttachments, retainPaneChatAttachments, storePaneChatAttachment } from "../paneChatAttachmentService";

const remote = vi.hoisted(() => ({ transfer: vi.fn(), hosts: vi.fn(), exec: vi.fn() }));
vi.mock("@/lib/remoteImagePaste", () => ({ transferFileToRemoteHost: remote.transfer }));
vi.mock("@/lib/sshConfig", () => ({ parseSSHConfig: remote.hosts }));
vi.mock("@/lib/gitOperations", () => ({ execGit: remote.exec }));

const owner = { taskId: "attachment-task", targetToken: "attachment-token", identity: "attachment-cli", sshHost: null };
const created: string[] = [];
beforeEach(() => { vi.clearAllMocks(); remote.hosts.mockResolvedValue([]); remote.exec.mockResolvedValue(""); });
afterEach(async () => { for (const id of created.splice(0)) await discardPaneChatAttachment(owner.taskId, owner.targetToken, id); });
describe("pane chat attachment ownership", () => {
  it("stages bytes in an owned path and removes an unsent file", async () => {
    const file = await storePaneChatAttachment(owner, { name: "설계 문서.txt", mimeType: "text/plain", dataUrl: "data:text/plain;base64,aGVsbG8=" });
    created.push(file.id);
    expect(file).toMatchObject({ name: "설계 문서.txt", kind: "file", mimeType: "text/plain", sizeBytes: 5 });
    const [stored] = resolvePaneChatAttachments(owner, [file.id]);
    expect((await readFile(stored.path)).toString()).toBe("hello");
    await discardPaneChatAttachment(owner.taskId, owner.targetToken, file.id);
    await expect(access(stored.path)).rejects.toThrow();
  });
  it("does not substitute attachments from another pane, host or CLI lease", async () => {
    const file = await storePaneChatAttachment(owner, { name: "readme.txt", mimeType: "text/plain", dataUrl: "data:text/plain;base64,aGVsbG8=" });
    created.push(file.id);
    expect(() => resolvePaneChatAttachments({ ...owner, targetToken: "other-token" }, [file.id])).toThrow();
    expect(() => resolvePaneChatAttachments({ ...owner, sshHost: "other-host" }, [file.id])).toThrow();
    expect(() => resolvePaneChatAttachments(owner, ["../../arbitrary"])).toThrow();
    await expect(discardPaneChatAttachment("other-task", owner.targetToken, file.id)).rejects.toThrow();
  });
  it("keeps a submitted file readable after its composer chip is removed", async () => {
    const file = await storePaneChatAttachment(owner, { name: "readme.txt", mimeType: "text/plain", dataUrl: "data:text/plain;base64,aGVsbG8=" });
    const [stored] = resolvePaneChatAttachments(owner, [file.id]);
    retainPaneChatAttachments(owner, [file.id]);
    await discardPaneChatAttachment(owner.taskId, owner.targetToken, file.id);
    expect((await readFile(stored.path)).toString()).toBe("hello");
  });
  it("rejects forged image bytes before creating a managed attachment", async () => {
    await expect(storePaneChatAttachment(owner, { name: "fake.png", mimeType: "image/png", dataUrl: "data:image/png;base64,aGVsbG8=" })).rejects.toThrow();
  });
  it("bounds concurrent uploads at ten rather than racing past the per-CLI limit", async () => {
    const attempts = await Promise.allSettled(Array.from({ length: 11 }, () => storePaneChatAttachment(owner, { name: "readme.txt", mimeType: "text/plain", dataUrl: "data:text/plain;base64,aGVsbG8=" })));
    for (const result of attempts) if (result.status === "fulfilled") created.push(result.value.id);
    expect(attempts.filter((entry) => entry.status === "fulfilled")).toHaveLength(10);
    expect(attempts.filter((entry) => entry.status === "rejected")).toHaveLength(1);
  });
  it("stages and removes a remote file only on its verified host while preserving extension", async () => {
    const config = { host: "own-host", hostname: "127.0.0.1", username: "fixture", port: 2222, privateKeyPath: "/fixture/key" };
    remote.hosts.mockResolvedValue([config]);
    remote.transfer.mockResolvedValue("/tmp/kanvibe-paste-fixture.pdf");
    const remoteOwner = { ...owner, sshHost: "own-host" };
    const file = await storePaneChatAttachment(remoteOwner, { name: "document.pdf", mimeType: "application/pdf", dataUrl: "data:application/pdf;base64,JVBERi0xLjc=" });
    try {
      expect(remote.transfer).toHaveBeenCalledWith(config, Buffer.from("%PDF-1.7"), "pdf");
      expect(resolvePaneChatAttachments(remoteOwner, [file.id])).toEqual([{ id: file.id, path: "/tmp/kanvibe-paste-fixture.pdf", kind: "file" }]);
      expect(() => resolvePaneChatAttachments(owner, [file.id])).toThrow();
    } finally { await discardPaneChatAttachment(owner.taskId, owner.targetToken, file.id); }
    expect(remote.exec).toHaveBeenCalledWith("rm -f -- '/tmp/kanvibe-paste-fixture.pdf'", "own-host", expect.anything());
    remote.hosts.mockResolvedValue([]);
    remote.transfer.mockClear();
    await expect(storePaneChatAttachment(remoteOwner, { name: "document.pdf", mimeType: "application/pdf", dataUrl: "data:application/pdf;base64,JVBERi0xLjc=" })).rejects.toThrow("SSH");
    expect(remote.transfer).not.toHaveBeenCalled();
  });
});
