import fs from "node:fs";
import path from "node:path";
import { randomUUID } from "node:crypto";
import { ensureKanvibeDataDirectory } from "@/lib/databasePaths";

const SESSION_FILE_NAME = "desktop-session.json";

function getSessionFilePath(): string {
  return path.join(ensureKanvibeDataDirectory(), SESSION_FILE_NAME);
}

export function createDesktopSession(): string {
  const token = randomUUID();
  fs.writeFileSync(getSessionFilePath(), `${JSON.stringify({ token, createdAt: Date.now() })}\n`, {
    mode: 0o600,
  });
  return token;
}

export function clearDesktopSession(): void {
  const sessionFilePath = getSessionFilePath();
  if (fs.existsSync(sessionFilePath)) {
    fs.unlinkSync(sessionFilePath);
  }
}

export function hasDesktopSession(): boolean {
  const sessionFilePath = getSessionFilePath();
  if (!fs.existsSync(sessionFilePath)) {
    return false;
  }

  try {
    const parsed = JSON.parse(fs.readFileSync(sessionFilePath, "utf8"));
    return typeof parsed?.token === "string" && parsed.token.length > 0;
  } catch {
    return false;
  }
}
