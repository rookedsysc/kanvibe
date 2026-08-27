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
- `app/[lang]/[[...mdxPath]]/page.jsx` renders MDX pages and attaches canonical/hreflang metadata.
- `app/[lang]/software-application-structured-data.jsx` emits the `SoftwareApplication` JSON-LD on the home page.
- `app/sitemap.js` and `app/robots.js` generate `/sitemap.xml` and `/robots.txt`.
- `lib/docsSite.mjs` owns the locale list, page list, canonical site URL, and hreflang builders.
- `lib/docsContent.mjs` reads the content directory and each page's last commit time.
- `lib/docsDictionary.mjs` holds the locale-specific shell copy shared by the layout and structured data.
- `content/ko`, `content/en`, and `content/zh` contain localized MDX pages.
- `public/screenshots` contains KanVibe feature screenshots used by the documentation.

## Site URL configuration

`KANVIBE_DOCS_SITE_URL` is the canonical origin of the deployed docs site. It feeds the sitemap
`<loc>` values, `rel=canonical`, hreflang alternates, and the `Sitemap:` line in `robots.txt`.

- Development builds fall back to `http://localhost:3000`.
- Production builds fail when the value is missing, so a wrong canonical URL can never ship silently.

```bash
KANVIBE_DOCS_SITE_URL=https://docs.example.com pnpm build
```

In CI the value comes from the `KANVIBE_DOCS_SITE_URL` repository variable.

## Search Console sitemap submission

Google removed the unauthenticated sitemap ping endpoint, so `.github/workflows/docs.yml`
submits the sitemap through the authenticated Search Console API on pushes.

Set these up once:

1. Create a Google Cloud service account and download its JSON key.
2. In Search Console, add the service account's `client_email` as a user on the property.
   Sitemap submission needs at least *Full* permission; *Owner* also covers owner-only APIs.
3. Store the values in the repository:
   - variable `KANVIBE_GSC_SITE_URL` — the property as Search Console defines it,
     either `https://docs.example.com/` or `sc-domain:example.com`.
   - secret `KANVIBE_GSC_CREDENTIALS` — the full service account key JSON.

Without those values the submit step logs a skip notice and succeeds, so docs CI keeps working
before the property is set up.

## Adding a page

`lib/docsSite.mjs` lists the documentation pages explicitly. After adding an MDX file to every
locale, add its path to `DOCS_PAGE_PATHS` — `docs-site/lib/__tests__/docsSite.test.js` fails when
the list and the content directory disagree.
