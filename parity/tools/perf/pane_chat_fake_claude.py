#!/usr/bin/env python3
"""pane 채팅 지연 측정용 가짜 Claude Code. 실제 모델을 부르지 않고 Claude Code 2.1.332가 남기는 연결 신호만 흉내 낸다.

- 시작할 때 `$HOME/.claude/sessions/<pid>.json`(pid·sessionId·cwd·procStart·kind·pidDomain)을 쓴다. KanVibe는 이 기록과
  `/proc/<pid>/stat` 시작 표시를 대조해 pane의 대화를 잇는다.
- 대화 기록 `$HOME/.claude/projects/<cwd를 바꾼 이름>/<sessionId>.jsonl`은 첫 입력 때 만든다(새 대화는 기록이 없다).
- SIGUSR1은 `/clear`(새 대화 ID), SIGUSR2는 `--switch-to`로 받은 대화로의 `/resume`을 흉내 낸다.
- 신호를 쓴 시각(유닉스 epoch ns)은 `--events` 파일에 한 줄 JSON으로 남긴다. 측정 드라이버가 그 시각부터 잰다.
- `--trust-wait`이면 처음 쓰는 폴더의 신뢰 화면(2.1.332 화면과 같은 줄 순서)을 그리고 `trust_shown`을 남긴 뒤, 한 줄(Enter)을
  읽을 때까지 세션 기록을 쓰지 않는다. 실제 CLI도 신뢰를 수락한 뒤에야 기록을 쓴다.

파일 이름이 `claude`여야 KanVibe의 프로세스 판정이 Claude로 본다. 드라이버가 `<작업 폴더>/agent/claude`로 복사해 실행한다.
사용법: claude [--resume <id>] [--switch-to <id>] [--trust-wait] --events <파일>
"""
from __future__ import annotations

import argparse
import json
import os
import signal
import sys
import time
import uuid
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--resume")
parser.add_argument("--switch-to")
parser.add_argument("--events", required=True)
parser.add_argument("--trust-wait", action="store_true")
args = parser.parse_args()

home = Path(os.environ["HOME"])
cwd = os.getcwd()
pid = os.getpid()
record_path = home / ".claude" / "sessions" / f"{pid}.json"
project_dir = home / ".claude" / "projects" / cwd.replace("/", "-").replace("_", "-")
events = open(args.events, "a", buffering=1)


def process_start_marker() -> str:
    stat = Path(f"/proc/{pid}/stat").read_text()
    return stat[stat.rindex(") ") + 2:].split()[19]


def pid_domain() -> str:
    machine_id = Path("/etc/machine-id").read_text().strip()
    return f"linux:{machine_id}:{os.readlink('/proc/self/ns/pid')}"


def write_record(session_id: str, event: str) -> None:
    """실제 CLI처럼 임시 파일에 쓰고 이름을 바꿔, 읽는 쪽이 반쯤 쓴 기록을 보지 않게 한다."""
    record = {"pid": pid, "sessionId": session_id, "cwd": cwd, "startedAt": int(time.time() * 1000),
              "procStart": process_start_marker(), "version": "2.1.332", "kind": "interactive",
              "entrypoint": "cli", "pidDomain": pid_domain()}
    record_path.parent.mkdir(parents=True, exist_ok=True)
    staging = record_path.with_suffix(".tmp")
    staging.write_text(json.dumps(record))
    os.replace(staging, record_path)
    events.write(json.dumps({"event": event, "sessionId": session_id, "ns": time.time_ns()}) + "\n")


def append_turn(session_id: str, text: str) -> None:
    project_dir.mkdir(parents=True, exist_ok=True)
    with (project_dir / f"{session_id}.jsonl").open("a") as transcript:
        for role, content in (("user", text), ("assistant", f"echo: {text}")):
            transcript.write(json.dumps({"type": role, "sessionId": session_id, "cwd": cwd,
                                         "timestamp": "2026-01-15T03:00:00.000Z",
                                         "message": {"role": role, "content": content}}) + "\n")


session_id = args.resume or str(uuid.uuid4())


def clear(*_):
    global session_id
    session_id = str(uuid.uuid4())
    write_record(session_id, "clear")


def resume(*_):
    global session_id
    session_id = args.switch_to
    write_record(session_id, "resume")


TRUST_SCREEN = "\r\n".join([
    "", "─" * 80, " Accessing workspace:", "", f" {cwd}", "",
    " Quick safety check: Is this a project you created or one you trust? (Like your own code, a well-known open",
    " source project, or work from your team). If not, take a moment to review what's in this folder first.", "",
    " Claude Code'll be able to read, edit, and execute files here.", "", " Security guide", "",
    " ❯ No, exit", "   Yes, I trust this folder", "", " Enter to confirm · Esc to cancel", "",
])


def wait_for_trust() -> None:
    sys.stdout.write("\x1b[2J\x1b[H" + TRUST_SCREEN)
    sys.stdout.flush()
    events.write(json.dumps({"event": "trust_shown", "ns": time.time_ns()}) + "\n")
    sys.stdin.readline()


signal.signal(signal.SIGUSR1, clear)
signal.signal(signal.SIGUSR2, resume)
if args.trust_wait:
    wait_for_trust()
write_record(session_id, "start")
sys.stdout.write("\x1b[2J\x1b[H Claude Code (latency fake)\r\n\r\n Ready for a prompt\r\n\r\n> ")
sys.stdout.flush()
# 터미널은 줄 단위(cooked) 그대로 둔다. 신호 처리기가 끝나면 readline은 이어서 기다린다(PEP 475)
for line in sys.stdin:
    if line.strip():
        append_turn(session_id, line.strip())
    sys.stdout.write("> ")
    sys.stdout.flush()
