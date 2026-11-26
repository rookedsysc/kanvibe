# Contributing to KanVibe

[KO](./docs/CONTRIBUTING.ko.md) | [ZH](./docs/CONTRIBUTING.zh.md)

Thank you for your interest in contributing to KanVibe! This guide will help you get started.

---

## Development Setup

### Prerequisites

- Rust stable with `rustfmt` and `clippy` (edition 2024)
- [cargo-nextest](https://nexte.st/)
- Herdr, tmux, or zellij (for terminal features)
- Linux only: the GPUI and WebKitGTK development packages, for example on Ubuntu:
  `pkg-config libfontconfig-dev libfreetype-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev libwebkit2gtk-4.1-dev`

### Getting Started

```bash
# Clone the repository
git clone https://github.com/rookedsysc/kanvibe.git
cd kanvibe

# Start the desktop app from source (debug build, development database)
cargo run -p kanvibe-app
```

A debug build uses the development data directory and hook port `19736`. See [Development and QA databases](./README.md#development-and-qa-databases).

### Release Build

```bash
# Linux: tarball and .deb in dist/ (--verify launches the unpacked bundle once;
# --allow-missing-agents skips remote-agent targets this machine cannot build)
packaging/linux/build-linux.sh --verify

# macOS: signed and notarized KanVibe.app and dist/KanVibe-${version}.dmg
packaging/macos/build-macos.sh
```

Both scripts build `kanvibe-app` with the `packaged` cargo feature. Only that binary uses the installed app's data directory and hook port `9736`; `cargo build --release` without it runs as manual QA (`<appData>/kanvibe-qa`, port `29736`).

The version comes from `workspace.package.version` in the root `Cargo.toml`. `build-macos.sh` reads the signing credentials from the environment or from the keys listed in `.env.example`; `--unsigned` builds without them for local checks.

- macOS output: `dist/KanVibe-${version}.dmg`
- Homebrew cask: update `Casks/kanvibe.rb`

---

## Pull Request Guidelines

### Before Submitting

1. **All checks must pass:**
   ```bash
   cargo fmt --all --check                                  # Formatting
   cargo clippy --workspace --all-targets -- -D warnings    # Lints
   cargo nextest run --workspace                            # Tests
   cargo build --workspace --release                        # Release build
   ```

2. **Include a working screenshot or GIF** demonstrating your change. PRs without visual proof of functionality will not be merged.

3. **Follow the existing code style.** The project uses:
   - A Rust workspace under `crates/` (one crate per layer: `kanvibe-domain`, `kanvibe-db`, `kanvibe-services`, `kanvibe-app`, ...)
   - GPUI views with semantic theme tokens (`crates/kanvibe-app/src/theme.rs`)
   - `messages/*.json` for all user-facing strings
   - Embedded SQLite through `rusqlite` (`kanvibe-db`)

### PR Process

1. Fork the repository
2. Create a feature branch from `main`
3. Make your changes
4. Run all checks (see above)
5. Commit with [Conventional Commits](https://www.conventionalcommits.org/) format
6. Push and create a Pull Request
7. Attach a screenshot/GIF of the working feature

### Commit Message Format

```
feat(scope): add new feature
fix(scope): fix specific bug
docs: update documentation
refactor(scope): code restructuring
```

---

## Internationalization (i18n)

All user-facing strings must be translated. When adding or modifying UI text:

1. Add the key/value to `messages/ko.json`
2. Add translations to `messages/en.json` and `messages/zh.json`
3. Read the text in views through `kanvibe_domain::i18n` (`Translator::translate` or `translate`)

---

## Database Changes

KanVibe uses an embedded SQLite database (`crates/kanvibe-db`). When you change the table structure:

1. Add a migration to `MIGRATIONS` in `crates/kanvibe-db/src/migrations/mod.rs`
2. Update the idempotent schema repair in `crates/kanvibe-db/src/schema/` so older databases reach the same schema

---

## Documentation Updates

When making changes that affect user-facing behavior, update the corresponding documentation:

| Changed | Update |
|---------|--------|
| Features or UI | `README.md`, `docs/README.ko.md`, `docs/README.zh.md` |
| Contributing process | `docs/CONTRIBUTING.md`, `docs/CONTRIBUTING.ko.md`, `docs/CONTRIBUTING.zh.md` |
| Environment variables | `.env.example` + all README files |
| Hook API | All README files (Hook API section) |

All three language versions (EN, KO, ZH) must be updated together.

---

## Next Steps & Roadmap

### Gemini Hooks / Codex Hooks Support (In Development)

Currently, KanVibe supports **Claude Code Hooks** for automatic status tracking. Support for **Gemini Hooks** and **Codex Hooks** is planned and under development.

If you have ideas for a better approach or architecture for multi-agent hook support, please open a [Discussion](https://github.com/rookedsysc/kanvibe/discussions) first. We'd love to collaborate on finding the best direction before implementation begins.

### Token Usage Dashboard

The next major feature goal is a **token usage tracking dashboard** - monitoring AI agent token consumption across tasks and sessions.

### How to Propose New Features

For significant changes or new features:
1. Open a [Discussion](https://github.com/rookedsysc/kanvibe/discussions) to share your idea
2. Get feedback from the community and maintainers
3. Once the direction is agreed upon, create an Issue and submit a PR

---

## License

By contributing to KanVibe, you agree that your contributions will be licensed under the [AGPL-3.0 License](../LICENSE).
