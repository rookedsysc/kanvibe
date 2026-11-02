import { createPrivateKey, sign } from "node:crypto";

const oauthUrl = "https://oauth2.googleapis.com/token";
const messagingScope = "https://www.googleapis.com/auth/firebase.messaging";

class FcmError extends Error {
  constructor(code = "UNAVAILABLE") {
    super("Push provider unavailable");
    this.code = code;
  }
}

function readServiceAccount(serviceAccount) {
  if (!serviceAccount || typeof serviceAccount.project_id !== "string" ||
      !/^[a-z][a-z0-9-]{4,61}[a-z0-9]$/.test(serviceAccount.project_id ?? "") ||
      typeof serviceAccount.client_email !== "string" ||
      !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(serviceAccount.client_email) ||
      serviceAccount.client_email.length > 254 ||
      typeof serviceAccount.private_key !== "string") {
    throw new Error("Invalid Firebase service account");
  }
  const privateKey = createPrivateKey(serviceAccount.private_key);
  if (privateKey.asymmetricKeyType !== "rsa" ||
      privateKey.asymmetricKeyDetails.modulusLength < 2048) {
    throw new Error("Invalid Firebase service account key");
  }
  const keyId = serviceAccount.private_key_id;
  if (keyId !== undefined && (typeof keyId !== "string" || keyId.length > 256)) {
    throw new Error("Invalid Firebase service account key ID");
  }
  return { projectId: serviceAccount.project_id, email: serviceAccount.client_email, privateKey, keyId };
}

function signAssertion(credentials, now) {
  const issuedAt = Math.floor(now / 1000);
  const header = { alg: "RS256", typ: "JWT" };
  if (credentials.keyId) header.kid = credentials.keyId;
  const claims = {
    iss: credentials.email,
    scope: messagingScope,
    aud: oauthUrl,
    iat: issuedAt,
    exp: issuedAt + 3600,
  };
  const unsigned = [header, claims].map((part) => Buffer.from(JSON.stringify(part)).toString("base64url")).join(".");
  const signature = sign("RSA-SHA256", Buffer.from(unsigned), credentials.privateKey).toString("base64url");
  return `${unsigned}.${signature}`;
}

async function readProviderResponse(response) {
  try {
    return await response.json();
  } catch {
    return null;
  }
}

function createFcmProvider({ serviceAccount, fetchImpl = fetch, now = Date.now, requestTimeoutMs = 10000 }) {
  const credentials = readServiceAccount(serviceAccount);
  const fcmUrl = `https://fcm.googleapis.com/v1/projects/${credentials.projectId}/messages:send`;
  let accessToken = null;
  let expiresAt = 0;
  let tokenRequest = null;

  async function requestAccessToken() {
    const response = await fetchImpl(oauthUrl, {
      method: "POST",
      headers: { "Content-Type": "application/x-www-form-urlencoded" },
      body: new URLSearchParams({
        grant_type: "urn:ietf:params:oauth:grant-type:jwt-bearer",
        assertion: signAssertion(credentials, now()),
      }).toString(),
      signal: AbortSignal.timeout(requestTimeoutMs),
      redirect: "error",
    });
    const result = await readProviderResponse(response);
    if (!response.ok || typeof result?.access_token !== "string" ||
        !result.access_token || result.access_token.length > 8192 ||
        /\s/.test(result.access_token) ||
        !Number.isFinite(result.expires_in) || result.expires_in <= 0 || result.expires_in > 86400) {
      throw new FcmError();
    }
    accessToken = result.access_token;
    expiresAt = now() + result.expires_in * 1000;
    return accessToken;
  }

  async function getAccessToken() {
    if (accessToken && now() < expiresAt - 60000) return accessToken;
    if (!tokenRequest) tokenRequest = requestAccessToken();
    try {
      return await tokenRequest;
    } finally {
      tokenRequest = null;
    }
  }

  async function send(message) {
    const token = await getAccessToken();
    const response = await fetchImpl(fcmUrl, {
      method: "POST",
      headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
      body: JSON.stringify({ message }),
      signal: AbortSignal.timeout(requestTimeoutMs),
      redirect: "error",
    });
    const result = await readProviderResponse(response);
    if (!response.ok) {
      if (response.status === 401) accessToken = null;
      const isUnregistered = response.status === 404 && Array.isArray(result?.error?.details) &&
        result.error.details.some((detail) =>
          detail?.["@type"] === "type.googleapis.com/google.firebase.fcm.v1.FcmError" &&
          detail.errorCode === "UNREGISTERED");
      throw new FcmError(isUnregistered ? "UNREGISTERED" : "UNAVAILABLE");
    }
    if (typeof result?.name !== "string" || !result.name) throw new FcmError();
  }

  return { send };
}

export { createFcmProvider, FcmError };
