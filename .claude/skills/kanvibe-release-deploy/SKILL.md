---
name: kanvibe-release-deploy
description: "Use this skill whenever releasing or deploying KanVibe desktop and mobile from a clean, up-to-date `dev` checkout: ask only for the target version and release-note approval, then let the AI update the Cargo workspace version, run `packaging/macos/build-macos.sh` and the Android APK/AAB and iOS IPA release builds, publish the DMG GitHub release, update the Homebrew cask, create the release PR, and auto-merge the release/promotion PRs when checks pass. Always use it for KanVibe release versions, DMG uploads, or Homebrew cask checksum updates."
---

# KanVibe Release Deploy

## Overview

This skill coordinates the KanVibe desktop and mobile release workflow from version selection to DMG publication, Homebrew cask update, release PR creation, and automatic merge. It is intentionally release-operator focused and may only be invoked from a clean, up-to-date local `dev` checkout: read and report the current Cargo workspace version (root `Cargo.toml` `[workspace.package]`), ask the user for the target release version, update the workspace version, run the `packaging/macos/build-macos.sh` DMG build, **launch the built app and confirm it actually starts**, build Android APK/AAB and iOS IPA artifacts from the same release source (step 3.6), get explicit approval for the release notes, upload the exact DMG artifact to the `rookedsysc/kanvibe` GitHub release, update the separate Homebrew cask repository with the same version and SHA-256, create the KanVibe release PR, then merge the release and promotion PRs automatically once checks and review-thread gates pass.

The smoke test in step 3.5 is not optional. Signing, notarization, stapling, and checksum verification all pass on a bundle whose app cannot start, and shipping one of those is how KanVibe 1.0.4 reached users.

The desktop app is a Rust + GPUI Cargo workspace (`crates/*`). The Electron/TypeScript desktop, the root `package.json`, electron-builder, and the `pnpm run deploy` script no longer exist; desktop packaging lives in `packaging/`. The mobile app (`mobile/`, Flutter) is unchanged.

The only routine user gates are target-version selection and release-note approval. After those are approved, do not ask for additional confirmation for build, release publication, cask update, PR creation, auto-merge enablement, or merge completion unless a blocker or branch-policy violation appears.

Before touching the cask, read `references/homebrew-cask-repository.md`. The cask repository location and cask file path live there so this skill stays portable if the tap checkout moves.

## When to Use

Use this skill when the user asks to:

- deploy or release KanVibe desktop;
- run `packaging/macos/build-macos.sh` for KanVibe release packaging;
- bump the KanVibe Cargo workspace version for a release;
- create release notes or a GitHub release that includes `dist/KanVibe-<version>.dmg`;
- create and merge the KanVibe release PR or pinned main-promotion PR after a desktop release;
- update the KanVibe Homebrew cask version or checksum;
- specifically release a version such as `1.0.2` where the expected DMG is `dist/KanVibe-1.0.2.dmg`.

Do not use this skill for docs-site deploys, Linux-only package checks, routine feature PRs that do not publish a desktop release, or any checkout that is not clean, up-to-date `dev`.

## Release Invariants

- Every release includes the desktop DMG, Android release APK/AAB, and signed iOS IPA builds. Complete all builds before publication; a missing toolchain, signing identity, or artifact is a blocker, not an implicit platform skip. Mobile store uploads and mobile GitHub asset publication require an explicit user request; this workflow builds and reports mobile artifacts locally.
- Keep the mobile marketing version equal to the target desktop version and increment the committed mobile build number once per release. Build retries reuse that number. All artifacts must use the same release source; source changes after a build require rebuilding affected artifacts before tagging.

- The GitHub release tag is the raw package version, for example `1.0.2`, not `v1.0.2`.
- The desktop version has one source: `version` in the root `Cargo.toml` `[workspace.package]` table. Every crate inherits it (`version.workspace = true`), the app reads it through `env!("CARGO_PKG_VERSION")`, and `packaging/macos/build-macos.sh` reads it from `cargo metadata` (package `kanvibe-app`) and writes it into `Info.plist` (`CFBundleShortVersionString` and `CFBundleVersion` from `packaging/macos/Info.plist.in`). The workspace members' entries in `Cargo.lock` carry the same version and must be refreshed with it.
- The DMG filename is `KanVibe-<version>.dmg` because `packaging/macos/build-macos.sh` writes `dist/KanVibe-$version.dmg`.
- The cask URL expects the same raw version tag: `https://github.com/rookedsysc/kanvibe/releases/download/#{version}/KanVibe-#{version}.dmg`.
- Use the SHA-256 of the final DMG produced after the version bump and `packaging/macos/build-macos.sh`, not an earlier artifact.
- `packaging/macos/build-macos.sh` (no arguments) is the release build command for this workflow. It builds the release `kanvibe` binary for the host architecture, builds the four remote-agent binaries through `packaging/build-remote-agents.sh`, assembles `dist/mac/KanVibe.app`, signs it with Hardened Runtime, creates and signs the DMG, notarizes and staples it, and prints the SHA-256.
- `packaging/macos/build-macos.sh` performs macOS signing/notarization and exits on any other OS, so it must run on macOS with Apple signing tools configured. Do not fabricate build, notarization, or checksum output from Linux.
- **Never publish a DMG built with `--unsigned`.** That flag skips signing, notarization, and stapling; it exists only for local checks without credentials.
- **A signed, notarized, correctly-checksummed DMG can still be an app that cannot start.** Signing and notarization say nothing about whether the bundle contains what it needs or whether the binary starts. Never publish without launching the built app (step 3.5).
- The release `Cargo.lock` must already match the bumped `Cargo.toml` before the build. `build-macos.sh` runs `cargo build` without `--locked`, so a stale lock is silently rewritten during the build and the built source no longer matches the release commit.
- Invoke this skill only from a clean, up-to-date local `dev` checkout. Stop on `main`, feature branches, existing release branches, detached HEADs, dirty worktrees, or a local `dev` HEAD that differs from `origin/dev`.
- The release source is always `origin/dev`. Do not accept, infer, or ask about alternate source branches for this workflow.
- The user's target-version selection and release-note approval authorize the remaining release actions. After those two gates, the AI should continue through release publication, cask update, release PR creation, auto-merge enablement, and pinned release-branch promotion to `main` without asking for another confirmation.
- Stop instead of auto-merging only when checks fail, required review threads remain unresolved, the PR is not mergeable, GitHub permissions are missing, the checkout is not clean/up-to-date `dev`, or the visible PR diff contains files outside the expected release bump scope.

## 1. Preflight and Version Selection

Work from the KanVibe application repository, not the Homebrew tap.

