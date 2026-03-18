## Tech Stack
- Next.js 16 + React 19 + TypeScript
- Embedded SQLite via TypeORM + better-sqlite3
- Electron + Electron Builder for desktop packaging
- next-intl, xterm.js, node-pty, ssh2

## Runtime Architecture
- `boot.js` loads `.env`, registers `tsx`, then boots `server.ts`
- `server.ts` runs Next custom HTTP server + separate WebSocket server for terminals and board notifications
- Desktop entrypoint: `electron/main.js` boots the internal server, waits for `http://127.0.0.1:${PORT}/ko/login`, then opens BrowserWindow
- Electron sets `KANVIBE_APP_DATA_DIR` and `KANVIBE_SEED_DB_PATH` so runtime DB lives under Electron `userData`

## Database
- `src/lib/database.ts` uses TypeORM `better-sqlite3`
- `src/lib/databasePaths.ts` resolves runtime DB path and copies bundled seed DB if available
- `src/lib/sqliteSchema.ts` idempotently bootstraps tables/columns/indexes for SQLite
- `scripts/build-seed-db.ts` generates `resources/database/app.seed.db`
- `pnpm db:prepare` rebuilds the bundled seed DB

## Packaging
- `electron-builder.yml` packages `.next`, `src`, `server.ts`, `boot.js`, `electron/`, public assets, messages
- `asar: false` is currently used so the custom Next server can read packaged files directly
- `pnpm dist:dir` succeeded on Linux and produced `dist/linux-unpacked/`
- `pnpm dist` is configured for macOS DMG/ZIP builds; actual DMG creation requires macOS host

## Notes
- Browser notifications + hooks remain on the existing localhost HTTP/WebSocket flow
- Login defaults now fall back to `admin` / `changeme` if env vars are absent, which is important for packaged desktop use
- One unrelated user change remains in `.codex/hooks/kanvibe-notify-hook.sh`; do not overwrite it unintentionally