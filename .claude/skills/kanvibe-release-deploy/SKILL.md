---
name: kanvibe-release-deploy
description: "Use this skill whenever releasing or deploying KanVibe desktop from a clean, up-to-date `dev` checkout: ask only for the target version and release-note approval, then let the AI update package versions, run `pnpm run deploy`, publish the DMG GitHub release, update the Homebrew cask, create the release PR, and auto-merge the release/promotion PRs when checks pass. Always use it for KanVibe release versions, DMG uploads, or Homebrew cask checksum updates."
---

# KanVibe Release Deploy

## Overview

This skill coordinates the KanVibe desktop release workflow from version selection to DMG publication, Homebrew cask update, release PR creation, and automatic merge. It is intentionally release-operator focused and may only be invoked from a clean, up-to-date local `dev` checkout: read and report the current `package.json` version, ask the user for the target release version, update the package version, run the `pnpm run deploy` DMG build, **launch the built app and confirm it actually starts**, get explicit approval for the release notes, upload the exact DMG artifact to the `rookedsysc/kanvibe` GitHub release, update the separate Homebrew cask repository with the same version and SHA-256, create the KanVibe release PR, then merge the release and promotion PRs automatically once checks and review-thread gates pass.

The smoke test in step 3.5 is not optional. Signing, notarization, stapling, and checksum verification all pass on a bundle whose app cannot start, and shipping one of those is how KanVibe 1.0.4 reached users.

The only routine user gates are target-version selection and release-note approval. After those are approved, do not ask for additional confirmation for build, release publication, cask update, PR creation, auto-merge enablement, or merge completion unless a blocker or branch-policy violation appears.

Before touching the cask, read `references/homebrew-cask-repository.md`. The cask repository location and cask file path live there so this skill stays portable if the tap checkout moves.

## When to Use

Use this skill when the user asks to:

- deploy or release KanVibe desktop;
- run `pnpm run deploy` for KanVibe release packaging;
- bump the KanVibe package version for a release;
- create release notes or a GitHub release that includes `dist/KanVibe-<version>.dmg`;
- create and merge the KanVibe release PR or pinned main-promotion PR after a desktop release;
- update the KanVibe Homebrew cask version or checksum;
- specifically release a version such as `1.0.2` where the expected DMG is `dist/KanVibe-1.0.2.dmg`.

Do not use this skill for docs-site deploys, Linux-only package checks, routine feature PRs that do not publish a desktop release, or any checkout that is not clean, up-to-date `dev`.

## Release Invariants

- The GitHub release tag is the raw package version, for example `1.0.2`, not `v1.0.2`.
- The DMG filename is `KanVibe-<version>.dmg` because `electron-builder.yml` sets `dmg.artifactName: "KanVibe-${version}.${ext}"`.
- The cask URL expects the same raw version tag: `https://github.com/rookedsysc/kanvibe/releases/download/#{version}/KanVibe-#{version}.dmg`.
- Use the SHA-256 of the final DMG produced after the version bump and `pnpm run deploy`, not an earlier artifact.
- `pnpm run deploy` is the release build command for this workflow. It runs `scripts/dist-deploy.cjs`, which builds the DMG, codesigns, notarizes, staples, verifies the packaged dependencies, and prints the SHA-256.
- `pnpm run deploy` performs macOS signing/notarization, so it must run on macOS with Apple signing tools configured. Do not fabricate build, notarization, or checksum output from Linux.
- **A signed, notarized, correctly-checksummed DMG can still be an app that cannot start.** Signing and notarization say nothing about whether the bundle contains the dependencies it needs. Never publish without launching the built app (step 3.5).
- **Never add a `packageManager` field to `package.json`.** electron-builder resolves the package manager from that field before anything else, which selects a dependency collector that drops transitive dependencies from `app.asar` and ships an app that crashes on startup. Pin the pnpm version through `pnpm/action-setup`'s `version` input in CI instead.
- Run the release under **pnpm 10**. Because there is no `packageManager` pin, corepack may resolve pnpm 11, which ignores `pnpm.onlyBuiltDependencies` and skips every native build script. Verify with `pnpm --version` during preflight.
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
node -p "process.version"
CURRENT_VERSION=$(node -p "require('./package.json').version")
printf 'Current KanVibe version: %s\n' "$CURRENT_VERSION"