```bash
pwd
git fetch origin --prune
CURRENT_BRANCH=$(git branch --show-current)
if [ "$CURRENT_BRANCH" != "dev" ]; then
  echo "KanVibe release deploy must start from the local dev branch; current branch is ${CURRENT_BRANCH:-detached}." >&2
  exit 1
fi
if [ -n "$(git status --short)" ]; then
  echo "KanVibe release deploy must start from a clean dev worktree." >&2
  git status --short
  exit 1
fi
git pull --ff-only origin dev
if [ "$(git rev-parse HEAD)" != "$(git rev-parse origin/dev)" ]; then
  echo "Local dev must match origin/dev before cutting a release branch." >&2
  exit 1
fi
git status --short --branch

# Rust toolchain. There is no rust-toolchain file; CI builds with stable, and the workspace uses edition 2024.
cargo --version
rustc --version
CURRENT_VERSION=$(cargo metadata --no-deps --format-version 1 --manifest-path Cargo.toml \
  | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "kanvibe-app"))')
printf 'Current KanVibe version: %s\n' "$CURRENT_VERSION"

# packaging/build-remote-agents.sh needs all four rustup targets. On macOS the two Linux targets are
# cross-built with cargo-zigbuild; without it the build fails (the release must not pass --allow-missing).
INSTALLED_TARGETS=$(rustup target list --installed)
for RUST_TARGET in x86_64-unknown-linux-musl aarch64-unknown-linux-musl x86_64-apple-darwin aarch64-apple-darwin; do
  printf '%s\n' "$INSTALLED_TARGETS" | grep -qx "$RUST_TARGET" \
    || { echo "Missing rustup target: $RUST_TARGET (rustup target add $RUST_TARGET)" >&2; exit 1; }
done
command -v cargo-zigbuild >/dev/null || { echo "cargo-zigbuild is required to cross-build the Linux remote agents." >&2; exit 1; }

# build-macos.sh signing inputs. It reads CSC_NAME, APPLE_API_KEY, APPLE_API_KEY_ID, and APPLE_API_ISSUER from the
# environment or, when unset, from .env at the root of the checkout it runs in. Print key names only, never values.
for CREDENTIAL_KEY in CSC_NAME APPLE_API_KEY APPLE_API_KEY_ID APPLE_API_ISSUER; do
  if [ -n "$(printenv "$CREDENTIAL_KEY")" ] \
    || { [ -f .env ] && grep -qE "^[[:space:]]*(export[[:space:]]+)?${CREDENTIAL_KEY}[[:space:]]*=" .env; }; then
    printf 'credential present: %s\n' "$CREDENTIAL_KEY"
  else
    echo "Missing credential: $CREDENTIAL_KEY (environment or .env)" >&2
    exit 1
  fi
done
security find-identity -v -p codesigning
xcrun --find notarytool
xcrun --find stapler

gh auth status
gh release list --limit 5 --repo rookedsysc/kanvibe
```

`security find-identity -v -p codesigning` must list a `Developer ID Application` identity whose name exactly matches `CSC_NAME`; `build-macos.sh` refuses to start otherwise. `APPLE_API_KEY` is the path to the App Store Connect `.p8` key (a leading `~/` is expanded by the script). `.env` is gitignored, so it exists only in the checkout where the operator created it — the release worktree created below starts without one (see step 3).

If a toolchain item is missing, install it for this host and rerun preflight. Do not pass `--allow-missing` to `build-remote-agents.sh` or edit the packaging scripts to get past a missing target.

After printing the current version, ask the user which version to release before editing files:

```text
현재 KanVibe 버전은 <CURRENT_VERSION>입니다. 배포 버전을 몇으로 올릴까요? 예: 1.0.2
```

Only proceed after the user supplies the target version. If the original user prompt already supplied the exact target version, echo the current version and target version back to the user and proceed without asking again. This is the first of the two routine user gates; after the target version is selected, do not ask again until release-note review.

Validate these before proceeding:

1. The target version is a plain `x.y.z` value such as `1.0.2`; do not include a leading `v` or any prerelease/build suffix (for example `1.0.3-beta.1`). The desktop update checker (`parse_release_version` in `crates/kanvibe-domain/src/release_updates.rs`) accepts only an optional `v` plus three all-digit parts, so a prerelease tag would be invisible to in-app updates.
2. The target version is different from the current Cargo workspace version unless the user explicitly wants to rebuild the same version.
3. Work from a clean local `dev` checkout before creating the release branch. If the current branch is not `dev`, stop and tell the user to rerun the skill from `dev`; do not silently proceed from `main`, a feature branch, or an existing release branch.
4. Confirm local `dev` matches `origin/dev` after `git fetch`/`git pull --ff-only`. The release source is always `origin/dev`; do not ask about or honor alternate source branches in this workflow.
5. The active GitHub account can create releases in `rookedsysc/kanvibe`.
6. If the release build requires macOS signing tools, the host is macOS:

```bash
uname -s
```

If the required host/tooling is unavailable, stop and report the blocker. Do not fake `packaging/macos/build-macos.sh`, DMG, notarization, or checksum output.

After the target version is selected, create a dedicated release branch from the current `origin/dev` SHA. Do not use an existing release branch as the invocation point; reuse an existing release branch only after the `dev` preflight passes and only when it is the same release version with no unrelated files. This keeps the release PR narrow and prevents dirty local feature work from leaking into the release.

```bash
TARGET_VERSION="<version supplied by user>"
BASE_BRANCH="dev"
RELEASE_BRANCH="release/${TARGET_VERSION}"
RELEASE_WT="/home/rookedsysc/Documents/kanvibe/kanvibe__worktrees/release-${TARGET_VERSION}"

git fetch origin --prune
SOURCE_SHA=$(git rev-parse "origin/${BASE_BRANCH}")
git worktree add -b "$RELEASE_BRANCH" "$RELEASE_WT" "$SOURCE_SHA"
cd "$RELEASE_WT"
printf 'Release source SHA: %s\n' "$SOURCE_SHA"
git status --short --branch
```

If the release worktree path or branch already exists, re-read its status and PR state before reusing it. Reuse only when it is the same release version and contains no unrelated files.

## 2. Update the Cargo Workspace Version

After the user chooses the target version, update the root `Cargo.toml` `[workspace.package]` `version` and refresh `Cargo.lock` before running the build. Use a deterministic script instead of editing by hand.

The desktop version has exactly one place to edit. Every crate uses `version.workspace = true`, the app reads `env!("CARGO_PKG_VERSION")`, and `packaging/macos/build-macos.sh` takes the version from `cargo metadata` and substitutes it into `packaging/macos/Info.plist.in`, so none of those files change. `Cargo.lock` does change: it has one `version = "..."` line per workspace member, and `cargo update --workspace` rewrites exactly those entries without touching third-party packages.

The `Cargo.toml` replacement anchors on a **whole-line exact match** inside the `[workspace.package]` table. It never matches substrings and never looks outside that table, so a dependency entry or a future `[workspace.dependencies.<name>]` sub-table carrying the same version string cannot be rewritten. The npm-era bump script lost three releases to anchors that were looser than this; the rule stays.

```bash
TARGET_VERSION="<version supplied by user>"
if ! printf '%s\n' "$TARGET_VERSION" | grep -qxE '[0-9]+\.[0-9]+\.[0-9]+'; then
  echo "Invalid release version (expected plain x.y.z, no v/prerelease): $TARGET_VERSION" >&2
  exit 1
fi
CURRENT_VERSION=$(cargo metadata --no-deps --format-version 1 --manifest-path Cargo.toml \
  | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "kanvibe-app"))')
if [ "$CURRENT_VERSION" = "$TARGET_VERSION" ]; then
  echo "Cargo workspace is already at ${TARGET_VERSION}; pick a different target version" >&2
  exit 1
fi

python3 - "$CURRENT_VERSION" "$TARGET_VERSION" <<'PY'
import sys

current, target = sys.argv[1], sys.argv[2]
with open("Cargo.toml", encoding="utf-8") as manifest:
    lines = manifest.read().split("\n")

if lines.count("[workspace.package]") != 1:
    raise SystemExit("Cargo.toml: expected exactly one [workspace.package] table")
table_start = lines.index("[workspace.package]")
table_end = next((index for index in range(table_start + 1, len(lines)) if lines[index].startswith("[")), len(lines))
needle = f'version = "{current}"'
matches = [index for index in range(table_start + 1, table_end) if lines[index] == needle]
if len(matches) != 1:
    raise SystemExit(f"Cargo.toml [workspace.package]: expected exactly 1 line {needle!r}, got {len(matches)}")
lines[matches[0]] = f'version = "{target}"'

with open("Cargo.toml", "w", encoding="utf-8") as manifest:
    manifest.write("\n".join(lines))
PY

# Refresh only the workspace members' lock entries. On failure, restore both files so a retry starts clean;
# otherwise the "already at" guard above would refuse the rerun.
if ! cargo update --workspace; then
  git checkout -- Cargo.toml Cargo.lock
  exit 1
fi

cargo metadata --locked --format-version 1 >/dev/null
cargo metadata --no-deps --format-version 1 --manifest-path Cargo.toml \
  | python3 -c 'import json,sys; packages = json.load(sys.stdin)["packages"]; print(len(packages), sorted({p["version"] for p in packages}))'
git diff --numstat -- Cargo.toml Cargo.lock
git diff -U0 -- Cargo.lock | grep -E '^[-+]' | grep -vE '^(\+\+\+|---) ' \
  | grep -vxE "[-+]version = \"(${CURRENT_VERSION//./\\.}|${TARGET_VERSION//./\\.})\"" \
  && { echo "Cargo.lock changed more than workspace version lines" >&2; exit 1; }
git status --short
```

