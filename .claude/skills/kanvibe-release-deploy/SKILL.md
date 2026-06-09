---
name: kanvibe-release-deploy
description: "Use this skill whenever releasing or deploying KanVibe desktop from a clean, up-to-date `dev` checkout: ask only for the target version and release-note approval, then let the AI update package versions, run `pnpm run deploy`, publish the DMG GitHub release, update the Homebrew cask, create the release PR, and auto-merge the release/promotion PRs when checks pass. Always use it for KanVibe release versions, DMG uploads, or Homebrew cask checksum updates."
---

# KanVibe Release Deploy

## Overview

This skill coordinates the KanVibe desktop release workflow from version selection to DMG publication, Homebrew cask update, release PR creation, and automatic merge. It is intentionally release-operator focused and may only be invoked from a clean, up-to-date local `dev` checkout: read and report the current `package.json` version, ask the user for the target release version, update the package version, run the `pnpm run deploy` DMG build, get explicit approval for the release notes, upload the exact DMG artifact to the `rookedsysc/kanvibe` GitHub release, update the separate Homebrew cask repository with the same version and SHA-256, create the KanVibe release PR, then merge the release and promotion PRs automatically once checks and review-thread gates pass.

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
- `pnpm run deploy` is the release build command for this workflow. It runs `scripts/dist-deploy.cjs`, which builds the DMG, codesigns, notarizes, staples, and prints the SHA-256.
- `pnpm run deploy` performs macOS signing/notarization, so it must run on macOS with Apple signing tools configured. Do not fabricate build, notarization, or checksum output from Linux.
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
gh auth status
gh release list --limit 5 --repo rookedsysc/kanvibe
```

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

The same script also keeps the tracked `package-lock.json` in sync, because its top-level `version` and root `packages[""].version` fields otherwise keep advertising the previous release and npm-based install/packaging paths would see conflicting metadata. The regex rejects prerelease/build suffixes so the published tag always matches the desktop update checker.

```bash
TARGET_VERSION="<version supplied by user>"
node -e '
const fs = require("node:fs");
const version = process.argv[1];
if (!/^\d+\.\d+\.\d+$/.test(version)) {
  throw new Error(`Invalid release version (expected plain x.y.z, no v/prerelease): ${version}`);
}
const packageJson = JSON.parse(fs.readFileSync("package.json", "utf8"));
packageJson.version = version;
fs.writeFileSync("package.json", `${JSON.stringify(packageJson, null, 2)}\n`);
if (fs.existsSync("package-lock.json")) {
  const lock = JSON.parse(fs.readFileSync("package-lock.json", "utf8"));
  lock.version = version;
  if (lock.packages && lock.packages[""]) lock.packages[""].version = version;
  fs.writeFileSync("package-lock.json", `${JSON.stringify(lock, null, 2)}\n`);
}
' "$TARGET_VERSION"

node -p "require('./package.json').version"
test -f package-lock.json && node -p "require('./package-lock.json').version"
git diff -- package.json package-lock.json
```

Verify the printed `package.json` and `package-lock.json` versions both match the user-selected target version. Commit the version bump (including `package-lock.json`) with the release changes or ensure the release tag targets a commit that already contains the updated files.

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

Keep the checksum from this final DMG for the cask update.

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

Optional macOS/Homebrew validation from inside the tap checkout:

```bash
brew audit --cask Casks/kanvibe.rb
brew fetch --cask Casks/kanvibe.rb
```

If Homebrew cannot run in the current environment, still verify Ruby syntax, the release asset URL, and the exact cask diff before pushing.

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

Merge automatically when checks are green. If checks are still pending but the PR is otherwise mergeable, enable auto-merge and keep watching until the PR becomes `MERGED` before starting the promotion PR. Never enable auto-merge after a failed, cancelled, timed-out, or action-required check.

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

# Clean up the release branch only after the main promotion is actually merged.
if [ "$PROMOTION_STATE" = "MERGED" ]; then
  git push origin --delete "$PROMOTION_BRANCH" || true
fi
```

## 7. Final Verification

Before reporting success, collect real output for:

- dev-only preflight evidence: current branch `dev`, clean status, and local `HEAD` matching `origin/dev` before the release branch was cut;
- current version printed before the bump and the user-selected target version;
- `git diff -- package.json package-lock.json` or commit evidence showing the version bump in both files;
- `pnpm run deploy` completion;
- `test -f dist/KanVibe-<version>.dmg` and `shasum -a 256`;
- user approval of the release notes, confirming both the English original and the Korean translation were shown before publishing;
- `gh release view <version>` showing the `KanVibe-<version>.dmg` asset;
- `curl -I -L` against the release asset URL returning an HTTP success/redirect chain rather than a 404;
- Homebrew cask diff showing only `version` and `sha256` changes, plus the cask repository push result or exact blocker;
- release branch commit SHA and pushed branch name;
- release PR URL, file list, check result, unresolved review-thread count, and merge or auto-merge result into `dev`;
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
- GitHub release: <release URL>
- Homebrew cask: <commit SHA or branch/status>
- Release PR: <URL> → <merged/auto-merge/blocker>
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