# The release build must run under pnpm 10; pnpm 11 ignores pnpm.onlyBuiltDependencies
# and silently skips every native build script.
PNPM_VERSION=$(pnpm --version)
printf 'pnpm: %s\n' "$PNPM_VERSION"
case "$PNPM_VERSION" in
  10.*) ;;
  *) echo "Release build requires pnpm 10; found ${PNPM_VERSION}." >&2; exit 1 ;;
esac

# package.json must NOT carry a packageManager field: it forces electron-builder into a
# dependency collector that drops transitive dependencies from app.asar.
if node -p "require('./package.json').packageManager" | grep -qv undefined; then
  echo "package.json has a packageManager field; remove it before releasing." >&2
  exit 1
fi

gh auth status
gh release list --limit 5 --repo rookedsysc/kanvibe
```

If `pnpm --version` is not 10.x, do not silently continue and do not add a `packageManager` field to fix it. Install or select pnpm 10 for this shell, then rerun preflight.

After printing the current version, ask the user which version to release before editing files:

```text
현재 KanVibe 버전은 <CURRENT_VERSION>입니다. 배포 버전을 몇으로 올릴까요? 예: 1.0.2
```

Only proceed after the user supplies the target version. If the original user prompt already supplied the exact target version, echo the current version and target version back to the user and proceed without asking again. This is the first of the two routine user gates; after the target version is selected, do not ask again until release-note review.

Validate these before proceeding:

1. The target version is a plain `x.y.z` value such as `1.0.2`; do not include a leading `v` or any prerelease/build suffix (for example `1.0.3-beta.1`). The desktop update checker (`src/desktop/shared/releaseUpdates.ts`) only parses `^v?\d+\.\d+\.\d+$`, so a prerelease tag would be invisible to in-app updates.
2. The target version is different from the current `package.json` version unless the user explicitly wants to rebuild the same version.
3. Work from a clean local `dev` checkout before creating the release branch. If the current branch is not `dev`, stop and tell the user to rerun the skill from `dev`; do not silently proceed from `main`, a feature branch, or an existing release branch.
4. Confirm local `dev` matches `origin/dev` after `git fetch`/`git pull --ff-only`. The release source is always `origin/dev`; do not ask about or honor alternate source branches in this workflow.
5. The active GitHub account can create releases in `rookedsysc/kanvibe`.
6. If the release build requires macOS signing tools, the host is macOS:

```bash
uname -s
```

If the required host/tooling is unavailable, stop and report the blocker. Do not fake `pnpm run deploy`, DMG, notarization, or checksum output.

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

## 2. Update `package.json` Version

After the user chooses the target version, update `package.json` before running the build. Use a deterministic script instead of manually editing JSON punctuation.

The same script also keeps the tracked `package-lock.json` in sync, because its top-level `version` and root `packages[""].version` fields otherwise keep advertising the previous release and npm-based install/packaging paths would see conflicting metadata. The format check rejects prerelease/build suffixes so the published tag always matches the desktop update checker.

Both files contain several `"version"` lines, and only three of them belong to the release. The replacement therefore anchors on **whole-line exact matches** and, inside `package-lock.json`, on the `packages[""]` block boundary. It never matches substrings and never treats indentation as optional.

```bash
TARGET_VERSION="<version supplied by user>"
node -e '
const fs = require("node:fs");
const version = process.argv[1];
if (!/^\d+\.\d+\.\d+$/.test(version)) {
  throw new Error(`Invalid release version (expected plain x.y.z, no v/prerelease): ${version}`);
}

const current = JSON.parse(fs.readFileSync("package.json", "utf8")).version;
if (current === version) {
  throw new Error(`package.json is already at ${version}; pick a different target version`);
}

const rootNeedle = `  "version": "${current}",`;
const rootReplacement = `  "version": "${version}",`;

const findExactlyOneLine = (label, lines, needle) => {
  const matches = lines.reduce((indexes, line, index) => (line === needle ? [...indexes, index] : indexes), []);
  if (matches.length !== 1) {
    throw new Error(`${label}: expected exactly 1 line matching ${JSON.stringify(needle)}, got ${matches.length}`);
  }
  return matches[0];
};

// package.json: the release version is the only two-space-indented version line.
// devEngines.packageManager.version is indented deeper and must survive untouched.
const packageLines = fs.readFileSync("package.json", "utf8").split("\n");
packageLines[findExactlyOneLine("package.json top-level version", packageLines, rootNeedle)] = rootReplacement;
fs.writeFileSync("package.json", packageLines.join("\n"));