The bump is correct only when all four hold:

1. `cargo metadata --locked` succeeds, which proves `Cargo.lock` already matches the bumped manifest and the build will not rewrite it.
2. The `--no-deps` line prints the workspace member count `N` and a single version set equal to the target version, for example `16 ['1.6.1']`.
3. `git diff --numstat` reports `1 1 Cargo.toml` and `N N Cargo.lock` with the same `N`, and the last `grep` pipeline prints nothing: every changed `Cargo.lock` line is a workspace member's `version` line. Any other count means the edit escaped its anchor or `cargo update` resolved more than the workspace.
4. Nothing else appears in `git status` at this stage; step 3.6 subsequently changes only the `mobile/pubspec.yaml` version line.

Commit the version bump (`Cargo.toml` and `Cargo.lock`) with the release changes or ensure the release tag targets a commit that already contains the updated files.

## 3. Build the Versioned DMG

The build uses the active Rust toolchain on that Mac (CI uses stable; the workspace needs edition 2024 support) and the four rustup targets and `cargo-zigbuild` checked in preflight.

`build-macos.sh` reads the signing credentials from the environment or from `.env` at the root of the checkout it runs in, which is the release worktree. `.env` is gitignored, so a freshly created release worktree has none. Either place the operator's existing `.env` at the release worktree root (it stays untracked, and the release diff check below only looks at tracked files), or pass the four keys to this one command only. Do not export them into the interactive shell and never print their values.

Run the release build command after the version bump:

```bash
packaging/macos/build-macos.sh
```

The script runs, in order: `cargo build --release -p kanvibe-app --bin kanvibe` (host architecture only), `packaging/build-remote-agents.sh --out <tmp>/agents` (Linux-x86_64, Linux-aarch64, Darwin-x86_64, Darwin-arm64), `.app` assembly with `shasum -a 256 -c SHA256SUMS`, codesign of the Darwin agents and the app (`--options runtime`, `packaging/macos/entitlements.plist`), `codesign --verify --deep --strict --verbose=2`, `hdiutil create`, DMG codesign, `xcrun notarytool submit --wait`, `xcrun stapler staple`, `xcrun stapler validate`, and finally `[kanvibe] DMG sha256:` followed by the hash. Each step is echoed as `[kanvibe] $ <command>`; the script exits on the first failure.

Expected artifacts for version `1.0.2`:

```text
dist/KanVibe-1.0.2.dmg
dist/mac/KanVibe.app
```

For any version, derive and verify the artifact path from the Cargo workspace:

```bash
VERSION=$(cargo metadata --no-deps --format-version 1 --manifest-path Cargo.toml \
  | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "kanvibe-app"))')
DMG="dist/KanVibe-${VERSION}.dmg"
test -f "$DMG"
shasum -a 256 "$DMG"
git status --short
```

Keep the checksum from this final DMG for the cask update. If you rebuild for any reason, the checksum changes — always re-hash the artifact you are actually publishing. `git status --short` must still show only the step 2 version files; a `Cargo.lock` change here means step 2 did not refresh the lock.

Read these lines in the build output and keep them for the final report:

- `codesign --verify` reporting the app as valid on disk and satisfying its Designated Requirement;
- `notarytool submit --wait` ending with `status: Accepted`;
- `stapler validate` succeeding on the DMG;
- the `SHA256SUMS` listing printed by `build-remote-agents.sh`, which must name all four `kanvibe-remote-agent-<OS>-<arch>` files. The Darwin hashes in that listing are pre-signing; `build-macos.sh` rewrites `SHA256SUMS` inside the bundle after signing, so verify the bundle copy in step 3.5, not this listing.

The DMG contains one architecture: the host's. `cargo build` runs without `--target`, so a build on Apple Silicon ships an arm64 `kanvibe` only. Report the architecture from step 3.5 in the release notes.

### Notarization blocked by an Apple agreement

When `xcrun notarytool submit` inside `build-macos.sh` fails with `HTTP status code: 403. A required agreement is missing or has expired`, this is an Apple account state problem, not a build problem. Do not rebuild to retry — check the credential cheaply instead, supplying the same values the script read (environment or `.env`) to this command only:

```bash
xcrun notarytool history --key "$APPLE_API_KEY" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER"
```

If a previous release notarized successfully with the same key and team, the configuration is fine and only the agreement changed. The Account Holder must accept the pending agreement at developer.apple.com, and propagation to the notary service takes a few minutes. Poll the command above and rerun `packaging/macos/build-macos.sh` once it returns history.

When notarization returns a status other than `Accepted`, read Apple's verdict for that submission before changing anything:

```bash
xcrun notarytool log <submission-id> --key "$APPLE_API_KEY" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER"
```

## 3.5. Smoke-Test the Built App (hard gate)

**Do not publish a release without this step.** Code signing, notarization, stapling, and checksum verification all pass on a bundle whose app cannot start. KanVibe 1.0.4 shipped exactly that way: the DMG was signed, notarized, stapled, and its checksum matched, but the app died on launch. The Rust bundle has no `app.asar`, but the same class of failure remains: a binary that panics at startup, a database migration that fails on an empty data directory, a hook server that never binds, or missing remote-agent binaries all survive signing.

The checks mirror `parity/tools/app-launch/check_release_launch.py`, the Linux release-launch check used by `packaging/linux/build-linux.sh --verify` (Xvfb/xdotool, Linux only). On the release Mac, mount the DMG you are about to publish, copy the app out, check its contents, launch it against a throwaway data directory and port, and read that run's own diagnostics log:

