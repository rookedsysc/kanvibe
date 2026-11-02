import path from "path";
import { extractPlainText, makePreviewMessage, toIsoString } from "@/lib/aiSessions/shared";
import type { AggregatedAiMessage, AiMessageRole, AiSessionProvider } from "@/lib/aiSessions/types";

type RecordValue = Record<string, unknown>;
const MUSE_REPEATED_USER_WINDOW_MS = 60_000;
export interface AdditionalFileSession {
  id: string;
  cwd: string | null;
  title: string | null;
  startedAt: string | null;
  updatedAt: string | null;
  messages: AggregatedAiMessage[];
}
export function sessionRecord(value: unknown): RecordValue {
  return value !== null && typeof value === "object" && !Array.isArray(value) ? value as RecordValue : {};
}
export function sessionString(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}
function array(value: unknown): unknown[] { return Array.isArray(value) ? value : []; }
function firstString(record: RecordValue, keys: string[]): string | null {
  for (const key of keys) { const value = sessionString(record[key]); if (value) return value; }
  return null;
}
function decodeKiroBlocks(value: unknown): unknown[] {
  return array(value).flatMap((raw) => {
    const block = sessionRecord(raw);
    if (block.kind === "text") return [{ text: block.data }];
    if (block.kind === "json") return [{ text: JSON.stringify(block.data) }];
    if (block.kind === "toolResult") return decodeKiroBlocks(sessionRecord(block.data).content);
    if (block.kind === "toolUse") return [{ text: `${sessionRecord(block.data).name ?? "tool"}: ${JSON.stringify(sessionRecord(block.data).input ?? {})}` }];
    return [];
  });
}
function stripUserEnvelope(value: unknown): unknown {
  if (typeof value !== "string") return value;
  const match = value.match(/<USER_REQUEST>([\s\S]*?)(?:<\/USER_REQUEST>|$)/);
  return match ? match[1] : value;
}

