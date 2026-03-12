import { access } from "fs/promises";
import { homedir } from "os";
import path from "path";
import { createReaderResult, createSessionDetail } from "@/lib/aiSessions/shared";
import type { AiSessionDetailReaderResult, AiSessionReaderContext, AiSessionReaderResult } from "@/lib/aiSessions/types";

const GEMINI_ROOT_DIR = path.join(homedir(), ".gemini");
const GEMINI_TMP_DIR = path.join(GEMINI_ROOT_DIR, "tmp");

export async function readGeminiSessions(context: AiSessionReaderContext): Promise<AiSessionReaderResult> {
  void context;
  const rootExists = await access(GEMINI_ROOT_DIR).then(() => true).catch(() => false);
  if (!rootExists) {
    return createReaderResult("gemini", { available: false, reason: "Gemini CLI directory not found" });
  }

  const tmpExists = await access(GEMINI_TMP_DIR).then(() => true).catch(() => false);
  if (!tmpExists) {
    return createReaderResult("gemini", { sessions: [], reason: "No local Gemini session files found" });
  }

  return createReaderResult("gemini", { sessions: [], reason: "Gemini session parsing is not available on this machine yet" });
}

export async function readGeminiSessionDetail(
  _context: AiSessionReaderContext,
  sessionId: string
): Promise<AiSessionDetailReaderResult> {
  return createSessionDetail({
    sessionId,
    provider: "gemini",
    messages: [],
    nextCursor: null,
  });
}
