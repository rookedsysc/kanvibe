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
- `lib/docsSite.mjs` owns the locale list, page list, origin resolution, and the sitemap/robots builders.
- `app/docsSiteUrl.js` picks the origin for the metadata routes: the pinned URL, or the request host.
- `lib/docsDictionary.mjs` holds the locale-specific shell copy shared by the layout and structured data.
- `content/ko`, `content/en`, and `content/zh` contain localized MDX pages.
- `public/screenshots` contains KanVibe feature screenshots used by the documentation.

## Site URL configuration

`/sitemap.xml` and `/robots.txt` run per request, so by default they use the host that served
them. That is always right for whichever hostname a visitor reached, and needs no configuration.

`KANVIBE_DOCS_SITE_URL` pins a single canonical origin instead. Set it once the docs site has a
domain you want search engines to consolidate on — for example when a `workers.dev` address and a
custom domain both serve the same pages. Pinning it also turns on `rel=canonical` and page-level
hreflang, which the statically generated pages cannot derive from a request.

```bash
KANVIBE_DOCS_SITE_URL=https://docs.example.com pnpm build
```

The value has to reach every build path. GitHub Actions reads the `KANVIBE_DOCS_SITE_URL`
repository variable, but Cloudflare Workers Builds runs its own build and does not see it — set it
there too, or the deployed site keeps using the request host.

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

Submission also needs `KANVIBE_DOCS_SITE_URL`. The routes derive their origin from the request,
but this script has no request to read, so once the property is configured the pinned origin is
what tells it which sitemap to submit. It fails with that name in the message when it is missing.

## Adding a page

`lib/docsSite.mjs` lists the documentation pages explicitly. After adding an MDX file to every
locale, add its path to `DOCS_PAGE_PATHS` — `docs-site/lib/__tests__/docsSite.test.js` fails when
the list and the content directory disagree.

Keep anything the metadata routes need inside that module. They execute on the Cloudflare Worker
at request time, where the filesystem and `git` are unavailable — build-time-only data silently
disappears from the deployed output.