```bash
VERSION=$(cargo metadata --no-deps --format-version 1 --manifest-path Cargo.toml \
  | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "kanvibe-app"))')
DMG="dist/KanVibe-${VERSION}.dmg"
SMOKE_ROOT=$(mktemp -d /tmp/kanvibe-smoke-XXXXXX)
SMOKE_MOUNT="$SMOKE_ROOT/mount"
SMOKE_APP="$SMOKE_ROOT/KanVibe.app"
SMOKE_DATA="$SMOKE_ROOT/appdata"
LOG="$SMOKE_DATA/logs/kanvibe-desktop.log"
# The installed app owns 9736 (hook) and 9746 (mobile TLS = hook + 10). Use a free pair.
SMOKE_PORT=39736
if lsof -nP -iTCP:"$SMOKE_PORT" -sTCP:LISTEN || lsof -nP -iTCP:"$((SMOKE_PORT + 10))" -sTCP:LISTEN; then
  echo "smoke port $SMOKE_PORT or $((SMOKE_PORT + 10)) is in use; pick another SMOKE_PORT" >&2
  exit 1
fi

xcrun stapler validate "$DMG"
mkdir "$SMOKE_MOUNT"
hdiutil attach "$DMG" -nobrowse -readonly -mountpoint "$SMOKE_MOUNT" -quiet
spctl -a -t exec -vv "$SMOKE_MOUNT/KanVibe.app"
cp -R "$SMOKE_MOUNT/KanVibe.app" "$SMOKE_APP"
hdiutil detach "$SMOKE_MOUNT" -quiet

# Bundle contents: version, architecture, signature, and the four remote agents with matching hashes.
/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$SMOKE_APP/Contents/Info.plist"
file "$SMOKE_APP/Contents/MacOS/kanvibe"
codesign --verify --deep --strict --verbose=2 "$SMOKE_APP"
ls "$SMOKE_APP/Contents/Resources/remote-agents"
(cd "$SMOKE_APP/Contents/Resources/remote-agents" && shasum -a 256 -c SHA256SUMS)

open -n -a "$SMOKE_APP" --stdout "$SMOKE_ROOT/app-output.log" --stderr "$SMOKE_ROOT/app-output.log" \
  --args --data-dir "$SMOKE_DATA" --kanvibe-hook-port "$SMOKE_PORT"
sleep 15

pgrep -f "$SMOKE_APP/Contents/MacOS/kanvibe" >/dev/null || { echo "app exited during startup" >&2; exit 1; }
curl -s -o /dev/null -w 'hook health: %{http_code}\n' "http://127.0.0.1:${SMOKE_PORT}/api/hooks/health"
sqlite3 "$SMOKE_DATA/kanvibe.db" "SELECT COUNT(*) FROM migrations"
sqlite3 "$SMOKE_DATA/kanvibe.db" \
  "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('projects', 'kanban_tasks', 'app_settings', 'migrations') ORDER BY name"
wc -l "$LOG"
grep -cF 'main:startup' "$LOG"
grep -cF 'main:hook-server-listening' "$LOG"
grep -cE 'main:uncaught-exception|main:hook-server-failed|main:development-database-failed' "$LOG"

# SIGTERM takes the app's normal quit path (server, remote agents, PTYs are shut down).
pkill -TERM -f "$SMOKE_APP/Contents/MacOS/kanvibe"
for _ in $(seq 20); do
  pgrep -f "$SMOKE_APP/Contents/MacOS/kanvibe" >/dev/null || break
  sleep 1
done
pgrep -f "$SMOKE_APP/Contents/MacOS/kanvibe" >/dev/null && { echo "app did not exit on SIGTERM" >&2; exit 1; }
curl -s -o /dev/null -w 'hook health after quit: %{http_code}\n' "http://127.0.0.1:${SMOKE_PORT}/api/hooks/health"

rm -rf "$SMOKE_ROOT"
```

The release may proceed only when all of these hold:

1. `stapler validate` succeeds on the DMG, and `spctl -a -t exec` reports `accepted` and `source=Notarized Developer ID` for the app.
2. `CFBundleShortVersionString` prints the target version, `file` reports the expected architecture, `codesign --verify` passes, `remote-agents/` holds `kanvibe-remote-agent-Linux-x86_64`, `-Linux-aarch64`, `-Darwin-x86_64`, `-Darwin-arm64` plus `SHA256SUMS`, and `shasum -c` reports `OK` for all four.
3. The process is still alive after the sleep.
4. `hook health` prints `200`, which proves the in-process hook server bound the smoke port.
5. The `migrations` count is at least 17 (the same floor `check_release_launch.py` uses) and all four core tables are listed, which proves the empty data directory was migrated.
6. `main:startup` and `main:hook-server-listening` counts are each greater than zero, and the error-event count is `0`.
7. The app exits within 20 seconds of SIGTERM and `hook health after quit` prints `000`.

Read the log from `$LOG` inside `$SMOKE_DATA`, never from `~/Library/Application Support/kanvibe`. Do not delete the user's log to get a clean read. If the smoke test fails, keep `$SMOKE_ROOT` (do not run the final `rm -rf`) so `app-output.log` and the diagnostics log stay available for diagnosis.

### Why the smoke run is isolated

The release binary resolves its data directory in `DesktopLaunchPlan::resolve` (`crates/kanvibe-app/src/desktop_process.rs`) through `resolve_app_data_directory` (`crates/kanvibe-server/src/launch.rs`): a `--data-dir` argument wins over the installed location, and the database (`<data-dir>/kanvibe.db`), the mobile TLS certificate (`<data-dir>/mobile-tls/`), and the diagnostics log (`<data-dir>/logs/kanvibe-desktop.log`, from `resolve_desktop_log_path`) all follow it. `--kanvibe-hook-port` overrides the packaged default `9736` through `resolve_hook_server_port`. Without both arguments, the smoke copy would open the user's real `~/Library/Application Support/kanvibe` database and collide with the installed app on `9736`.

The Rust app does **not** read `KANVIBE_APP_DATA_DIR`; the Electron-era `open --env KANVIBE_APP_DATA_DIR=...` launch would silently run against the user's real data. Pass `--data-dir` through `open --args`, which reaches the launched app only and does not touch the shell, so this stays inside the repository's runtime-environment safety rules. `open -n` is required: without it macOS just focuses an already-running instance instead of starting the copy under test.

If the smoke test fails, stop. Do not publish, do not update the cask, and do not open PRs. Diagnose from `$SMOKE_ROOT/app-output.log` (stdout/stderr, including the `[kanvibe] Desktop diagnostics log:` line and any panic message) and `$LOG` (`main:uncaught-exception` carries the panic text). If `remote-agents/` is incomplete, rerun `packaging/build-remote-agents.sh --out <dir>` on the release Mac and read which platform it reports under `missing platforms:`.

## 3.6 Build Android and iOS Release Artifacts

This step is required before publishing the DMG. Read `mobile/README.md`, `mobile/.fvmrc`, `mobile/pubspec.yaml`, Android signing configuration, and the iOS Runner signing settings first. Use the FVM-pinned Flutter SDK from `mobile/`, not a global Flutter installation.

### Mobile preflight and version

- Run `fvm flutter --version`, `fvm flutter doctor -v`, and `xcodebuild -version` from `mobile/`. Resolve toolchain requirements against the checked-in mobile documentation and build configuration, including the Android SDK and selected Xcode. Do not change global SDK selections or signing settings to bypass a failure.
- Before invoking raw Gradle, run `fvm flutter pub get`, then `fvm flutter build apk --release --config-only` from `mobile/`. The latter prepares Android build inputs and the ignored Gradle wrapper without producing an APK. Require `.dart_tool/package_config.json`, `.flutter-plugins-dependencies`, `android/local.properties`, executable `android/gradlew`, and `android/gradle/wrapper/gradle-wrapper.jar`; verify `flutter.sdk` points to the FVM-pinned SDK and `sdk.dir` to the available Android SDK before continuing. Keep these generated/local files uncommitted. Stop on preparation failure or unexpected tracked changes.
- Require `mobile/android/key.properties` and its referenced keystore. Run `./gradlew :app:signingReport` from `mobile/android/` and verify that `Variant: release` uses `Config: release`. The Gradle configuration otherwise falls back to a debug key; `--release` alone does not prove distribution signing.
- Confirm an iOS distribution identity, provisioning profile, team, and export configuration are available. Stop if signed IPA export cannot run; simulator builds, `--no-codesign`, and an archive without an IPA do not satisfy this release gate.
- Use the platform-specific Firebase local configuration described in `mobile/README.md` when present. If it is absent, report that background push is unavailable and build without that platform’s `--dart-define-from-file` argument, as supported by the mobile README. Never print config contents or commit local configs, signing keys, or credentials.
- Read and report the existing `mobile/pubspec.yaml` version. Change only its single top-level `version:` line to `<TARGET_VERSION>+<previous build number + 1>` before building. Preserve other YAML content. Include that file in the release commit. On a retry, reuse the already selected version/build number rather than incrementing again. If distribution history shows that number has already been used, stop and obtain a valid higher number before building.

