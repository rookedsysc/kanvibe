#!/usr/bin/env python3
"""격리된 PTY에서 채팅 브리지의 실제 입력·출력·선택기 경로를 검증한다."""
import codecs
import json
import os
from pathlib import Path
import select
import subprocess
import sys
import termios
import time
import tty


provider, session_id = sys.argv[1:3]
root = Path.cwd()
home = Path.home()
if provider == "codex":
    transcript = home / ".codex" / "sessions" / f"rollout-{session_id}.jsonl"
else:
    project = str(root).replace("/", "-").replace("_", "-")
    transcript = home / ".claude" / "projects" / project / f"{session_id}.jsonl"
transcript.parent.mkdir(parents=True, exist_ok=True)
recording = transcript.open("a", buffering=1)
audit = (root / f"{session_id}-input.jsonl").open("a", buffering=1)
model = 0
models = ["fixture-fast", "fixture-deep"]
# 모델 설정 선택창이 읽는 값. 실제 CLI처럼 기록에 남기고 슬래시 명령으로 바꾼다.
# Claude 쪽은 선택창의 고정 모델 목록과 맞도록 실제 모델 ID를 기록에 쓴다.
DEFAULT_EFFORT = "low" if provider == "codex" else "medium"
effort = DEFAULT_EFFORT
fast = False
claude_model = "claude-opus-5-5"
CLAUDE_MODEL_IDS = {"fable": "claude-fable-5-2", "opus": "claude-opus-5-5", "sonnet": "claude-sonnet-5-5", "haiku": "claude-haiku-5-5"}
picker = False
draft = ""
paste = False


def write_event(role, text):
    timestamp = time.strftime("%Y-%m-%dT%H:%M:%S.000Z", time.gmtime())
    if provider == "codex":
        event = {"type": "response_item", "timestamp": timestamp, "payload": {
            "type": "message", "role": role, "content": [{"type": "text", "text": text}]}}
    else:
        event = {"type": role, "sessionId": session_id, "cwd": str(root),
                 "timestamp": timestamp, "message": {"role": role, "content": text}}
        if role == "assistant":
            event["effort"] = effort
            event["requestedModel"] = claude_model
            event["message"]["model"] = claude_model
            event["message"]["usage"] = {"speed": "fast" if fast else "standard"}
    recording.write(json.dumps(event) + "\n")


def write_settings():
    """Codex는 턴 사이에 바뀐 설정을 이 이벤트로 남긴다."""
    recording.write(json.dumps({"type": "event_msg", "payload": {"type": "thread_settings_applied", "thread_settings": {
        "model": models[model], "reasoning_effort": effort, "service_tier": "priority" if fast else None}}}) + "\n")


def write_command_output(text):
    """Claude Code는 설정 명령의 결과를 이 모양의 user 기록으로 남긴다."""
    recording.write(json.dumps({"type": "user", "sessionId": session_id, "cwd": str(root),
                                "message": {"role": "user", "content": f"<local-command-stdout>{text}</local-command-stdout>"}}) + "\n")


def apply_setting_command(command):
    """모델 설정 명령이면 적용하고 True를 돌려준다. `/model`만 치면 선택기를 여는 예전 동작은 건드리지 않는다."""
    global model, effort, fast, claude_model
    words = command.split()
    if provider == "codex" and words[:1] == ["/model"] and len(words) > 1:
        for word in words[1:]:
            if word.startswith("--reasoning="):
                effort = word.split("=", 1)[1]
            elif word.startswith("--speed="):
                fast = word.split("=", 1)[1] == "priority"
            elif word in models:
                model = models.index(word)
                effort = DEFAULT_EFFORT
        write_settings()
        return True
    if provider == "claude" and len(words) == 2 and words[0] in ("/model", "/effort", "/fast"):
        if words[0] == "/model" and words[1] in CLAUDE_MODEL_IDS:
            claude_model = CLAUDE_MODEL_IDS[words[1]]
            write_command_output(f"Set model to `{words[1].capitalize()} 5.5` and saved as your default for new sessions")
        elif words[0] == "/effort":
            effort = DEFAULT_EFFORT if words[1] == "auto" else words[1]
            write_command_output(f"Set effort level to {effort} (saved as your default for new sessions)")
        elif words[0] == "/fast":
            fast = words[1] == "on"
        return True
    return False


