# Contributing to KanVibe

[KO](./docs/CONTRIBUTING.ko.md) | [ZH](./docs/CONTRIBUTING.zh.md)

Thank you for your interest in contributing to KanVibe! This guide will help you get started.

---

## Development Setup

### Prerequisites

- Node.js 20+ (see `.nvmrc`)
- pnpm
- Docker (for PostgreSQL)
- tmux or zellij (for terminal features)

### Getting Started

```bash
# Clone the repository
git clone https://github.com/rookedsysc/kanvibe.git
cd kanvibe

# Copy environment variables
cp .env.example .env

# Start PostgreSQL
docker compose up db -d

# Install dependencies
pnpm install

# Run database migrations
pnpm migration:run

# Start development server
pnpm dev
```

Open `http://localhost:4885` in your browser.

---

## Pull Request Guidelines

### Before Submitting

1. **All checks must pass:**
   ```bash
   pnpm build    # Build must succeed
   pnpm check    # Type check must pass
   pnpm test     # Tests must pass
   ```

2. **Include a working screenshot or GIF** demonstrating your change. PRs without visual proof of functionality will not be merged.

3. **Follow the existing code style.** The project uses:
   - TypeScript strict mode
   - Tailwind CSS v4 with design tokens (CSS variables)
   - next-intl for all user-facing strings
   - TypeORM migrations for schema changes

### PR Process

1. Fork the repository
2. Create a feature branch from `main`
3. Make your changes
4. Run all checks (`pnpm build && pnpm check && pnpm test`)
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
3. Use `t("key")` in components via `useTranslations` or `getTranslations`

---

## Database Changes

All schema changes go through TypeORM migrations:

```bash
# Generate migration from entity changes
pnpm migration:generate -- src/migrations/DescriptiveName

# Run migrations
pnpm migration:run
```

Never use `synchronize: true`. See `CLAUDE.md` for detailed migration workflow.

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
