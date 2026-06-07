---
name: kanvibe-release-deploy
description: "Use this skill whenever releasing or deploying KanVibe desktop: run the signed/notarized `pnpm dist:deploy` build, create or update the GitHub release with the generated `dist/KanVibe-<version>.dmg` asset, and update the KanVibe Homebrew cask tap. Always use it for KanVibe release versions such as 1.0.2, DMG uploads, or Homebrew cask checksum updates."
---

# KanVibe Release Deploy

## Overview

This skill coordinates the KanVibe desktop release workflow from a signed/notarized macOS DMG to GitHub release publication and Homebrew cask update. It is intentionally release-operator focused: verify the version, build with `pnpm dist:deploy`, upload the exact DMG artifact to the `rookedsysc/kanvibe` GitHub release, then update the separate Homebrew cask repository with the same version and SHA-256.

Before touching the cask, read `references/homebrew-cask-repository.md`. The cask repository location and cask file path live there so this skill stays portable if the tap checkout moves.

## When to Use

Use this skill when the user asks to:

- deploy or release KanVibe desktop;
- run `pnpm dist:deploy` for KanVibe;
- create release notes or a GitHub release that includes `dist/KanVibe-<version>.dmg`;
- update the KanVibe Homebrew cask version or checksum;
- specifically release a version such as `1.0.2` where the expected DMG is `dist/KanVibe-1.0.2.dmg`.

Do not use this skill for docs-site deploys, Linux-only package checks, or routine feature PRs that do not publish a desktop release.

## Release Invariants

- The GitHub release tag is the raw package version, for example `1.0.2`, not `v1.0.2`.
- The DMG filename is `KanVibe-<version>.dmg` because `electron-builder.yml` sets `dmg.artifactName: "KanVibe-${version}.${ext}"`.
- The cask URL expects the same raw version tag: `https://github.com/rookedsysc/kanvibe/releases/download/#{version}/KanVibe-#{version}.dmg`.
- Use the SHA-256 of the final stapled DMG produced by `pnpm dist:deploy`, not an earlier unsigned or unstapled artifact.
- `pnpm dist:deploy` must run on macOS. It requires Apple signing/notarization tooling and exits on non-Darwin hosts.

## 1. Preflight

Work from the KanVibe application repository, not the Homebrew tap.

```bash
pwd
git status --short --branch
git fetch origin --prune
node -p "process.version"
node -p "require('./package.json').version"
gh auth status
gh release list --limit 5 --repo rookedsysc/kanvibe
```

Validate these before proceeding:

1. The requested release version matches `package.json` `version`. If the user asks for `1.0.2`, `node -p "require('./package.json').version"` must print `1.0.2` before building.
2. The worktree is clean or only contains intentional release-version changes.
3. The release commit/branch is the intended one. If unsure whether to release from `dev`, `main`, or a specific commit, ask before creating the tag.
4. The active GitHub account can create releases in `rookedsysc/kanvibe`.
5. The host is macOS:

```bash
uname -s
```

If it is not `Darwin`, stop and report that `pnpm dist:deploy` cannot be exercised on this host. Do not fabricate build, notarization, or checksum output.

## 2. Build, sign, notarize, and staple the DMG

KanVibe requires Node 24.x. If `node -p "process.version"` is not v24, switch to Node 24 with the local toolchain available on that Mac before running the release build.

`pnpm dist:deploy` loads `.env`, verifies Apple signing values, runs `pnpm dist`, verifies the app signature, submits the DMG to notarytool, staples the ticket, validates the staple, and prints the DMG SHA-256.

```bash
pnpm install --frozen-lockfile
pnpm dist:deploy
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

## 3. Create or update the GitHub release

Draft release notes in a temporary markdown file. Match the existing release-note tone: concise sections such as `## New Features`, `## Improvements and Stability`, and `## Packaging Note`. Include a packaging note naming the exact DMG artifact, for example:

```markdown
## Packaging Note

- The signed and notarized DMG artifact is `KanVibe-1.0.2.dmg`.
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

## 4. Update the Homebrew cask tap

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

## 5. Final Verification

Before reporting success, collect real output for:

- `pnpm dist:deploy` completion and stapler validation;
- `test -f dist/KanVibe-<version>.dmg` and `shasum -a 256`;
- `gh release view <version>` showing the `KanVibe-<version>.dmg` asset;
- `curl -I -L` against the release asset URL returning an HTTP success/redirect chain rather than a 404;
- Homebrew cask diff showing only `version` and `sha256` changes;
- cask repository push result or, if not pushed, the exact reason it is left unpushed.

Final handoff format:

```markdown
완료했습니다.

- Version: <version>
- DMG: `dist/KanVibe-<version>.dmg`
- SHA-256: `<sha256>`
- GitHub release: <release URL>
- Homebrew cask: <commit SHA or branch/status>
- Verification:
  - `<command>` → <real result>
  - `<command>` → <real result>
```

## Common Pitfalls

1. **Running on Linux and pretending success.** `pnpm dist:deploy` requires macOS codesign, notarytool, and stapler. Stop honestly if the host is not Darwin.
2. **Using a `v` tag.** The cask URL uses the raw version as the release tag. `v1.0.2` will break the current cask URL.
3. **Checksum from the wrong file.** Always hash the final stapled DMG produced by `pnpm dist:deploy`.
4. **Package version mismatch.** `dist/KanVibe-<version>.dmg` is derived from `package.json`; update and commit the package version before building if the requested release version differs.
5. **Editing the cask before the release asset exists.** Homebrew fetch/audit can fail if the GitHub release asset is missing or still uploading.
6. **Leaking signing secrets.** Never print `.env` contents or Apple API key material in release notes, logs, commits, or Discord output.
7. **Dirty tap checkout.** Do not overwrite unrelated cask repository edits. Inspect status and either preserve them or ask the user before continuing.