Run these commands from `mobile/` with fail-fast behavior (`set -e`); retain each command's exit status and log. Run the two platform builds sequentially because they share Flutter build state.

```bash
set -e
fvm flutter pub get
fvm flutter analyze
fvm flutter test

test -f android/key.properties
KANVIBE_ANDROID_BUILD_ARGS=()
KANVIBE_IOS_BUILD_ARGS=()
if [ -f config/firebase.android.local.json ]; then
  KANVIBE_ANDROID_BUILD_ARGS+=(--dart-define-from-file=config/firebase.android.local.json)
fi
if [ -f config/firebase.ios.local.json ]; then
  KANVIBE_IOS_BUILD_ARGS+=(--dart-define-from-file=config/firebase.ios.local.json)
fi

fvm flutter build apk --release "${KANVIBE_ANDROID_BUILD_ARGS[@]}"
fvm flutter build appbundle --release "${KANVIBE_ANDROID_BUILD_ARGS[@]}"
fvm flutter build ipa --release "${KANVIBE_IOS_BUILD_ARGS[@]}"

test -s build/app/outputs/flutter-apk/app-release.apk
test -s build/app/outputs/bundle/release/app-release.aab
shasum -a 256 build/app/outputs/flutter-apk/app-release.apk
shasum -a 256 build/app/outputs/bundle/release/app-release.aab
```

Use the existing iOS export settings; if the project needs an explicit export-options plist, pass the operator's existing file with `--export-options-plist=<path>` and record that path without exposing credentials. Do not invent a signing team, generate keys, or upload to TestFlight/App Store/Google Play as part of building.

After the commands succeed:

1. Identify the IPA produced by this run under `mobile/build/ios/ipa/`; require exactly one nonempty IPA, record its actual filename and SHA-256, and verify the archive under `mobile/build/ios/archive/`. Do not use a wildcard that could silently pick a stale IPA. Preserve or move previous outputs before starting if they would make attribution ambiguous.
2. Inspect the generated Android and iOS artifact metadata with the installed SDK tools. Confirm the target marketing version, build number, application identifiers, and distribution signing. Record the actual exported iOS build number if export changes it; do not assume an automatic increment occurred.
3. Check `git status --short`. The only new tracked release change allowed here is `mobile/pubspec.yaml`; unexpected lockfile, generated project, or signing changes are blockers to investigate, not files to silently stage.
4. Report Android APK/AAB and iOS archive/IPA paths, hashes, build numbers, signing verification, and command exit statuses. A successful build is build evidence; do not claim device smoke testing or push delivery without a real device run.

Return to the repository root before step 4. Keep the existing desktop smoke test in step 3.5 mandatory. Include mobile build results and local artifact locations in release-note review and the release PR Test Plan; mobile binaries stay local unless the user explicitly requests publication.

## 4. Create or Update the GitHub Release

Draft release notes in a temporary markdown file. Match the existing release-note tone: concise sections such as `## New Features`, `## Improvements and Stability`, and `## Packaging Note`. Include a packaging note naming the exact DMG artifact, for example:

```markdown
## Packaging Note

- The version was updated from `1.0.1` to `1.0.2` before building.
- The DMG artifact is `KanVibe-1.0.2.dmg`.
- The app is built for Apple Silicon (arm64).
```

State the architecture that `file` reported in step 3.5; the DMG contains only the build host's architecture.

### Review release notes with the user before publishing

Publishing a GitHub release is outward-facing, so the release notes must be reviewed and approved by the user before any `gh release create` or `gh release edit` runs. This is the second and final routine user gate. After approval, continue through commit/push, release publication, cask update, release PR creation, and auto-merge without asking for more confirmation unless a blocker appears.

1. Keep the original English notes in the temporary markdown file (`$RELEASE_NOTES`).
2. Present both versions to the user in the same message:
   - the original English release notes, exactly as they will be published;
   - a Korean translation of the same notes, clearly labeled as a translation for review only.
3. Ask the user to approve or request changes. Treat this as a hard gate: do not proceed to create or edit the release until the user explicitly approves.
4. If the user requests edits, update `$RELEASE_NOTES` with the corrected English text, re-translate, and present both versions again. Repeat until approved.
5. Only the approved English notes file is published; the Korean translation is for the user's review and is not uploaded unless the user explicitly asks for bilingual release notes.
6. Once approved, do not ask separately about creating the release PR or merging it; those actions are part of the authorized release automation.

Create the release with the DMG asset only after the user approves the notes. Use the raw version tag and upload the generated DMG, not a directory or renamed copy.

After release-note approval, commit and push the release branch before deriving the tag target. The target SHA must already exist in GitHub, and the visible release PR diff must stay limited to the version bump files. If extra files appear, stop and report the blocker instead of tagging or merging.

```bash
VERSION=$(cargo metadata --no-deps --format-version 1 --manifest-path Cargo.toml \
  | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "kanvibe-app"))')
DMG="dist/KanVibe-${VERSION}.dmg"
RELEASE_NOTES="/tmp/kanvibe-release-${VERSION}.md"
BASE_BRANCH="dev"
RELEASE_BRANCH="${RELEASE_BRANCH:-release/${VERSION}}"

# Verify the release branch diff is only the expected version bump against the required dev base.
git fetch origin --prune
git diff --name-status "origin/${BASE_BRANCH}"
UNEXPECTED_FILES=$(git diff --name-only "origin/${BASE_BRANCH}" | grep -vxE 'Cargo\.toml|Cargo\.lock|mobile/pubspec\.yaml' || true)
if [ -n "$UNEXPECTED_FILES" ]; then
  echo "Unexpected release PR files:" >&2
  printf '%s\n' "$UNEXPECTED_FILES" >&2
  exit 1
fi

git add Cargo.toml Cargo.lock mobile/pubspec.yaml
if ! git diff --cached --quiet; then
  git commit -m "chore(release): publish KanVibe ${VERSION}"
fi
git push -u origin "$RELEASE_BRANCH"

# Fail fast if the committed release branch does not contain the selected version.
if ! git diff --quiet -- Cargo.toml Cargo.lock mobile/pubspec.yaml || ! git diff --cached --quiet -- Cargo.toml Cargo.lock mobile/pubspec.yaml; then
  echo "Release version files have uncommitted changes; commit the version bump before tagging the release." >&2
  exit 1
fi
# Step 2 already proved this is the only such line in the [workspace.package] table.
if ! git show HEAD:Cargo.toml | grep -qx "version = \"${VERSION}\""; then
  echo "HEAD Cargo.toml does not carry workspace version ${VERSION}; commit the bump onto the release commit first." >&2
  exit 1
fi
TARGET_SHA=$(git rev-parse HEAD)

gh release create "$VERSION" "$DMG" \
  --repo rookedsysc/kanvibe \
  --target "$TARGET_SHA" \
  --title "KanVibe ${VERSION} Release Notes" \
  --notes-file "$RELEASE_NOTES" \
  --latest
```

If the release already exists, do not create a duplicate. Update the notes/title if needed and upload the DMG with `--clobber`:

```bash
gh release edit "$VERSION" \
  --repo rookedsysc/kanvibe \
  --title "KanVibe ${VERSION} Release Notes" \
  --notes-file "$RELEASE_NOTES"

gh release upload "$VERSION" "$DMG" \
  --repo rookedsysc/kanvibe \
  --clobber
```

