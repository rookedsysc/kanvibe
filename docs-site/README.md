# KanVibe Docs

Nextra 4 documentation site for KanVibe.

## Scripts

Run from the repository root:

```bash
pnpm docs:dev
pnpm docs:build
pnpm docs:start
```

Or run directly in this directory:

```bash
pnpm dev
pnpm build
pnpm start
```

## Structure

- `app/[lang]/layout.jsx` configures the shared Nextra docs layout and language switcher.
- `content/ko`, `content/en`, and `content/zh` contain localized MDX pages.
- `public/screenshots` contains KanVibe feature screenshots used by the documentation.
