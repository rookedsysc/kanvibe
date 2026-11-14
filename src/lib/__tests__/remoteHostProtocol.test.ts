// @vitest-environment node
import { describe, expect, it } from "vitest";
import { buildHostSocketPaths, createHostRecordDecoder, decodeHostBytes, encodeHostRecord, HOST_RECORD_BYTES, isValidHostNamespace } from "../remoteHostProtocol";

describe("host wire", () => {
  it("reassembles split headers and unicode payloads with adjacent records", () => {
    const records: unknown[] = [];
    const decode = createHostRecordDecoder((record) => records.push(record));
    const expected = [{ type: "message", text: "한글😀" }, { type: "status", id: "next" }];
    const bytes = Buffer.concat(expected.map(encodeHostRecord));
    for (const byte of bytes) decode(Buffer.from([byte]));
    expect(records).toEqual(expected);
  });
  it("rejects oversized lengths before receiving a payload", () => {
    const prefix = Buffer.alloc(4); prefix.writeUInt32BE(HOST_RECORD_BYTES + 1);
    expect(() => createHostRecordDecoder(() => undefined)(prefix)).toThrow(/size/i);
  });
  it("rejects non-record JSON and malformed UTF8", () => {
    const prefix = Buffer.alloc(4); prefix.writeUInt32BE(2);
    expect(() => createHostRecordDecoder(() => undefined)(Buffer.concat([prefix, Buffer.from("[]")]))).toThrow();
    prefix.writeUInt32BE(1);
    expect(() => createHostRecordDecoder(() => undefined)(Buffer.concat([prefix, Buffer.from([255])]))).toThrow();
  });
  it("accepts only canonical base64 and enforces byte bounds", () => {
    expect(decodeHostBytes(Buffer.from("한글").toString("base64")).toString()).toBe("한글");
    expect(() => decodeHostBytes("@@@=")).toThrow();
    expect(() => decodeHostBytes(Buffer.alloc(32769).toString("base64"))).toThrow();
  });
  it.each(["dev", "prod", "qa-fixture-42"])("recognizes explicit namespace %s", (namespace) => {
    expect(isValidHostNamespace(namespace)).toBe(true);
    expect(buildHostSocketPaths(namespace, 501).rpcSocketPath).toBe(`/tmp/kv-501/${namespace}/rpc-v1.sock`);
  });
  it.each(["../dev", "", "DEV", "qa-" + "x".repeat(40)])("rejects path/unknown namespace %s", (namespace) => {
    expect(isValidHostNamespace(namespace)).toBe(false);
    expect(() => buildHostSocketPaths(namespace, 501)).toThrow();
  });
});