Verify the release and asset URL:

```bash
gh release view "$VERSION" --repo rookedsysc/kanvibe --json tagName,name,assets,url
curl -I -L "https://github.com/rookedsysc/kanvibe/releases/download/${VERSION}/KanVibe-${VERSION}.dmg"
```

## 5. Update the Homebrew Cask Tap

Read `references/homebrew-cask-repository.md` first and use its local repository path and cask file path.

Compute the checksum from the exact DMG uploaded to the release:

```bash
VERSION=$(cargo metadata --no-deps --format-version 1 --manifest-path Cargo.toml \
  | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "kanvibe-app"))')
DMG="dist/KanVibe-${VERSION}.dmg"
SHA256=$(shasum -a 256 "$DMG" | cut -d ' ' -f 1)
printf '%s\n' "$SHA256"
```

In the Homebrew cask repository:

1. Fetch and fast-forward `main`.
2. Update only the `version` and `sha256` lines in `Casks/kanvibe.rb`.
3. Keep the URL shape unchanged: it must continue to use `#{version}` and `KanVibe-#{version}.dmg`.
4. Validate the Ruby syntax and, when Homebrew is available, audit/fetch the cask.
5. Commit and push the cask update.

Command outline:

```bash
CASK_REPO="<read from references/homebrew-cask-repository.md>"
CASK_FILE="$CASK_REPO/Casks/kanvibe.rb"

git -C "$CASK_REPO" status --short --branch
git -C "$CASK_REPO" pull --ff-only origin main
ruby -c "$CASK_FILE"
# Use file tools or a small script to replace version and sha256 with $VERSION and $SHA256.
ruby -c "$CASK_FILE"

git -C "$CASK_REPO" diff -- Casks/kanvibe.rb
git -C "$CASK_REPO" add Casks/kanvibe.rb
git -C "$CASK_REPO" commit -m "Update KanVibe cask to ${VERSION}"
git -C "$CASK_REPO" push origin main
```

Homebrew rejects casks that live outside a tap, so `brew fetch --cask Casks/kanvibe.rb` against the local checkout fails with `Homebrew requires casks to be in a tap`. Verify in two stages instead.

Before pushing, check the syntax and prove the checksum against the bytes GitHub actually serves:

```bash
ruby -c "$CASK_FILE"
curl -sL "https://github.com/rookedsysc/kanvibe/releases/download/${VERSION}/KanVibe-${VERSION}.dmg" | shasum -a 256
```

That hash must equal the `sha256` written into the cask. After pushing, verify through the real tap name:

```bash
brew update
brew fetch --cask rookedsysc/kanvibe/kanvibe   # expect: ✔︎ Cask kanvibe (<version>)
```

## 6. Create and Auto-Merge the Release PRs

After the GitHub release and Homebrew cask are published, the release commit must land on `main` so `main` reflects the shipped version. The target-version selection and release-note approval are sufficient authorization for the remaining PR work. Do not ask for another merge confirmation; instead, create the PRs, verify the gates, and merge or enable auto-merge automatically.

Stop and report a blocker only if checks fail, a PR has active unresolved review threads, GitHub says the PR is not mergeable, required permissions are missing, or the PR file list is outside the expected release scope.

### 6.1 Release branch to `dev`

Create or reuse the release PR from `release/<version>` to `dev`. Write a concise English PR body with Summary, Test Plan, GitHub release URL, DMG asset path, SHA-256, and Homebrew cask commit/status.

```bash
VERSION=$(cargo metadata --no-deps --format-version 1 --manifest-path Cargo.toml \
  | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "kanvibe-app"))')
RELEASE_BRANCH="${RELEASE_BRANCH:-release/${VERSION}}"
BASE_BRANCH="dev"
RELEASE_PR_BODY="/tmp/kanvibe-release-pr-${VERSION}.md"

# Use file tools to write $RELEASE_PR_BODY before running gh pr create.
RELEASE_PR=$(gh pr list --repo rookedsysc/kanvibe --base "$BASE_BRANCH" --head "$RELEASE_BRANCH" --state open --json number --jq '.[0].number // empty')
if [ -z "$RELEASE_PR" ]; then
  gh pr create --repo rookedsysc/kanvibe --base "$BASE_BRANCH" --head "$RELEASE_BRANCH" \
    --title "chore(release): publish KanVibe ${VERSION}" \
    --body-file "$RELEASE_PR_BODY" \
    --assignee @me
  RELEASE_PR=$(gh pr list --repo rookedsysc/kanvibe --base "$BASE_BRANCH" --head "$RELEASE_BRANCH" --state open --json number --jq '.[0].number')
fi

RELEASE_BASE=$(gh pr view "$RELEASE_PR" --repo rookedsysc/kanvibe --json baseRefName --jq .baseRefName)
test "$RELEASE_BASE" = "dev"
gh pr view "$RELEASE_PR" --repo rookedsysc/kanvibe --json url,baseRefName,headRefName,headRefOid,mergeable,assignees,files,statusCheckRollup
RELEASE_HEAD_SHA=$(gh pr view "$RELEASE_PR" --repo rookedsysc/kanvibe --json headRefOid --jq .headRefOid)
printf 'Release PR head SHA: %s\n' "$RELEASE_HEAD_SHA"
```

Verify the release PR file list before merging. It should contain only `Cargo.toml` (the `[workspace.package]` version line), `Cargo.lock` (workspace member version lines), and the single version-line change in `mobile/pubspec.yaml`. If a release-specific file is intentionally added, name it in the PR body and final handoff; otherwise stop.

This file-scope check applies **only to the release PR into `dev`**. The promotion PR into `main` legitimately carries every commit that `main` is behind by, so a large diff there is expected and is not a blocker.

```bash
gh pr view "$RELEASE_PR" --repo rookedsysc/kanvibe --json files --jq '.files[].path'
gh pr checks "$RELEASE_PR" --repo rookedsysc/kanvibe --watch
```

Check active review threads before merging:

```bash
UNRESOLVED_THREADS=$(gh api graphql \
  -f owner=rookedsysc -f name=kanvibe -F number="$RELEASE_PR" \
  -f query='query($owner:String!, $name:String!, $number:Int!) { repository(owner:$owner, name:$name) { pullRequest(number:$number) { reviewThreads(first:100) { nodes { isResolved isOutdated } } } } }' \
  --jq '[.data.repository.pullRequest.reviewThreads.nodes[] | select(.isResolved == false and .isOutdated == false)] | length')
test "$UNRESOLVED_THREADS" = "0"
```

Merge automatically when checks are green. If checks are still pending but the PR is otherwise mergeable, enable auto-merge and keep watching until the PR becomes `MERGED` before starting the promotion PR. Never enable auto-merge after a failed, cancelled, timed-out, or action-required check. Auto-merge on this repository can land the PR before the pending checks finish; see the note below.

```bash
CHECK_STATE=$(gh pr checks "$RELEASE_PR" --repo rookedsysc/kanvibe --json state --jq '
  if any(.[]; .state == "FAILURE" or .state == "CANCELLED" or .state == "TIMED_OUT" or .state == "ACTION_REQUIRED") then "failure"
  elif all(.[]; .state == "SUCCESS" or .state == "SKIPPED") then "success"
  else "pending" end
')
case "$CHECK_STATE" in
  success)
    gh pr merge "$RELEASE_PR" --repo rookedsysc/kanvibe --merge
    ;;
  pending)
    gh pr merge "$RELEASE_PR" --repo rookedsysc/kanvibe --auto --merge
    gh pr checks "$RELEASE_PR" --repo rookedsysc/kanvibe --watch
    ;;
  failure)
    echo "Release PR checks failed; do not enable auto-merge." >&2
    exit 1
    ;;
esac

gh pr view "$RELEASE_PR" --repo rookedsysc/kanvibe --json state,mergeCommit,mergedBy,headRefName
```