if (fs.existsSync("package-lock.json")) {
  const lockLines = fs.readFileSync("package-lock.json", "utf8").split("\n");
  lockLines[findExactlyOneLine("package-lock.json top-level version", lockLines, rootNeedle)] = rootReplacement;

  // packages[""] is the root package block. Its six-space version line is textually identical
  // to the one in every dependency that happens to sit on the same version, so scan only
  // between the block opener and its closing brace.
  const blockStart = lockLines.indexOf(`    "": {`);
  if (blockStart === -1) {
    throw new Error("package-lock.json: packages[\"\"] block opener not found");
  }
  const innerNeedle = `      "version": "${current}",`;
  let innerIndex = -1;
  for (let i = blockStart + 1; i < lockLines.length; i++) {
    if (lockLines[i] === "    },") break;
    if (lockLines[i] === innerNeedle) {
      innerIndex = i;
      break;
    }
  }
  if (innerIndex === -1) {
    throw new Error("package-lock.json: packages[\"\"].version line not found inside the root package block");
  }
  lockLines[innerIndex] = `      "version": "${version}",`;

  fs.writeFileSync("package-lock.json", lockLines.join("\n"));
}
' "$TARGET_VERSION"

node -p "require('./package.json').version"
node -p "require('./package.json').devEngines.packageManager.version"
test -f package-lock.json && node -e 'const lock = require("./package-lock.json"); console.log(lock.version, lock.packages[""].version);'
git diff --numstat -- package.json package-lock.json
```

The bump is correct only when all four hold:

1. `package.json` version and both `package-lock.json` versions print the user-selected target version. Re-reading them through `require` also proves the files still parse as JSON.
2. `devEngines.packageManager.version` still prints the pnpm pin, not the release version.
3. `git diff --numstat` reports `1 1 package.json` and `2 2 package-lock.json`. Any other count means the edit escaped its anchors.
4. Nothing else appears in `git status`.

Commit the version bump (including `package-lock.json`) with the release changes or ensure the release tag targets a commit that already contains the updated files.

### Why the anchors look like this

Three earlier approaches were tried and discarded. Do not reintroduce them.

| Approach | Used in | Defect |
| --- | --- | --- |
| `JSON.parse` → `JSON.stringify` round-trip | up to 1.0.8 | Reserializes the whole file, so unrelated lines land in the release diff. |
| `^(\s*)"version": "\d+\.\d+\.\d+"` regex | 1.0.8 | `\s*` also matches the deeper indentation of `devEngines.packageManager.version`, which rewrites the **pnpm pin** to the release version. |
| `text.split(<literal indented needle>)` | 1.1.0 | `split` has no concept of line boundaries. A six-space dependency line *contains* the two-space needle as a substring, so the anchor is not isolated. |

The third defect stayed invisible in 1.1.0 only by luck: the then-current version `1.0.10` happened to match no dependency. In 1.2.0 the current version was `1.1.0`, a common value that twenty dependencies in `package-lock.json` also carried, and the two-space needle matched 22 places. The safety of that anchor depended on how rare the current version string happened to be — which is not a property a release procedure may rely on.

## 3. Build the Versioned DMG

KanVibe requires Node 24.x. If `node -p "process.version"` is not v24, switch to Node 24 with the local toolchain available on that Mac before running the release build.

Run the release build command after the version bump:

```bash
pnpm install --frozen-lockfile
pnpm run deploy
```

Expected artifact for version `1.0.2`:

```text
dist/KanVibe-1.0.2.dmg
```

For any version, derive and verify the artifact path from `package.json`:

```bash
VERSION=$(node -p "require('./package.json').version")
DMG="dist/KanVibe-${VERSION}.dmg"
test -f "$DMG"
shasum -a 256 "$DMG"
```

Keep the checksum from this final DMG for the cask update. If you rebuild for any reason, the checksum changes — always re-hash the artifact you are actually publishing.

`scripts/dist-deploy.cjs` prints the dependency-collection mode and a packaging guard result. Both lines must appear and must look like this:

```text
• searching for node modules  pm=npm  searchDir=/Users/<user>/Documents/kanvibe/kanvibe
[kanvibe] packaged node_modules verified: 278 packages
```

If the collector reports `pm=pnpm`, the build is wrong even though it will succeed: that collector silently omits transitive dependencies. Check that `package.json` has no `packageManager` field and that the deploy script is invoking electron-builder directly, then rebuild. The guard aborts the deploy when required modules are missing, but treat `pm=pnpm` itself as a failure signal.

### Notarization blocked by an Apple agreement

When `pnpm run deploy` fails with `HTTP status code: 403. A required agreement is missing or has expired`, this is an Apple account state problem, not a build problem. Do not rebuild to retry — check the credential cheaply instead:

```bash
xcrun notarytool history --key "$APPLE_API_KEY" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER"
```

If a previous release notarized successfully with the same key and team, the configuration is fine and only the agreement changed. The Account Holder must accept the pending agreement at developer.apple.com, and propagation to the notary service takes a few minutes. Poll the command above and resume the build once it returns history.

## 3.5. Smoke-Test the Built App (hard gate)

**Do not publish a release without this step.** Code signing, notarization, stapling, and checksum verification all pass on a bundle whose app cannot start. KanVibe 1.0.4 shipped exactly that way: the DMG was signed, notarized, stapled, and its checksum matched, but `app.asar` was missing seven transitive dependencies and the app died on launch with `Cannot find module 'ms'`.

Mount the DMG you are about to publish, copy the app out, launch it against a throwaway data directory, and read that run's own diagnostics log:

```bash
VERSION=$(node -p "require('./package.json').version")
SMOKE_DATA=$(mktemp -d /tmp/kanvibe-smoke-XXXXXX)
LOG="$SMOKE_DATA/logs/kanvibe-desktop.log"

