# KanVibe Homebrew Cask Repository

Use this reference whenever the `kanvibe-release-deploy` skill needs to update the Homebrew cask after publishing a KanVibe DMG release.

## Repository Location

- Local checkout: `/home/rookedsysc/Documents/kanvibe/homebrew-kanvibe`
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
CASK_REPO="/home/rookedsysc/Documents/kanvibe/homebrew-kanvibe"
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