#### A pending check does not hold the merge

`dev` has no required status checks:

```bash
gh api repos/rookedsysc/kanvibe/branches/dev/protection --jq '.required_status_checks.contexts'
```

This returns an empty list, which means `Type Check & Test` is **not** a merge gate. `gh pr merge --auto` has nothing to wait for, so a PR whose checks read `pending` can merge immediately. Do not read a pending line in `gh pr checks` as "waiting to merge"; it only says the workflow has not finished yet. In the 1.2.0 release, PR #359 merged while this check was still `in_progress` — that followed the procedure, but the merge landed ahead of the CI conclusion.

Because the merge does not wait, confirm the release commit's CI conclusion separately after merging:

```bash
gh run list --repo rookedsysc/kanvibe --commit "$RELEASE_HEAD_SHA" --json databaseId,name,status,conclusion
gh run view <databaseId> --repo rookedsysc/kanvibe
```

`RELEASE_HEAD_SHA` (captured in §6.1) is the right commit to query, because it is the commit the DMG was built from. The merge commit on `dev` is a different SHA with its own runs; it answers "is `dev` green", not "did the released code pass".

Every run for that commit must end with `conclusion: success`. A failed conclusion after a completed merge is a blocker: report it instead of continuing quietly.

Delete release branches only after that conclusion exists. Deleting a branch cancels workflow runs still in progress on it, which destroys the evidence this step depends on.

### 6.2 Promote the pinned release branch to `main`

After the release PR is merged into `dev`, promote the same pinned release branch SHA to `main`; do not use the moving `dev` branch as the promotion PR head. This keeps `main` aligned with the exact `TARGET_SHA` used for the DMG release instead of accidentally merging commits that landed on `dev` after the release branch was cut.

```bash
# RELEASE_HEAD_SHA was captured from the release PR before merge. Recreate the remote branch if the repository auto-deleted it after the dev merge.
git fetch origin main dev --prune
PROMOTION_BRANCH="${PROMOTION_BRANCH:-$RELEASE_BRANCH}"
if ! git ls-remote --exit-code --heads origin "$PROMOTION_BRANCH" >/dev/null 2>&1; then
  git push origin "${RELEASE_HEAD_SHA}:refs/heads/${PROMOTION_BRANCH}"
fi

PROMOTION_PR=$(gh pr list --repo rookedsysc/kanvibe --base main --head "$PROMOTION_BRANCH" --state open --json number --jq '.[0].number // empty')
if [ -z "$PROMOTION_PR" ]; then
  gh pr create --repo rookedsysc/kanvibe --base main --head "$PROMOTION_BRANCH" \
    --title "Release: promote KanVibe ${VERSION} to main" \
    --body "Promote the pinned ${VERSION} release branch to main after the published DMG release and Homebrew cask update." \
    --assignee @me
  PROMOTION_PR=$(gh pr list --repo rookedsysc/kanvibe --base main --head "$PROMOTION_BRANCH" --state open --json number --jq '.[0].number')
fi

PROMOTION_HEAD_SHA=$(gh pr view "$PROMOTION_PR" --repo rookedsysc/kanvibe --json headRefOid --jq .headRefOid)
test "$PROMOTION_HEAD_SHA" = "$RELEASE_HEAD_SHA"
gh pr view "$PROMOTION_PR" --repo rookedsysc/kanvibe --json url,baseRefName,headRefName,headRefOid,mergeable,assignees,statusCheckRollup

UNRESOLVED_THREADS=$(gh api graphql \
  -f owner=rookedsysc -f name=kanvibe -F number="$PROMOTION_PR" \
  -f query='query($owner:String!, $name:String!, $number:Int!) { repository(owner:$owner, name:$name) { pullRequest(number:$number) { reviewThreads(first:100) { nodes { isResolved isOutdated } } } } }' \
  --jq '[.data.repository.pullRequest.reviewThreads.nodes[] | select(.isResolved == false and .isOutdated == false)] | length')
test "$UNRESOLVED_THREADS" = "0"

CHECK_STATE=$(gh pr checks "$PROMOTION_PR" --repo rookedsysc/kanvibe --json state --jq '
  if any(.[]; .state == "FAILURE" or .state == "CANCELLED" or .state == "TIMED_OUT" or .state == "ACTION_REQUIRED") then "failure"
  elif all(.[]; .state == "SUCCESS" or .state == "SKIPPED") then "success"
  else "pending" end
')
case "$CHECK_STATE" in
  success)
    gh pr merge "$PROMOTION_PR" --repo rookedsysc/kanvibe --merge
    ;;
  pending)
    gh pr merge "$PROMOTION_PR" --repo rookedsysc/kanvibe --auto --merge
    gh pr checks "$PROMOTION_PR" --repo rookedsysc/kanvibe --watch
    ;;
  failure)
    echo "Promotion PR checks failed; do not enable auto-merge." >&2
    exit 1
    ;;
esac

PROMOTION_STATE=$(gh pr view "$PROMOTION_PR" --repo rookedsysc/kanvibe --json state,mergeCommit,mergedBy --jq .state)
printf 'Promotion PR state: %s\n' "$PROMOTION_STATE"

git fetch origin main
git show origin/main:Cargo.toml | grep -qx "version = \"${VERSION}\""

# Clean up the release branch only after the main promotion is merged and the release commit's
# workflow runs have reached a conclusion. Deleting the branch cancels runs still in progress,
# so the pending-run count below is the gate, not just something to read.
if [ "$PROMOTION_STATE" = "MERGED" ]; then
  gh run list --repo rookedsysc/kanvibe --commit "$RELEASE_HEAD_SHA" --json name,status,conclusion
  PENDING_RUNS=$(gh run list --repo rookedsysc/kanvibe --commit "$RELEASE_HEAD_SHA" \
    --json status --jq '[.[] | select(.status != "completed")] | length')
  if [ "$PENDING_RUNS" != "0" ]; then
    echo "release commit runs still in progress ($PENDING_RUNS); skip branch cleanup" >&2
  else
    git push origin --delete "$PROMOTION_BRANCH" || true
  fi
fi
```

## 7. Final Verification

Before reporting success, collect real output for:

- dev-only preflight evidence: current branch `dev`, clean status, and local `HEAD` matching `origin/dev` before the release branch was cut;
- `cargo --version`/`rustc --version`, the four installed rustup targets, `cargo-zigbuild` presence, the four credential keys reported present (names only), and the matching `security find-identity` identity;
- current version printed before the bump and the user-selected target version;
- `cargo metadata --locked` succeeding after the bump, the workspace member count with a single target version, and `git diff --numstat -- Cargo.toml Cargo.lock` showing `1 1 Cargo.toml` and `N N Cargo.lock` with only workspace `version` lines changed;
- `packaging/macos/build-macos.sh` completion, including `codesign --verify` passing, `notarytool` `status: Accepted`, `stapler validate` succeeding, and the four remote agents in the `SHA256SUMS` listing;
- **smoke-test evidence from step 3.5**: `stapler`/`spctl` verdicts, bundle version and architecture, remote-agent `shasum -c` result, process alive after launch, hook health `200`, migration count and core tables, `main:startup`/`main:hook-server-listening` counts with error-event count `0`, and the clean SIGTERM exit;
- `test -f dist/KanVibe-<version>.dmg` and `shasum -a 256`;
- **mobile build evidence from step 3.6**: FVM/toolchain versions, `analyze`/`test`/APK/AAB/IPA command exit statuses, Android release signing configuration, exported iOS signing/version metadata, and nonempty APK/AAB/IPA paths with SHA-256;
- `mobile/pubspec.yaml` diff containing only the target version and incremented build number, committed in the same release source as the desktop bump;
- user approval of the release notes, confirming both the English original and the Korean translation were shown before publishing;
- `gh release view <version>` showing the `KanVibe-<version>.dmg` asset;
- `curl -I -L` against the release asset URL returning an HTTP success/redirect chain rather than a 404;
- Homebrew cask diff showing only `version` and `sha256` changes, plus the cask repository push result or exact blocker;
- release branch commit SHA and pushed branch name;
- release PR URL, file list, check result, unresolved review-thread count, and merge or auto-merge result into `dev`;
- the release commit's workflow conclusion from `gh run list --commit <sha>`, collected after the merge because a pending check does not hold it;
- promotion PR URL, check result, unresolved review-thread count, and merge or auto-merge result into `main`;
- `git show origin/main:Cargo.toml` confirming `main` now carries the release workspace version, or the exact blocker if auto-merge is still pending.

