const fs = require("node:fs/promises");
const path = require("node:path");
const selfsigned = require("selfsigned");

const CERTIFICATE_FILE_NAME = "certificate.pem";
const PRIVATE_KEY_FILE_NAME = "private-key.pem";
/** 앱은 인증서 체인이 아니라 지문으로 신뢰하므로, 만료로 재페어링이 강제되지 않도록 길게 잡는다 */
const CERTIFICATE_VALIDITY_MS = 20 * 365 * 24 * 60 * 60_000;

async function readExistingCertificate(certificatePath, privateKeyPath) {
  try {
    const [cert, key] = await Promise.all([
      fs.readFile(certificatePath, "utf8"),
      fs.readFile(privateKeyPath, "utf8"),
    ]);
    return { cert, key };
  } catch (error) {
    if (error.code === "ENOENT") return null;
    throw error;
  }
}

/**
 * 모바일 TLS 포트의 자체 서명 인증서를 읽거나 처음 한 번 만든다.
 * 지문이 바뀌면 모든 기기를 다시 페어링해야 하므로, 있는 파일은 절대 새로 만들지 않는다.
 */
async function loadOrCreateMobileTlsCertificate(directory) {
  const certificatePath = path.join(directory, CERTIFICATE_FILE_NAME);
  const privateKeyPath = path.join(directory, PRIVATE_KEY_FILE_NAME);
  const existing = await readExistingCertificate(certificatePath, privateKeyPath);
  if (existing) return existing;

  await fs.mkdir(directory, { recursive: true, mode: 0o700 });
  const now = Date.now();
  const generated = await selfsigned.generate([{ name: "commonName", value: "KanVibe Desktop" }], {
    keySize: 2048,
    algorithm: "sha256",
    notBeforeDate: new Date(now),
    notAfterDate: new Date(now + CERTIFICATE_VALIDITY_MS),
  });
  await fs.writeFile(privateKeyPath, generated.private, { mode: 0o600 });
  await fs.writeFile(certificatePath, generated.cert, { mode: 0o644 });
  return { cert: generated.cert, key: generated.private };
}

module.exports = { loadOrCreateMobileTlsCertificate };
