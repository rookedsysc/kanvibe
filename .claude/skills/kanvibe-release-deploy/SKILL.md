---
name: kanvibe-release-deploy
description: "Use this skill whenever releasing or deploying KanVibe desktop: report the current package version, ask the user which release version to use, update `package.json`, run the requested `pnpm dev:dist` DMG build, create or update the GitHub release with `dist/KanVibe-<version>.dmg`, and update the KanVibe Homebrew cask tap. Always use it for KanVibe release versions such as 1.0.2, DMG uploads, or Homebrew cask checksum updates."
---

# KanVibe Release Deploy

## Overview

This skill coordinates the KanVibe desktop release workflow from version selection to DMG publication and Homebrew cask update. It is intentionally release-operator focused: read and report the current `package.json` version, ask the user for the target release version, update the package version, run the requested `pnpm dev:dist` DMG build, upload the exact DMG artifact to the `rookedsysc/kanvibe` GitHub release, then update the separate Homebrew cask repository with the same version and SHA-256.

Before touching the cask, read `references/homebrew-cask-repository.md`. The cask repository location and cask file path live there so this skill stays portable if the tap checkout moves.

## When to Use

Use this skill when the user asks to:

- deploy or release KanVibe desktop;
- run `pnpm dev:dist` for KanVibe release packaging;
- bump the KanVibe package version for a release;
- create release notes or a GitHub release that includes `dist/KanVibe-<version>.dmg`;
- update the KanVibe Homebrew cask version or checksum;
- specifically release a version such as `1.0.2` where the expected DMG is `dist/KanVibe-1.0.2.dmg`.

Do not use this skill for docs-site deploys, Linux-only package checks, or routine feature PRs that do not publish a desktop release.

## Release Invariants

- The GitHub release tag is the raw package version, for example `1.0.2`, not `v1.0.2`.
- The DMG filename is `KanVibe-<version>.dmg` because `electron-builder.yml` sets `dmg.artifactName: "KanVibe-${version}.${ext}"`.
- The cask URL expects the same raw version tag: `https://github.com/rookedsysc/kanvibe/releases/download/#{version}/KanVibe-#{version}.dmg`.
- Use the SHA-256 of the final DMG produced after the version bump and `pnpm dev:dist`, not an earlier artifact.
- `pnpm dev:dist` is the requested release build command for this workflow. If the current checkout does not define a `dev:dist` script, stop and report that exact blocker instead of silently substituting `pnpm dist:deploy` or another command.
- If `pnpm dev:dist` performs macOS signing/notarization, it must run on macOS with Apple signing tools configured. Do not fabricate build, notarization, or checksum output from Linux.

## 1. Preflight and Version Selection

Work from the KanVibe application repository, not the Homebrew tap.

```bash
pwd
git status --short --branch
git fetch origin --prune
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

Only proceed after the user supplies the target version. If the original user prompt already supplied the exact target version, echo the current version and target version back to the user and proceed without asking again.

Validate these before proceeding:

1. The target version is a plain SemVer-like value such as `1.0.2`; do not include a leading `v`.
2. The target version is different from the current `package.json` version unless the user explicitly wants to rebuild the same version.
3. The worktree is clean or only contains intentional release-version changes.
4. The release commit/branch is the intended one. If unsure whether to release from `dev`, `main`, or a specific commit, ask before creating the tag.
5. The active GitHub account can create releases in `rookedsysc/kanvibe`.
6. If the release build requires macOS signing tools, the host is macOS:

```bash
uname -s
```

If the required host/tooling is unavailable, stop and report the blocker. Do not fake `pnpm dev:dist`, DMG, notarization, or checksum output.

## 2. Update `package.json` Version

After the user chooses the target version, update `package.json` before running the build. Use a deterministic script instead of manually editing JSON punctuation.

```bash
TARGET_VERSION="<version supplied by user>"
node -e '
const fs = require("node:fs");
const version = process.argv[1];
if (!/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
  throw new Error(`Invalid release version: ${version}`);
}
const packagePath = "package.json";
const packageJson = JSON.parse(fs.readFileSync(packagePath, "utf8"));
packageJson.version = version;
fs.writeFileSync(packagePath, `${JSON.stringify(packageJson, null, 2)}\n`);
' "$TARGET_VERSION"

node -p "require('./package.json').version"
git diff -- package.json
```

Verify the printed package version exactly matches the user-selected target version. Commit the version bump with the release changes or ensure the release tag targets a commit that already contains the updated `package.json`.

## 3. Build the Versioned DMG

KanVibe requires Node 24.x. If `node -p "process.version"` is not v24, switch to Node 24 with the local toolchain available on that Mac before running the release build.

Run the requested release build command after the version bump:

```bash
node -e "const scripts=require('./package.json').scripts || {}; if (!scripts['dev:dist']) { console.error('Missing package.json scripts.dev:dist'); process.exit(1); }"
pnpm install --frozen-lockfile
pnpm dev:dist
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

Create the release with the DMG asset. Use the raw version tag and upload the generated DMG, not a directory or renamed copy.

```bash
VERSION=$(node -p "require('./package.json').version")
DMG="dist/KanVibe-${VERSION}.dmg"
RELEASE_NOTES="/tmp/kanvibe-release-${VERSION}.md"
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

## 6. Final Verification

Before reporting success, collect real output for:

- current version printed before the bump and the user-selected target version;
- `git diff -- package.json` or commit evidence showing the version bump;
- `pnpm dev:dist` completion;
- `test -f dist/KanVibe-<version>.dmg` and `shasum -a 256`;
- `gh release view <version>` showing the `KanVibe-<version>.dmg` asset;
- `curl -I -L` against the release asset URL returning an HTTP success/redirect chain rather than a 404;
- Homebrew cask diff showing only `version` and `sha256` changes;
- cask repository push result or, if not pushed, the exact reason it is left unpushed.

Final handoff format:

```markdown
완료했습니다.

- Previous version: <current version reported before bump>
- Release version: <target version selected by user>
- DMG: `dist/KanVibe-<version>.dmg`
- SHA-256: `<sha256>`
- GitHub release: <release URL>
- Homebrew cask: <commit SHA or branch/status>
- Verification:
  - `<command>` → <real result>
  - `<command>` → <real result>
```

## Common Pitfalls

1. **Skipping the version question.** Always report the current package version and ask the user what version to release unless the prompt already contains the exact target version.
2. **Running a stale version build.** Update `package.json` before `pnpm dev:dist`, then derive the DMG path from the updated version.
3. **Substituting the wrong build command.** The requested command is `pnpm dev:dist`; if it is missing, report that blocker instead of silently running `pnpm dist:deploy`.
4. **Running on Linux and pretending success.** If the build/signing command requires macOS codesign, notarytool, or stapler, stop honestly when the host is not Darwin.
5. **Using a `v` tag.** The cask URL uses the raw version as the release tag. `v1.0.2` will break the current cask URL.
6. **Checksum from the wrong file.** Always hash the final DMG produced after the version bump and release build.
7. **Editing the cask before the release asset exists.** Homebrew fetch/audit can fail if the GitHub release asset is missing or still uploading.
8. **Leaking signing secrets.** Never print `.env` contents or Apple API key material in release notes, logs, commits, or Discord output.
9. **Dirty tap checkout.** Do not overwrite unrelated cask repository edits. Inspect status and either preserve them or ask the user before continuing.