Final handoff format:

```markdown
완료했습니다.

- Dev preflight: <branch/status/local HEAD == origin/dev evidence>
- Previous version: <current version reported before bump>
- Release version: <target version selected by user>
- DMG: `dist/KanVibe-<version>.dmg`
- SHA-256: `<sha256>`
- Android: <APK/AAB paths, version/build number, signing result, SHA-256>
- iOS: <archive/IPA paths, exported version/build number, signing result, SHA-256>
- Mobile build verification: <analyze/test/build command exit statuses; device QA only if run>
- Bundle check: <architecture / codesign verify / notarization status / remote agents 4 + SHA256SUMS OK>
- Smoke test: <app alive / hook health / migrations / startup+listening log counts / error events / SIGTERM exit>
- GitHub release: <release URL>
- Homebrew cask: <commit SHA or branch/status>
- Release PR: <URL> → <merged/auto-merge/blocker>
- Release commit CI: <workflow conclusion for the release commit>
- Promotion PR: <URL> → <merged/auto-merge/blocker>
- main version: <confirmed version or pending reason>
- Verification:
  - `<command>` → <real result>
  - `<command>` → <real result>
```

## Common Pitfalls

1. **Skipping the version question.** Always report the current Cargo workspace version and ask the user what version to release unless the prompt already contains the exact target version.
2. **Running a stale version build.** Update the `[workspace.package]` version and refresh `Cargo.lock` before `packaging/macos/build-macos.sh`, then derive the DMG path from the updated version.
3. **Running on Linux and pretending success.** `packaging/macos/build-macos.sh` requires macOS codesign, hdiutil, notarytool, and stapler and exits on any other OS, so stop honestly when the host is not Darwin. `packaging/linux/build-linux.sh` builds Linux tarball/.deb bundles, not the DMG this release publishes.
4. **Using a `v` tag.** The cask URL uses the raw version as the release tag. `v1.0.2` will break the current cask URL.
5. **Checksum from the wrong file.** Always hash the final DMG produced after the version bump and release build.
6. **Editing the cask before the release asset exists.** Homebrew fetch/audit can fail if the GitHub release asset is missing or still uploading.
7. **Leaking signing secrets.** Never print `.env` contents or Apple API key material in release notes, logs, commits, or Discord output.
8. **Dirty tap checkout.** Do not overwrite unrelated cask repository edits. Inspect status and either preserve them or ask the user before continuing.
9. **Publishing unreviewed release notes.** Always show the user the English original and a Korean translation and get explicit approval before `gh release create`/`gh release edit`.
10. **Asking for extra merge confirmation after approval.** Once the target version and release notes are approved, create the release PR, enable auto-merge or merge when green, and promote the pinned release branch to `main` without another routine confirmation.
11. **Auto-merging through blockers.** Automatic merge is allowed only after checks pass or auto-merge is enabled from a pending state, active unresolved review threads are zero, GitHub reports the PR mergeable, and the PR file list matches the expected release scope.
12. **Promoting moving `dev`.** Do not create the `main` promotion PR with `--head dev`; use the pinned release branch/SHA so commits that land on `dev` after the DMG build cannot ride along into `main`.
13. **Starting from a non-`dev` checkout.** This workflow is dev-only at invocation. Stop instead of switching branches, inferring a different source, or running directly from `main`, feature branches, release branches, or detached HEADs.
14. **Publishing an `--unsigned` build.** `packaging/macos/build-macos.sh --unsigned` produces a DMG with the same name but no signature, notarization, or stapled ticket. It is for local checks only; the release build takes no arguments.
15. **Treating signing and notarization as proof the app works.** They verify provenance, not completeness. KanVibe 1.0.4 was signed, notarized, stapled, and checksum-matched, and still could not start. Step 3.5 is the only check that catches this class of failure.
16. **Leaving `Cargo.lock` stale after the bump.** `build-macos.sh` runs `cargo build` without `--locked`, so the build rewrites a stale lock instead of failing. The DMG is then built from a tree that differs from the release commit, and an uncommitted `Cargo.lock` shows up after the build. Run `cargo update --workspace` in step 2 and prove it with `cargo metadata --locked`.
17. **Getting past a missing remote agent with `--allow-missing`.** `build-remote-agents.sh` fails unless all four platforms build, and `build-macos.sh` calls it without `--allow-missing` on purpose. The two Linux agents need the musl rustup targets and `cargo-zigbuild` on the Mac; install them instead of weakening the build. A bundle without an agent cannot drive remote hosts of that platform.
18. **Rebuilding to retry an Apple agreement 403.** The failure is account state, not build state. Re-check with `xcrun notarytool history` and, if an earlier release succeeded with the same key, wait for propagation rather than spending build cycles.
19. **Launching the app by executing its binary directly.** Use `open -n -a <app> --args ...`. Running `KanVibe.app/Contents/MacOS/kanvibe` as a child of the terminal makes macOS attribute the app's file-access prompt to the terminal process; a denial there revokes `~/Documents` access for the whole process tree and blocks the rest of the release.
20. **Blocking on the promotion PR's diff size.** The file-scope guard belongs to the release PR into `dev`. The promotion PR into `main` carries every commit `main` is behind by, so a wide diff there is normal.
21. **Misreading DMG verification signals.** `build-macos.sh` notarizes and staples the DMG only, so the app inside reports "does not have a ticket stapled" even for a correct release. Judge with `xcrun stapler validate <dmg>` and `spctl -a -t exec` on the app, as step 3.5 does.
22. **Shipping only the DMG after a mobile build fails.** Android APK/AAB and signed iOS IPA builds are required before publication. Missing signing configuration is a blocker; debug-signed Android output, unsigned iOS output, and stale artifacts are not successful release builds.
23. **Smoke-testing with the Electron-era launch.** The Rust app ignores `KANVIBE_APP_DATA_DIR`, so `open --env KANVIBE_APP_DATA_DIR=...` runs the candidate against the user's real `~/Library/Application Support/kanvibe` data and the installed app's port `9736`. Isolate with `--args --data-dir <tmp> --kanvibe-hook-port <free port>`.
24. **Assuming the DMG is universal.** `build-macos.sh` builds `kanvibe` for the host architecture only. Read the `file` output from step 3.5 and say which architecture the release supports instead of implying both.
25. **Building in a release worktree without credentials.** `.env` is gitignored, so the new release worktree has none and `build-macos.sh` stops with `missing credential: CSC_NAME`. Provide the keys as described in step 3; never commit `.env` or export the keys into the interactive shell.
