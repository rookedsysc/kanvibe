# KanVibe Homebrew Cask Repository

Use this reference whenever the `kanvibe-release-deploy` skill needs to update the Homebrew cask after publishing a KanVibe DMG release.

## Repository Location

- Local checkout: `~/Documents/kanvibe/homebrew-kanvibe` (on the release Mac this resolves to `/Users/rookedsysc/Documents/kanvibe/homebrew-kanvibe`; the release build is macOS-only, so do not use a `/home/...` path)
- GitHub repository: `https://github.com/rookedsysc/homebrew-kanvibe`
- Default branch: `main`
- Tap name: `rookedsysc/kanvibe`
- Cask file: `Casks/kanvibe.rb`

## Cask Update Target

Update only these lines for a normal release:

```ruby
version "<release-version>"
sha256 "<sha256-of-final-stapled-dmg>"
```

Keep this URL shape unchanged unless the release asset naming convention changes in the KanVibe app repository:

```ruby
url "https://github.com/rookedsysc/kanvibe/releases/download/#{version}/KanVibe-#{version}.dmg"
```

## Normal Commands

```bash
CASK_REPO="$HOME/Documents/kanvibe/homebrew-kanvibe"
CASK_FILE="$CASK_REPO/Casks/kanvibe.rb"

git -C "$CASK_REPO" status --short --branch
git -C "$CASK_REPO" pull --ff-only origin main
ruby -c "$CASK_FILE"
```

After editing `version` and `sha256`:

```bash
ruby -c "$CASK_FILE"
git -C "$CASK_REPO" diff -- Casks/kanvibe.rb
git -C "$CASK_REPO" add Casks/kanvibe.rb
git -C "$CASK_REPO" commit -m "Update KanVibe cask to ${VERSION}"
git -C "$CASK_REPO" push origin main
```

## Verifying the Cask

`brew fetch --cask "$CASK_FILE"` does not work: Homebrew rejects casks outside a tap with `Homebrew requires casks to be in a tap`. Verify in two stages instead.

Before pushing, prove the checksum against the bytes GitHub actually serves:

```bash
curl -sL "https://github.com/rookedsysc/kanvibe/releases/download/${VERSION}/KanVibe-${VERSION}.dmg" | shasum -a 256
```

That value must match the `sha256` in the cask. After pushing, verify through the tap name:

```bash
brew update
brew fetch --cask rookedsysc/kanvibe/kanvibe   # expect: ✔︎ Cask kanvibe (<version>)
```