if provider == "codex":
    recording.write(json.dumps({"type": "session_meta", "payload": {"id": session_id, "cwd": str(root)}}) + "\n")
    recording.write(json.dumps({"type": "turn_context", "payload": {"model": models[model], "effort": effort}}) + "\n")
else:
    recording.write(json.dumps({"type": "system", "sessionId": session_id, "cwd": str(root)}) + "\n")
    write_command_output("Set model to `Opus 5.5` and saved as your default for new sessions")
    write_command_output(f"Set effort level to {effort} (saved as your default for new sessions)")


def screen():
    sys.stdout.write("\x1b[H\x1b[2J")
    if picker:
        sys.stdout.write("Select model\r\n")
        for index, name in enumerate(models):
            sys.stdout.write(f"{'›' if index == model else ' '} {index + 1}. {name}\r\n")
        sys.stdout.write("↑ ↓ choose · Enter confirm · Esc cancel\r\n")
    else:
        marker = "❯" if provider == "claude" else "›"
        shown_model = models[model] if provider == "codex" else f"{models[model]} ({claude_model})"
        sys.stdout.write(f"{provider} · {session_id}\r\nmodel: {shown_model}\r\neffort: {effort} · speed: {'fast' if fast else 'standard'}\r\nReady for a prompt\r\n{marker} {draft}")
    sys.stdout.flush()


def notify_hook():
    hook = root / f".{provider}" / "hooks" / "kanvibe-prompt-hook.sh"
    if hook.exists():
        subprocess.run(["bash", str(hook)], input=json.dumps({
            "session_id": session_id, "transcript_path": str(transcript), "cwd": str(root),
            "hook_event_name": "UserPromptSubmit"}), text=True, timeout=15, check=False,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


old_settings = termios.tcgetattr(sys.stdin.fileno())
try:
    tty.setraw(sys.stdin.fileno())
    screen()
    pending = b""
    decoder = codecs.getincrementaldecoder("utf-8")()
    while True:
        if not select.select([sys.stdin], [], [], 30)[0]:
            continue
        chunk = os.read(sys.stdin.fileno(), 65536)
        if not chunk:
            break
        audit.write(json.dumps({"bytes": list(chunk)}) + "\n")
        pending += chunk
        while pending:
            sequences = {b"\x1b[200~": "paste-start", b"\x1b[201~": "paste-end",
                         b"\x1b[A": "up", b"\x1b[B": "down"}
            match = next(((key, value) for key, value in sequences.items() if pending.startswith(key)), None)
            if match:
                key, action = match
                pending = pending[len(key):]
                if action == "paste-start":
                    paste = True
                elif action == "paste-end":
                    paste = False
                elif picker:
                    model = (model + (1 if action == "down" else -1)) % len(models)
                    screen()
                continue
            if pending == b"\x1b" and not select.select([sys.stdin], [], [], 0.1)[0]:
                picker = False
                pending = b""
                screen()
                continue
            if any(key.startswith(pending) for key in sequences):
                break
            byte, pending = pending[:1], pending[1:]
            if byte == b"\x03":
                draft = ""
                sys.stdout.write("\r\nInterrupted\r\n")
                sys.stdout.flush()
            elif byte == b"\x1b":
                picker = False
                screen()
            elif byte == b"\r" and not paste:
                if picker:
                    picker = False
                    if provider == "codex":
                        write_settings()
                    screen()
                elif draft.strip() == "/model":
                    picker = True
                    draft = ""
                    screen()
                elif apply_setting_command(draft.strip()):
                    draft = ""
                    screen()
                elif draft.strip():
                    prompt = draft
                    draft = ""
                    write_event("user", prompt)
                    notify_hook()
                    reply = f"{session_id} received on {models[model]}: {prompt}"
                    sys.stdout.write(f"\r\n{reply}\r\n")
                    sys.stdout.flush()
                    write_event("assistant", reply)
                    screen()
            else:
                draft += decoder.decode(byte)
                screen()
finally:
    termios.tcsetattr(sys.stdin.fileno(), termios.TCSADRAIN, old_settings)
