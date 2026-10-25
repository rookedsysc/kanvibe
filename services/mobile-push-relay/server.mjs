import { createHash, timingSafeEqual } from "node:crypto";
import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import { isIP } from "node:net";
import { dirname, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { createFcmProvider, FcmError } from "./fcm-provider.mjs";

const maxRequestBytes = 16 * 1024;
const fieldLimits = {
  type: 32,
  eventId: 128,
  connectionId: 128,
  taskId: 128,
  title: 512,
  body: 2048,
  status: 64,
  createdAt: 64,
};

class RequestError extends Error {
  constructor(status, code) {
    super(code);
    this.status = status;
    this.code = code;
  }
}

function hasExactKeys(value, keys) {
  return value !== null && typeof value === "object" && !Array.isArray(value) &&
    Object.keys(value).length === keys.length && keys.every((key) => Object.hasOwn(value, key));
}

function isBoundedString(value, maxBytes) {
  return typeof value === "string" && value.trim().length > 0 && Buffer.byteLength(value, "utf8") <= maxBytes;
}

function createMessage(payload) {
  if (!hasExactKeys(payload, ["token", "notification", "data"]) ||
      !isBoundedString(payload.token, 4096) || !/^[\x21-\x7e]+$/.test(payload.token) ||
      !hasExactKeys(payload.notification, ["title", "body"]) ||
      !hasExactKeys(payload.data, Object.keys(fieldLimits)) ||
      !Object.entries(fieldLimits).every(([key, limit]) => isBoundedString(payload.data[key], limit)) ||
      !isBoundedString(payload.notification.title, fieldLimits.title) ||
      !isBoundedString(payload.notification.body, fieldLimits.body) ||
      payload.data.type !== "task-status-changed" ||
      !/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,3})?Z$/.test(payload.data.createdAt) ||
      !Number.isFinite(Date.parse(payload.data.createdAt))) {
    throw new RequestError(400, "invalid_notification");
  }
  const collapseId = createHash("sha256")
    .update(JSON.stringify([payload.data.connectionId, payload.data.taskId])).digest("hex");
  const message = {
    token: payload.token,
    notification: payload.notification,
    data: payload.data,
    android: {
      priority: "HIGH",
      collapse_key: collapseId,
      notification: { channel_id: "kanvibe_task_status", tag: collapseId, sound: "default" },
    },
    apns: {
      headers: { "apns-push-type": "alert", "apns-priority": "10", "apns-collapse-id": collapseId },
      payload: { aps: { alert: payload.notification, sound: "default" } },
    },
  };
  const deliveryPayload = { ...message };
  delete deliveryPayload.token;
  if (Buffer.byteLength(JSON.stringify(deliveryPayload)) > 4096) {
    throw new RequestError(400, "invalid_notification");
  }
  return message;
}

function readPayload(request) {
  if (Number(request.headers["content-length"]) > maxRequestBytes) {
    throw new RequestError(413, "request_too_large");
  }
  if (!/^application\/json(?:\s*;|$)/i.test(request.headers["content-type"] ?? "")) {
    throw new RequestError(415, "json_required");
  }
  return new Promise((resolvePayload, reject) => {
    const chunks = [];
    let receivedBytes = 0;
    let isTooLarge = false;
    request.on("data", (chunk) => {
      receivedBytes += chunk.length;
      if (receivedBytes > maxRequestBytes) {
        isTooLarge = true;
        reject(new RequestError(413, "request_too_large"));
      } else if (!isTooLarge) {
        chunks.push(chunk);
      }
    });
    request.once("end", () => {
      if (isTooLarge) return;
      try {
        resolvePayload(JSON.parse(Buffer.concat(chunks).toString("utf8")));
      } catch {
        reject(new RequestError(400, "invalid_json"));
      }
    });
    request.once("error", () => reject(new RequestError(400, "invalid_request")));
    request.once("aborted", () => reject(new RequestError(400, "invalid_request")));
  });
}

function reply(response, status, body) {
  response.writeHead(status, { "Content-Type": "application/json", "Cache-Control": "no-store", Connection: "close" });
  response.end(JSON.stringify(body));
}

function validateRelayToken(relayToken) {
  if (typeof relayToken !== "string" || !/^[\x21-\x7e]{32,512}$/.test(relayToken)) {
    throw new Error("Relay token must contain 32 to 512 printable non-space ASCII characters");
  }
}

function createPushRelayServer({ relayToken, provider }) {
  validateRelayToken(relayToken);
  if (typeof provider?.send !== "function") throw new Error("Push provider is required");
  const expectedAuth = createHash("sha256").update(`Bearer ${relayToken}`).digest();
  return createServer({ requestTimeout: 15000, headersTimeout: 10000, maxHeaderSize: 8192 }, async (request, response) => {
    if (request.url !== "/notifications") return reply(response, 404, { error: "not_found" });
    if (request.method !== "POST") return reply(response, 405, { error: "method_not_allowed" });
    const suppliedAuth = createHash("sha256").update(request.headers.authorization ?? "").digest();
    if (!timingSafeEqual(expectedAuth, suppliedAuth)) return reply(response, 401, { error: "unauthorized" });
    try {
      const message = createMessage(await readPayload(request));
      await provider.send(message);
      reply(response, 200, { success: true });
    } catch (error) {
      if (error instanceof RequestError) return reply(response, error.status, { error: error.code });
      if (error instanceof FcmError && error.code === "UNREGISTERED") {
        return reply(response, 410, { error: "unregistered" });
      }
      reply(response, 502, { error: "push_unavailable" });
    }
  });
}

function loadRelayConfiguration(configPath) {
  const absolutePath = resolve(configPath);
  const config = JSON.parse(readFileSync(absolutePath, "utf8"));
  if (!hasExactKeys(config, ["host", "port", "relayToken", "serviceAccountPath"]) ||
      typeof config.host !== "string" || !isIP(config.host) ||
      !Number.isInteger(config.port) || config.port < 1 || config.port > 65535 ||
      !isBoundedString(config.serviceAccountPath, 4096)) {
    throw new Error("Invalid relay configuration");
  }
  validateRelayToken(config.relayToken);
  const accountPath = resolve(dirname(absolutePath), config.serviceAccountPath);
  return {
    host: config.host,
    port: config.port,
    relayToken: config.relayToken,
    serviceAccount: JSON.parse(readFileSync(accountPath, "utf8")),
  };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  if (process.argv.length !== 4 || process.argv[2] !== "--config") {
    console.error("Usage: node services/mobile-push-relay/server.mjs --config /absolute/path/relay.json");
    process.exitCode = 1;
  } else {
    try {
      const config = loadRelayConfiguration(process.argv[3]);
      const provider = createFcmProvider({ serviceAccount: config.serviceAccount });
      const server = createPushRelayServer({ relayToken: config.relayToken, provider });
      server.on("error", () => {
        console.error("Mobile push relay could not listen on the configured address.");
        process.exitCode = 1;
      });
      server.listen(config.port, config.host, () => console.log("KanVibe mobile push relay is listening."));
      const stop = () => server.close();
      process.once("SIGINT", stop);
      process.once("SIGTERM", stop);
    } catch {
      console.error("Mobile push relay could not start. Check the external configuration and service account.");
      process.exitCode = 1;
    }
  }
}

export { createPushRelayServer, loadRelayConfiguration };
