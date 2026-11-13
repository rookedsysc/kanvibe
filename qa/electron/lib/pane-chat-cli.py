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
    recording.write(json.dumps(event) + "\n")


if provider == "codex":
    recording.write(json.dumps({"type": "session_meta", "payload": {"id": session_id, "cwd": str(root)}}) + "\n")
else:
    recording.write(json.dumps({"type": "system", "sessionId": session_id, "cwd": str(root)}) + "\n")


def screen():
    sys.stdout.write("\x1b[H\x1b[2J")
    if picker:
        sys.stdout.write("Select model\r\n")
        for index, name in enumerate(models):
            sys.stdout.write(f"{'›' if index == model else ' '} {index + 1}. {name}\r\n")
        sys.stdout.write("↑ ↓ choose · Enter confirm · Esc cancel\r\n")
    else:
        marker = "❯" if provider == "claude" else "›"
        sys.stdout.write(f"{provider} · {session_id}\r\nmodel: {models[model]}\r\nReady for a prompt\r\n{marker} {draft}")
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
                    screen()
                elif draft.strip() == "/model":
                    picker = True
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
