export const HOST_PROTOCOL_VERSION = 1;
export const HOST_RECORD_BYTES = 1024 * 1024;
export const HOST_CHUNK_BYTES = 32 * 1024;
export const HOST_BUFFER_BYTES = 8 * 1024 * 1024;

export interface HostRecord { type: string; id?: string; [key: string]: unknown }

export function isValidHostNamespace(namespace: unknown): namespace is string {
  return typeof namespace === "string" && /^(dev|prod|qa-[a-z0-9-]{1,24})$/.test(namespace);
}

export function buildHostSocketPaths(namespace: string, uid = process.getuid?.()): {
  directory: string; rpcSocketPath: string; hookSocketPath: string;
} {
  if (!isValidHostNamespace(namespace) || !Number.isSafeInteger(uid) || uid! < 0) throw new Error("Invalid host namespace or uid");
  const directory = `/tmp/kv-${uid}/${namespace}`;
  return { directory, rpcSocketPath: `${directory}/rpc-v1.sock`, hookSocketPath: `${directory}/hooks-v1.sock` };
}

export function encodeHostRecord(record: HostRecord): Buffer {
  const body = Buffer.from(JSON.stringify(record));
  if (!body.length || body.length > HOST_RECORD_BYTES) throw new Error("Host record size exceeded");
  const prefix = Buffer.allocUnsafe(4); prefix.writeUInt32BE(body.length);
  return Buffer.concat([prefix, body]);
}

export function createHostRecordDecoder(receive: (record: HostRecord) => void): (chunk: Buffer) => void {
  const prefix = Buffer.alloc(4);
  let prefixBytes = 0;
  let expected = 0;
  let received = 0;
  let body: Buffer | undefined;
  return (chunk) => {
    let offset = 0;
    while (offset < chunk.length) {
      if (prefixBytes < 4) {
        const count = Math.min(4 - prefixBytes, chunk.length - offset);
        chunk.copy(prefix, prefixBytes, offset, offset + count);
        prefixBytes += count; offset += count;
        if (prefixBytes < 4) continue;
        expected = prefix.readUInt32BE();
        if (!expected || expected > HOST_RECORD_BYTES) throw new Error("Host record size exceeded");
        body = Buffer.allocUnsafe(expected); received = 0;
      }
      const count = Math.min(expected - received, chunk.length - offset);
      chunk.copy(body!, received, offset, offset + count);
      received += count; offset += count;
      if (received !== expected) continue;
      const text = new TextDecoder("utf-8", { fatal: true }).decode(body);
      const record: unknown = JSON.parse(text);
      if (!record || typeof record !== "object" || Array.isArray(record) || typeof (record as HostRecord).type !== "string") throw new Error("Invalid host record");
      prefixBytes = 0; body = undefined;
      receive(record as HostRecord);
    }
  };
}

export function decodeHostBytes(encoded: unknown, maximum = HOST_CHUNK_BYTES): Buffer {
  if (typeof encoded !== "string" || encoded.length > Math.ceil(maximum / 3) * 4 || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(encoded)) throw new Error("Invalid host bytes");
  const bytes = Buffer.from(encoded, "base64");
  if (bytes.length > maximum || bytes.toString("base64") !== encoded) throw new Error("Invalid host bytes size");
  return bytes;
}
