#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../.." && pwd)"

usage() {
  cat >&2 <<USAGE
usage: $0 --run qa/parity/run-024 [--previous-run qa/parity/run-023]

Verifies that a real macOS Phase 5 run has PASS machine reports, completed
terminal and updater checklists, and non-empty S01-S14 screen/video evidence. With
--previous-run, also proves two consecutive runs used the same source commit
and scenario-definition digest.
USAGE
}

RUN_DIR=""
PREVIOUS_RUN_DIR=""
while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --run)
      RUN_DIR="${2:-}"
      shift 2
      ;;
    --previous-run)
      PREVIOUS_RUN_DIR="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage
      exit 64
      ;;
  esac
done

if [[ -z "$RUN_DIR" ]]; then
  usage
  exit 64
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "Phase 5 evidence verification requires python3 for strict JSON validation." >&2
  exit 69
fi

resolve_run_dir() {
  local candidate="$1"
  if [[ "$candidate" = /* ]]; then
    printf '%s\n' "$candidate"
  else
    printf '%s\n' "$REPO_ROOT/$candidate"
  fi
}

RUN_DIR="$(resolve_run_dir "$RUN_DIR")"
if [[ -n "$PREVIOUS_RUN_DIR" ]]; then
  PREVIOUS_RUN_DIR="$(resolve_run_dir "$PREVIOUS_RUN_DIR")"
fi

python3 - "$RUN_DIR" "$PREVIOUS_RUN_DIR" <<'PY'
import json
import pathlib
import re
import sys

run_dir = pathlib.Path(sys.argv[1]).resolve()
previous_dir = pathlib.Path(sys.argv[2]).resolve() if sys.argv[2] else None
errors = []

required_reports = {
    "qa-control-protocol.json": ("native-qa-control-protocol", "coveragePass"),
    "qa-control-replay-plan.json": ("native-qa-control-replay-plan", "coveragePass"),
    "qa-app-launch.json": ("native-app-launch-contract", "status"),
    "qa-app-replay.json": ("native-app-replay-contract", "status"),
    "native-visual-parity.json": ("native-visual-parity-evidence", "status"),
    "native-performance.json": ("native-performance-comparison", "status"),
    "full-parity.json": ("phase-5-full-parity", "status"),
}

def load_json(path):
    try:
        return json.loads(path.read_text())
    except FileNotFoundError:
        errors.append(f"missing file: {path}")
    except (OSError, json.JSONDecodeError) as error:
        errors.append(f"invalid JSON {path}: {error}")
    return {}

def validate_machine_reports(directory):
    if not directory.is_dir():
        errors.append(f"run directory does not exist: {directory}")
    for name, (artifact, pass_field) in required_reports.items():
        report = load_json(directory / name)
        if report.get("artifact") != artifact:
            errors.append(
                f"{directory.name}/{name}: expected artifact {artifact!r}"
            )
        expected = True if pass_field == "coveragePass" else "PASS"
        if report.get(pass_field) != expected:
            errors.append(
                f"{directory.name}/{name}: expected {pass_field}={expected!r}, "
                f"got {report.get(pass_field)!r}"
            )

def validate_checklist(directory):
    for label, name in (
        ("terminal", "terminal-runtime-checklist.md"),
        ("updater", "updater-runtime-checklist.md"),
    ):
        checklist_path = directory / name
        try:
            checklist = checklist_path.read_text()
            if "- [ ]" in checklist:
                errors.append(
                    f"{directory.name}: {label} checklist has unchecked items"
                )
            if not re.search(r"^Verdict:\s*`?PASS`?\s*$", checklist, re.MULTILINE):
                errors.append(
                    f"{directory.name}: {label} checklist verdict is not PASS"
                )
        except OSError as error:
            errors.append(
                f"{directory.name}: cannot read {label} checklist: {error}"
            )

def validate_manifest(directory):
    manifest = load_json(directory / "evidence-manifest.json")
    required_text = (
        "runId",
        "sourceCommit",
        "scenarioDefinitionDigest",
        "appVersion",
        "macosVersion",
        "hardware",
        "operator",
        "executedAt",
        "approvedBy",
    )
    for key in required_text:
        if not isinstance(manifest.get(key), str) or not manifest[key].strip():
            errors.append(f"{directory.name}: manifest {key} is empty")
    if manifest.get("runId") != directory.name:
        errors.append(
            f"{directory.name}: manifest runId must equal directory name"
        )
    if manifest.get("verdict") != "PASS":
        errors.append(f"{directory.name}: manifest verdict is not PASS")

    scenarios = manifest.get("scenarios", {})
    for number in range(1, 15):
        scenario_id = f"S{number:02d}"
        scenario = scenarios.get(scenario_id)
        if not isinstance(scenario, dict):
            errors.append(f"{directory.name}: missing manifest scenario {scenario_id}")
            continue
        for kind in ("screens", "videos"):
            paths = scenario.get(kind)
            if not isinstance(paths, list) or not paths:
                errors.append(
                    f"{directory.name}: {scenario_id} has no {kind} evidence"
                )
                continue
            for relative in paths:
                if not isinstance(relative, str) or not relative:
                    errors.append(
                        f"{directory.name}: {scenario_id} has invalid {kind} path"
                    )
                    continue
                candidate = (directory / relative).resolve()
                try:
                    candidate.relative_to(directory)
                except ValueError:
                    errors.append(
                        f"{directory.name}: evidence escapes run directory: {relative}"
                    )
                    continue
                if not candidate.is_file() or candidate.stat().st_size == 0:
                    errors.append(
                        f"{directory.name}: missing/empty evidence: {relative}"
                    )
    return manifest

validate_machine_reports(run_dir)
validate_checklist(run_dir)
manifest = validate_manifest(run_dir)
if previous_dir:
    validate_machine_reports(previous_dir)
    validate_checklist(previous_dir)
    previous_manifest = validate_manifest(previous_dir)
    for key in ("sourceCommit", "scenarioDefinitionDigest", "appVersion"):
        if manifest.get(key) != previous_manifest.get(key):
            errors.append(
                f"consecutive runs differ in {key}: "
                f"{previous_manifest.get(key)!r} != {manifest.get(key)!r}"
            )
    current_number = re.fullmatch(r"run-(\d+)", run_dir.name)
    previous_number = re.fullmatch(r"run-(\d+)", previous_dir.name)
    if not current_number or not previous_number:
        errors.append("consecutive verification requires run-<number> directories")
    elif int(current_number.group(1)) != int(previous_number.group(1)) + 1:
        errors.append("run directories are not consecutive")

if errors:
    for error in errors:
        print(f"FAIL: {error}", file=sys.stderr)
    raise SystemExit(1)

print(f"PASS: verified Phase 5 evidence in {run_dir}")
if previous_dir:
    print(f"PASS: consecutive with {previous_dir}")
PY
