# Contributing to KanVibe

[KO](./docs/CONTRIBUTING.ko.md) | [ZH](./docs/CONTRIBUTING.zh.md)

Thank you for your interest in contributing to KanVibe.

---

## Current Architecture

KanVibe now runs as a desktop-only Tauri application.

- Active runtime: Tauri v2 + Rust backend
- Desktop UI host: `src-tauri/desktop/index.html`
- Rust entry point: `src-tauri/src/main.rs`
- Local persistence: SQLite via `rusqlite`
- No active Next.js or Node web runtime path

---

## Development Setup

### Prerequisites

- Node.js 22+
- pnpm
- Rust toolchain (`rustup`, `cargo`)
- GTK/WebKit runtime dependencies required by Tauri on your OS
- tmux or zellij if you want to verify terminal/worktree flows locally

### Getting Started

```bash
# Clone the repository
git clone https://github.com/rookedsysc/kanvibe.git
cd kanvibe

# Copy environment variables
cp .env.example .env

# Install JS-side tooling
pnpm install

# Start the desktop app in development mode
pnpm dev
```

### Useful Commands

```bash
pnpm dev            # Run the Tauri desktop app in development mode
pnpm check          # cargo check for src-tauri
pnpm test           # cargo test for src-tauri
pnpm lint           # cargo fmt --check
pnpm build          # Build the desktop release binary
pnpm desktop:qa     # check + test + build in one pass
```

---

## Desktop Development Workflow

When working on the app, prefer this loop:

1. Update Rust backend code under `src-tauri/src/`
2. Update desktop UI shell under `src-tauri/desktop/index.html` if needed
3. Run `pnpm check`
4. Run `pnpm test`
5. Launch with `pnpm dev` and verify the flow manually

### Runtime Notes

- Tauri serves the local desktop assets defined by `frontendDist` in `src-tauri/tauri.conf.json`
- The active UI talks to Rust through `window.__TAURI__.core.invoke(...)`
- SQLite schema bootstrapping currently lives in `src-tauri/src/backend/db.rs`

---

## Build and Release

### Local Release Build

```bash
pnpm build
```

Current release output:

- Binary: `src-tauri/target/release/kanvibe-desktop`

### Runtime Verification

After building, verify that the desktop binary launches.

```bash
./src-tauri/target/release/kanvibe-desktop
```

For headless Linux environments, a Broadway-based smoke test is acceptable:

```bash
broadwayd :7 >/tmp/kanvibe-broadway.log 2>&1 &
GDK_BACKEND=broadway BROADWAY_DISPLAY=:7 timeout 8s ./src-tauri/target/release/kanvibe-desktop
```

If the process stays alive until timeout, treat that as a successful launch smoke test.

### Packaging and Distribution

At the moment, `src-tauri/tauri.conf.json` has `bundle.active` set to `false`, so the default release flow produces a desktop binary only.

If you want installers or OS-native bundles:

1. Enable bundling in `src-tauri/tauri.conf.json`
2. Configure bundle targets and signing as needed for your platform
3. Run `pnpm build` again

---

## Pull Request Guidelines

### Before Submitting

1. All checks must pass:

```bash
pnpm check
pnpm test
pnpm build
```

2. If you changed visible desktop behavior, attach a screenshot or GIF.
3. Keep changes aligned with the desktop-only Tauri/Rust architecture.

### PR Process

1. Fork the repository
2. Create a branch from `dev`
3. Make your changes
4. Run `pnpm desktop:qa`
5. Verify the desktop app with `pnpm dev` or a release launch check
6. Commit with Conventional Commits
7. Push and open a Pull Request

### Commit Message Format

```text
feat(scope): add new feature
fix(scope): fix specific bug
docs: update documentation
refactor(scope): restructure code
```

---

## Database Changes

KanVibe no longer uses TypeORM migrations in the active runtime.

When changing persistence behavior:

1. Update schema creation logic in `src-tauri/src/backend/db.rs`
2. Update the Rust commands that read or write the affected tables
3. Rebuild and validate existing local data behavior carefully

Because the app uses local SQLite, backward compatibility matters. Avoid destructive schema changes unless they are explicitly planned.

---

## Documentation Updates

When changing user-facing or contributor-facing behavior, update the related docs together:

| Changed | Update |
|---------|--------|
| Desktop behavior or workflow | `README.md`, `docs/README.ko.md`, `docs/README.zh.md` |
| Contributing or release flow | `CONTRIBUTING.md`, `docs/CONTRIBUTING.ko.md`, `docs/CONTRIBUTING.zh.md` |
| Environment variables | `.env.example` + all README files |
| Tauri build or release process | README + all contributing guides |

Keep EN, KO, and ZH versions aligned.

---

## License

By contributing to KanVibe, you agree that your contributions will be licensed under the [AGPL-3.0 License](./LICENSE).