export function decodeAdditionalFileSession(
  provider: AiSessionProvider,
  filePath: string,
  raw: unknown,
  sidecar: RecordValue = {},
): AdditionalFileSession {
  const records = Array.isArray(raw) ? raw.map(sessionRecord) : [sessionRecord(raw)];
  const document = sessionRecord(raw);
  const session: AdditionalFileSession = {
    id: path.basename(filePath, path.extname(filePath)), cwd: null, title: null,
    startedAt: null, updatedAt: null, messages: [],
  };
  const touch = (value: unknown) => {
    const time = toIsoString(value);
    if (!time) return;
    if (!session.startedAt || time < session.startedAt) session.startedAt = time;
    if (!session.updatedAt || time > session.updatedAt) session.updatedAt = time;
  };
  const add = (role: AiMessageRole, content: unknown, time?: unknown) => {
    const text = extractPlainText(content);
    const message = makePreviewMessage(role, time, text);
    touch(time);
    if (message) session.messages.push(message);
  };
  const addMessage = (rawMessage: unknown) => {
    const message = sessionRecord(rawMessage);
    const role = message.role;
    if (role === "user" || role === "assistant" || role === "tool" || role === "system") {
      add(role, message.content, message.timestamp ?? message.ts);
    }
  };
  switch (provider) {
    case "qoder":
      for (const record of records) {
        session.id = sessionString(record.sessionId) ?? session.id;
        session.cwd ??= sessionString(record.cwd);
        if (record.type === "workspace-directories") session.cwd ??= sessionString(array(record.directories)[0]);
        if (record.type === "summary") session.title = sessionString(record.summary) ?? session.title;
        if (record.type === "user" || record.type === "assistant") {
          add(record.type, sessionRecord(record.message).content, record.timestamp);
        }
      }
      break;
    case "codebuddy":
      for (const record of records) {
        session.id = sessionString(record.sessionId) ?? session.id;
        session.cwd ??= sessionString(record.cwd);
        if (record.type === "summary") session.title = sessionString(record.summary) ?? session.title;
        if (record.type === "ai-title") session.title = sessionString(record.aiTitle) ?? session.title;
        if (record.type === "message" && (record.role === "user" || record.role === "assistant")) {
          // skipRun은 실제로 보내지 않은 주입 문맥이므로 History 대화에서 제외한다.
          if (record.role === "user" && sessionRecord(record.providerData).skipRun === true) continue;
          const content = Array.isArray(record.content) ? record.content.filter((part) => ["text", "input_text", "output_text"].includes(String(sessionRecord(part).type))) : record.content;
          add(record.role, content, record.timestamp);
        }
      }
      break;
    case "pi": case "omp": case "prime-agent": case "openclaw":
      for (const record of records) {
        if (record.type === "session") {
          session.id = sessionString(record.id) ?? session.id;
          session.cwd ??= sessionString(record.cwd);
          touch(record.timestamp);
        }
        if (record.type === "title") session.title = sessionString(record.title) ?? session.title;
        if (record.type === "message") {
          const message = sessionRecord(record.message);
          if (message.role === "user" || message.role === "assistant" || message.role === "toolResult") {
            add(message.role === "toolResult" ? "tool" : message.role, message.content, record.timestamp);
          }
        }
      }
      break;
    case "copilot":
      for (const record of records) {
        const data = sessionRecord(record.data);
        if (record.type === "session.start") {
          session.id = sessionString(data.sessionId) ?? session.id;
          const context = sessionRecord(data.context);
          session.cwd ??= sessionString(context.cwd) ?? sessionString(context.gitRoot);
          touch(data.startTime);
        }
        if (record.type === "session.info" && !session.cwd) {
          const match = sessionString(data.message)?.match(/^Folder (.+) has been added to trusted folders\.$/);
          session.cwd = match?.[1]?.trim() ?? null;
        }
        if (record.type === "user.message") add("user", data.content ?? data.transformedContent, record.timestamp);
        if (record.type === "assistant.message") add("assistant", data.content, record.timestamp);
      }
      break;
    case "cursor":
      session.id = path.basename(path.dirname(filePath)) === "agent-transcripts" ? session.id : path.basename(path.dirname(filePath));
      session.cwd = sessionString(sidecar.cwd);
      session.title = sessionString(sidecar.title);
      touch(sidecar.createdAtMs); touch(sidecar.updatedAtMs);
      for (const record of records) if (record.role === "user" || record.role === "assistant") add(record.role, sessionRecord(record.message).content ?? record.content, record.timestamp);
      break;
    case "droid":
      for (const record of records) {
        if (record.type === "session_start" || record.type === "session" || record.type === "session_meta") {
          session.id = sessionString(record.id) ?? session.id;
          session.cwd ??= sessionString(record.cwd);
        }
        session.id = sessionString(record.session_id) ?? sessionString(record.sessionId) ?? session.id;
        session.cwd ??= sessionString(record.cwd);
        const message = sessionRecord(record.message);
        const role = record.role ?? message.role;
        if (role === "user" || role === "assistant") add(role, message.content ?? record.content, record.timestamp);
        if (record.type === "completion") add("assistant", record.finalText, record.timestamp);
      }
      break;
    case "hermes": case "jcode":
      session.id = firstString(document, provider === "hermes" ? ["session_id"] : ["id", "session_id"]) ?? session.id;
      session.cwd = firstString(document, provider === "hermes" ? ["cwd"] : ["working_dir", "working_directory"]);
      session.title = firstString(document, ["custom_title", "title"]);
      touch(document.session_start ?? document.created_at); touch(document.last_updated ?? document.updated_at);
      for (const message of array(document.messages)) {
        const value = sessionRecord(message);
        if (provider === "jcode" && (["system", "background_task"].includes(String(value.display_role)) || value.role === "system" || /^(?:\[Scheduled task\]|<system-reminder>)/.test(extractPlainText(value.content)))) continue;
        addMessage(value);
      }
      break;
    case "rovo":
      session.id = path.basename(path.dirname(filePath));
      session.cwd = firstString(document, ["workspace_path", "workspacePath", "workspace", "cwd", "working_directory", "workingDirectory", "project_path", "projectPath"]);
      session.title = firstString(document, ["title", "name", "summary"]);
      touch(document.created_at ?? document.createdAt); touch(document.updated_at ?? document.updatedAt);
      for (const message of array(sidecar.messages)) addMessage(message);
      for (const rawEntry of array(sidecar.message_history)) {
        const entry = sessionRecord(rawEntry);
        const role = entry.role ?? (entry.kind === "request" ? "user" : entry.kind === "response" ? "assistant" : null);
        if (role === "user" || role === "assistant") {
          const parts = array(entry.parts).filter((part) => {
            const kind = sessionRecord(part).part_kind;
            return kind === "text" || (role === "user" && kind === "user-prompt");
          }).map((part) => sessionRecord(part).content ?? sessionRecord(part).text);
          add(role, parts, entry.timestamp);
        }
      }
      break;
    case "grok":
      session.id = sessionString(sessionRecord(document.info).id) ?? path.basename(path.dirname(filePath));
      session.cwd = sessionString(sessionRecord(document.info).cwd);
      session.title = firstString(document, ["generated_title", "session_summary"]);
      touch(document.created_at); touch(document.updated_at ?? document.last_active_at);
      for (const value of array(sidecar.lines)) {
        const record = sessionRecord(value);
        if (record.type === "user" || record.type === "assistant") add(record.type, record.content, record.timestamp);
      }
      break;
    case "devin":
      session.id = firstString(document, ["session_id", "sessionId"]) ?? session.id;
      session.cwd = sessionString(document.working_directory) ?? sessionString(sidecar.working_directory);
      session.title = sessionString(sidecar.title);
      touch(sidecar.created_at); touch(sidecar.last_activity_at);
      for (const rawStep of array(document.steps)) {
        const step = sessionRecord(rawStep);
        const metadata = sessionRecord(step.metadata);
        if (step.source === "system") continue;
        const isUser = step.source === "user" || metadata.is_user_input === true;
        if (!isUser && step.source !== "agent" && step.role !== "assistant" && !step.tool_calls) continue;
        add(isUser ? "user" : "assistant", sessionRecord(step.message).content ?? step.message ?? step.text ?? step.content, step.timestamp ?? metadata.created_at);
      }
      break;
    case "cline":
      session.id = sessionString(document.session_id) ?? path.basename(path.dirname(filePath));
      session.cwd = firstString(document, ["cwd", "workspace_root"]);
      session.title = sessionString(document.prompt);
      touch(document.started_at); touch(sidecar.updated_at);
      for (const message of array(sidecar.messages)) addMessage(message);
      break;
    case "kiro":
      session.cwd = sessionString(document.cwd);
      session.title = sessionString(document.title);
      touch(document.created_at); touch(document.updated_at);
      for (const value of array(sidecar.lines)) {
        const record = sessionRecord(value);
        const data = sessionRecord(record.data);
        const role = record.kind === "Prompt" ? "user" : record.kind === "AssistantMessage" ? "assistant" : record.kind === "ToolResults" ? "tool" : null;
        if (role) add(role, decodeKiroBlocks(data.content), sessionRecord(data.meta).timestamp);
      }
      break;
    case "kimi": {
      session.id = path.basename(path.dirname(filePath));
      session.cwd = sessionString(sidecar.workDir);
      session.title = firstString(document, ["title", "lastPrompt"]);
      touch(document.createdAt); touch(document.updatedAt);
      let pending = "";
      const flush = () => { if (pending) add("assistant", pending); pending = ""; };
      for (const value of array(sidecar.lines)) {
        const record = sessionRecord(value);
        if (record.type === "context.append_message") {
          const message = sessionRecord(record.message);
          if (message.role === "user" && sessionRecord(message.origin).kind === "user") { flush(); add("user", message.content); }
        }
        if (record.type === "context.append_loop_event") {
          const event = sessionRecord(record.event);
          const part = sessionRecord(event.part);
          if (event.type === "content.part" && part.type === "text" && typeof part.text === "string") pending += part.text;
          if (event.type === "step.end") flush();
        }
      }
      flush();
      break;
    }
    case "antigravity":
      session.id = path.basename(path.dirname(path.dirname(path.dirname(filePath))));
      session.cwd = sessionString(sidecar.workspace);
      for (const record of records) {
        if ((record.source === "USER_EXPLICIT" || record.source === "USER") && (record.type === "USER_INPUT" || record.type === "REQUEST")) add("user", stripUserEnvelope(record.content), record.created_at);
        if (record.source === "MODEL" && record.type === "PLANNER_RESPONSE") add("assistant", record.content, record.created_at);
      }
      break;
    case "muse": {
      session.id = path.basename(path.dirname(filePath));
      let lastUserText: string | null = null;
      let lastUserTime: string | null = null;
      for (const envelope of records) {
        const entries = Array.isArray(envelope.children) ? envelope.children.flatMap((child) => {
          const value = sessionRecord(child).record_json;
          try { return [sessionRecord(typeof value === "string" ? JSON.parse(value) : value)]; } catch { return []; }
        }) : [envelope];
        for (const record of entries) {
          const payload = sessionRecord(record.payload);
          const value = sessionRecord(payload.record);
          const recordedAt = typeof record.recorded_at === "number" && record.recorded_at >= 1e14 ? Math.floor(record.recorded_at / 1000) : record.recorded_at;
          if (record.payload_type === "runtime.session.metadata") session.cwd ??= sessionString(value.workspace_root);
          if (record.payload_type === "runtime.session.route_facts") session.cwd ??= sessionString(value.cwd);
          if (record.payload_type === "runtime.user_intent.accepted") {
            const refill = array(payload.refill_blocks);
            const modelContent = array(payload.model_messages).flatMap((message) => array(sessionRecord(message).content));
            const text = extractPlainText((refill.length ? refill : modelContent).find((block) => sessionString(sessionRecord(block).text)));
            if (text) { add("user", text, recordedAt); lastUserText = text; lastUserTime = toIsoString(recordedAt); }
          }
          if (record.payload_type === "runtime.session") {
            const event = sessionRecord(payload.event);
            if (event.kind === "started" && typeof event.prompt === "string") {
              const time = toIsoString(recordedAt);
              // runtime.started는 accepted intent를 반복하므로 같은 분의 fallback만 제외한다.
              const repeated = lastUserText === event.prompt && lastUserTime && time && Math.abs(Date.parse(time) - Date.parse(lastUserTime)) < MUSE_REPEATED_USER_WINDOW_MS;
              if (!repeated) add("user", event.prompt, recordedAt);
            }
            if (event.kind === "assistant_message_committed") add("assistant", event.text, recordedAt);
          }
        }
      }
      break;
    }
  }
  return session;
}