hdiutil attach "dist/KanVibe-${VERSION}.dmg" -nobrowse -readonly -quiet
MOUNT="/Volumes/KanVibe ${VERSION}-arm64"
spctl -a -t exec -vv "$MOUNT/KanVibe.app"
rm -rf /tmp/KanVibe-smoke.app
cp -R "$MOUNT/KanVibe.app" /tmp/KanVibe-smoke.app
hdiutil detach "$MOUNT" -quiet

open -n --env "KANVIBE_APP_DATA_DIR=$SMOKE_DATA" -a /tmp/KanVibe-smoke.app
sleep 15

pgrep -f "KanVibe-smoke.app/Contents/MacOS/KanVibe" >/dev/null || { echo "app exited during startup" >&2; exit 1; }
wc -l "$LOG"
grep -icE "cannot find module|MODULE_NOT_FOUND|unhandled rejection" "$LOG"
grep -c "invoke-succeeded" "$LOG"

pkill -f "KanVibe-smoke.app"
rm -rf /tmp/KanVibe-smoke.app "$SMOKE_DATA"
```

The release may proceed only when all four hold:

1. `spctl -a -t exec` reports `accepted` and `source=Notarized Developer ID`.
2. The process is still alive after the sleep.
3. The module-error count is `0`.
4. The `invoke-succeeded` count is greater than zero, which proves the database and IPC layer actually came up rather than the window merely opening.

Read the counts from `$LOG` inside `$SMOKE_DATA`, never from `~/Library/Application Support/kanvibe`. Do not delete the user's log to get a clean read.

### Why the smoke run is isolated

`electron/main.js` calls `applyAppDataDirectoryOverride(app, process.env)` at module load, well before `app.whenReady()`. That helper (`electron/runtimeEnvironment.js`) turns `KANVIBE_APP_DATA_DIR` into `app.setPath("userData", ...)`, and the diagnostics log path is derived from `userData` (`electron/diagnostics.js`), so the smoke run's database **and** its log both follow the temporary directory. The user's installed instance, its data, and its existing log are left alone — during the 1.2.0 smoke the verdict came from a 43-line isolated log while `/Applications/KanVibe.app` kept running untouched.

`open --env` applies the variable to the launched app only and does not touch the shell, so this stays inside the repository's runtime-environment safety rules. `open -n` is required: without it macOS just focuses an already-running instance instead of starting the copy under test.

If the smoke test fails, stop. Do not publish, do not update the cask, and do not open PRs. Diagnose the bundle first; `app.asar` contents can be listed from its header:

```bash
node -e '
const fs=require("fs");
const p=process.argv[1] ?? "dist/mac-arm64/KanVibe.app/Contents/Resources/app.asar";
const fd=fs.openSync(p,"r");
const b=Buffer.alloc(16); fs.readSync(fd,b,0,16,0);
const hb=Buffer.alloc(b.readUInt32LE(12)); fs.readSync(fd,hb,0,hb.length,16);
const h=JSON.parse(hb.toString("utf8").replace(/\0+$/,""));
fs.closeSync(fd);
const entries=Object.entries(h.files.node_modules.files);
const expanded=entries.reduce((n,[name,entry])=>n+(name.startsWith("@")?Object.keys(entry.files??{}).length:1),0);
console.log(`${entries.length} top-level entries (scopes counted once)`);
console.log(`${expanded} packages with scopes expanded  <-- matches the build guard`);
' "$ASAR_PATH"
```

The two numbers are both correct and they are not interchangeable. The build guard line `packaged node_modules verified: N packages` comes from `readAsarNodeModuleNames()` in `scripts/dist-deploy.cjs`, which expands `@scope/name` into one entry per scoped package, so it always reports the larger number. Counting the top-level keys instead reports the smaller one. Reading the smaller number as if it were the guard's makes a healthy build look like the 1.0.4 dependency-loss failure.

Do not judge either number against a remembered absolute value; package counts move whenever dependencies change. Judge by comparison: run the same snippet against a build known to be good — the installed `/Applications/KanVibe.app/Contents/Resources/app.asar` is the convenient one — and compare it with the candidate bundle.

## 4. Create or Update the GitHub Release

Draft release notes in a temporary markdown file. Match the existing release-note tone: concise sections such as `## New Features`, `## Improvements and Stability`, and `## Packaging Note`. Include a packaging note naming the exact DMG artifact, for example:

```markdown
## Packaging Note

- The version was updated from `1.0.1` to `1.0.2` before building.
- The DMG artifact is `KanVibe-1.0.2.dmg`.
```

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
VERSION=$(node -p "require('./package.json').version")
DMG="dist/KanVibe-${VERSION}.dmg"
RELEASE_NOTES="/tmp/kanvibe-release-${VERSION}.md"
BASE_BRANCH="dev"
RELEASE_BRANCH="${RELEASE_BRANCH:-release/${VERSION}}"

# Verify the release branch diff is only the expected version bump against the required dev base.
git fetch origin --prune
git diff --name-status "origin/${BASE_BRANCH}"
node -e '
const { execSync } = require("node:child_process");
const baseBranch = process.argv[1];
const allowed = new Set(["package.json", "package-lock.json"]);
const files = execSync(`git diff --name-only origin/${baseBranch}`, { encoding: "utf8" })
  .trim()
  .split(/\n/)
  .filter(Boolean);
const unexpected = files.filter((file) => !allowed.has(file));
if (unexpected.length > 0) {
  throw new Error(`Unexpected release PR files: ${unexpected.join(", ")}`);
}
' "$BASE_BRANCH"

git add package.json package-lock.json
if ! git diff --cached --quiet; then
  git commit -m "chore(release): publish KanVibe ${VERSION}"
fi
git push -u origin "$RELEASE_BRANCH"

# Fail fast if the committed release branch does not contain the selected version.
if ! git diff --quiet -- package.json package-lock.json || ! git diff --cached --quiet -- package.json package-lock.json; then
  echo "package.json/package-lock.json have uncommitted changes; commit the version bump before tagging the release." >&2
  exit 1
fi
HEAD_VERSION=$(git show HEAD:package.json | node -p "JSON.parse(require('fs').readFileSync(0,'utf8')).version")
if [ "$HEAD_VERSION" != "$VERSION" ]; then
  echo "HEAD package.json version ($HEAD_VERSION) does not match target ($VERSION); commit the bump onto the release commit first." >&2
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
VERSION=$(node -p "require('./package.json').version")
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
VERSION=$(node -p "require('./package.json').version")
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

Verify the release PR file list before merging. It should normally contain only `package.json` and `package-lock.json`. If a release-specific file is intentionally added, name it in the PR body and final handoff; otherwise stop.

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
MERGE_SHA=$(gh pr view "$RELEASE_PR" --repo rookedsysc/kanvibe --json mergeCommit --jq .mergeCommit.oid)
gh run list --repo rookedsysc/kanvibe --commit "$RELEASE_HEAD_SHA" --json databaseId,name,status,conclusion
gh run view <databaseId> --repo rookedsysc/kanvibe
```

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
MAIN_VERSION=$(git show origin/main:package.json | node -p "JSON.parse(require('fs').readFileSync(0,'utf8')).version")
test "$MAIN_VERSION" = "$VERSION"

# Clean up the release branch only after the main promotion is merged and the release commit's
# workflow runs have reached a conclusion. Deleting the branch cancels runs still in progress.
if [ "$PROMOTION_STATE" = "MERGED" ]; then
  gh run list --repo rookedsysc/kanvibe --commit "$RELEASE_HEAD_SHA" --json name,status,conclusion
  git push origin --delete "$PROMOTION_BRANCH" || true
fi
```

## 7. Final Verification

Before reporting success, collect real output for:

- dev-only preflight evidence: current branch `dev`, clean status, and local `HEAD` matching `origin/dev` before the release branch was cut;
- `pnpm --version` showing 10.x and `package.json` carrying no `packageManager` field;
- current version printed before the bump and the user-selected target version;
- `git diff --numstat -- package.json package-lock.json` showing `1 1 package.json` and `2 2 package-lock.json`, plus `devEngines.packageManager.version` still on the pnpm pin;
- `pnpm run deploy` completion, including the `pm=npm` collector line and the `packaged node_modules verified: N packages` guard line;
- **smoke-test evidence from step 3.5**: `spctl` verdict, process alive after launch, module-error count `0`, and a non-zero `invoke-succeeded` count;
- `test -f dist/KanVibe-<version>.dmg` and `shasum -a 256`;
- user approval of the release notes, confirming both the English original and the Korean translation were shown before publishing;
- `gh release view <version>` showing the `KanVibe-<version>.dmg` asset;
- `curl -I -L` against the release asset URL returning an HTTP success/redirect chain rather than a 404;
- Homebrew cask diff showing only `version` and `sha256` changes, plus the cask repository push result or exact blocker;
- release branch commit SHA and pushed branch name;
- release PR URL, file list, check result, unresolved review-thread count, and merge or auto-merge result into `dev`;
- the release commit's workflow conclusion from `gh run list --commit <sha>`, collected after the merge because a pending check does not hold it;
- promotion PR URL, check result, unresolved review-thread count, and merge or auto-merge result into `main`;
- `git show origin/main:package.json` confirming `main` now contains the release version, or the exact blocker if auto-merge is still pending.

Final handoff format:

```markdown
완료했습니다.

- Dev preflight: <branch/status/local HEAD == origin/dev evidence>
- Previous version: <current version reported before bump>
- Release version: <target version selected by user>
- DMG: `dist/KanVibe-<version>.dmg`
- SHA-256: `<sha256>`
- Packaging guard: <collector mode + verified package count>
- Smoke test: <app alive / module errors / invoke-succeeded count>
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

1. **Skipping the version question.** Always report the current package version and ask the user what version to release unless the prompt already contains the exact target version.
2. **Running a stale version build.** Update `package.json` before `pnpm run deploy`, then derive the DMG path from the updated version.
3. **Running on Linux and pretending success.** `pnpm run deploy` requires macOS codesign, notarytool, and stapler, so stop honestly when the host is not Darwin.
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
14. **Invoking the build as `pnpm deploy`.** `deploy` is a reserved pnpm command (workspace deploy), so `pnpm deploy` fails with `ERR_PNPM_CANNOT_DEPLOY` and never runs the release script. Always invoke the release build as `pnpm run deploy`.
15. **Treating signing and notarization as proof the app works.** They verify provenance, not completeness. KanVibe 1.0.4 was signed, notarized, stapled, and checksum-matched, and still could not start because `app.asar` was missing seven transitive dependencies. Step 3.5 is the only check that catches this class of failure.
16. **Adding a `packageManager` field to `package.json`.** electron-builder reads it before lock files and before the environment, which selects a dependency collector that drops transitive dependencies in this repository's hoisted layout. This is what broke 1.0.4. Pin pnpm through CI's `pnpm/action-setup` `version` input instead.
17. **Ignoring the `pm=` collector line.** The build succeeds either way, so `pm=pnpm` is easy to scroll past. It means the artifact is probably incomplete — treat it as a failure, not a detail.
18. **Rebuilding to retry an Apple agreement 403.** The failure is account state, not build state. Re-check with `xcrun notarytool history` and, if an earlier release succeeded with the same key, wait for propagation rather than spending build cycles.
19. **Launching the app by executing its binary directly.** Use `open -a`. Running `KanVibe.app/Contents/MacOS/KanVibe` as a child of the terminal makes macOS attribute the app's file-access prompt to the terminal process; a denial there revokes `~/Documents` access for the whole process tree and blocks the rest of the release.
20. **Blocking on the promotion PR's diff size.** The file-scope guard belongs to the release PR into `dev`. The promotion PR into `main` carries every commit `main` is behind by, so a wide diff there is normal.
21. **Misreading DMG verification signals.** `spctl -a -t open` on the DMG reports `rejected / no usable signature` and the app inside reports "does not have a ticket stapled" even for a correct release — the ticket is stapled to the DMG, and the disk image itself is not Developer ID signed. Judge with `xcrun stapler validate <dmg>` and `spctl -a -t exec` on the app.
