import { beforeEach, describe, expect, it, vi } from "vitest";
import { EventEmitter } from "events";

const mocks = vi.hoisted(() => ({
  spawn: vi.fn(),
  mkdtemp: vi.fn(),
  writeFile: vi.fn(),
  rm: vi.fn(),
  tmpdir: vi.fn(() => "/tmp"),
  randomUUID: vi.fn(() => "fixed-uuid"),
}));

vi.mock("child_process", () => ({
  default: { spawn: mocks.spawn },
  spawn: mocks.spawn,
}));

vi.mock("fs/promises", () => ({
  default: {
    mkdtemp: mocks.mkdtemp,
    writeFile: mocks.writeFile,
    rm: mocks.rm,
    mkdir: vi.fn(),
    readFile: vi.fn(),
  },
  mkdtemp: mocks.mkdtemp,
  writeFile: mocks.writeFile,
  rm: mocks.rm,
  mkdir: vi.fn(),
  readFile: vi.fn(),
}));

vi.mock("os", () => ({
  default: { tmpdir: mocks.tmpdir },
  tmpdir: mocks.tmpdir,
}));

vi.mock("crypto", () => ({
  default: { randomUUID: mocks.randomUUID },
  randomUUID: mocks.randomUUID,
}));

class FakeChildProcess extends EventEmitter {
  stderr = new EventEmitter();
}

const SSH_CONFIG = {
  host: "app-prod",
  hostname: "example.com",
  port: 2202,
  username: "tester",
  privateKeyPath: "/tmp/test-key",
};

describe("remoteImagePaste.decodeDataUrlToBuffer", () => {
  it("decodes the base64 payload of a data URL", async () => {
    // Given
    const { decodeDataUrlToBuffer } = await import("@/lib/remoteImagePaste");
    const payload = Buffer.from("hello").toString("base64");

    // When
    const buffer = decodeDataUrlToBuffer(`data:image/png;base64,${payload}`);

    // Then
    expect(buffer.toString()).toBe("hello");
  });

  it("throws when the data URL has no comma separator", async () => {
    // Given
    const { decodeDataUrlToBuffer } = await import("@/lib/remoteImagePaste");

    // When & Then
    expect(() => decodeDataUrlToBuffer("not-a-data-url")).toThrow();
  });
});

describe("remoteImagePaste.buildRemoteImagePastePath", () => {
  it("builds a fixed /tmp path from the given uuid", async () => {
    // Given
    const { buildRemoteImagePastePath } = await import("@/lib/remoteImagePaste");

    // When & Then
    expect(buildRemoteImagePastePath("fixed-uuid")).toBe("/tmp/kanvibe-paste-fixed-uuid.png");
  });
});

describe("remoteImagePaste.transferImageToRemoteHost", () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    mocks.tmpdir.mockReturnValue("/tmp");
    mocks.randomUUID.mockReturnValue("fixed-uuid");
    mocks.mkdtemp.mockResolvedValue("/tmp/kanvibe-paste-xyz");
    mocks.writeFile.mockResolvedValue(undefined);
    mocks.rm.mockResolvedValue(undefined);
  });

  it("uploads the image via scp and resolves the remote path on exit code 0", async () => {
    // Given
    const child = new FakeChildProcess();
    mocks.spawn.mockReturnValue(child);
    const { transferImageToRemoteHost } = await import("@/lib/remoteImagePaste");

    // When
    const resultPromise = transferImageToRemoteHost(SSH_CONFIG, Buffer.from("image-bytes"));
    await vi.waitFor(() => expect(mocks.spawn).toHaveBeenCalled());
    child.emit("close", 0);
    const result = await resultPromise;

    // Then
    expect(result).toBe("/tmp/kanvibe-paste-fixed-uuid.png");
    expect(mocks.writeFile).toHaveBeenCalledWith("/tmp/kanvibe-paste-xyz/fixed-uuid.png", Buffer.from("image-bytes"));
    expect(mocks.spawn).toHaveBeenCalledWith("scp", [
      "-i",
      "/tmp/test-key",
      "-P",
      "2202",
      "-o",
      "BatchMode=yes",
      "-o",
      "IdentitiesOnly=yes",
      "/tmp/kanvibe-paste-xyz/fixed-uuid.png",
      "app-prod:/tmp/kanvibe-paste-fixed-uuid.png",
    ], expect.anything());
    expect(mocks.rm).toHaveBeenCalledWith("/tmp/kanvibe-paste-xyz", { recursive: true, force: true });
  });

  it("rejects with stderr context on non-zero exit and still cleans up the local temp directory", async () => {
    // Given
    const child = new FakeChildProcess();
    mocks.spawn.mockReturnValue(child);
    const { transferImageToRemoteHost } = await import("@/lib/remoteImagePaste");

    // When
    const resultPromise = transferImageToRemoteHost(SSH_CONFIG, Buffer.from("image-bytes"));
    await vi.waitFor(() => expect(mocks.spawn).toHaveBeenCalled());
    child.stderr.emit("data", Buffer.from("Permission denied"));
    child.emit("close", 1);

    // Then
    await expect(resultPromise).rejects.toThrow(/Permission denied/);
    expect(mocks.rm).toHaveBeenCalledWith("/tmp/kanvibe-paste-xyz", { recursive: true, force: true });
  });

  it("rejects when the scp process itself fails to spawn and still cleans up", async () => {
    // Given
    const child = new FakeChildProcess();
    mocks.spawn.mockReturnValue(child);
    const { transferImageToRemoteHost } = await import("@/lib/remoteImagePaste");

    // When
    const resultPromise = transferImageToRemoteHost(SSH_CONFIG, Buffer.from("image-bytes"));
    await vi.waitFor(() => expect(mocks.spawn).toHaveBeenCalled());
    child.emit("error", new Error("ENOENT"));

    // Then
    await expect(resultPromise).rejects.toThrow("ENOENT");
    expect(mocks.rm).toHaveBeenCalledWith("/tmp/kanvibe-paste-xyz", { recursive: true, force: true });
  });
});
