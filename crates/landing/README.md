# Amarcode website

One Astro workspace serves the landing at `/` and the Starlight handbook at
`/docs/`. React islands preserve the interactive landing components; static
sections and documentation render to HTML at build time. Both use one dev
server, one public asset directory, one sitemap, and one production build.

## Develop and build

From the repository root:

```sh
bun install
bun run landing:dev
bun run landing:check
bun run landing:build
bun run landing:preview
```

Open `http://localhost:4321/` or `http://localhost:4321/docs/`. The existing
`docs:dev`, `docs:check`, `docs:build`, and `docs:preview` commands are aliases for
the same website workspace. No second server or proxy is needed.

`landing:build` checks the site and produces `crates/landing/dist/` directly.
Keep the Vercel project rooted at `crates/landing`, with `bun run build` and
`dist` as its output directory. The existing `/install.sh` redirect remains in
`vercel.json`.

Set a different origin for canonical URLs, the sitemap, and social metadata:

```sh
SITE_URL=https://your-domain.example bun run landing:build
```

## Source map

| Path | Purpose |
| --- | --- |
| `src/pages/index.astro` | Landing composition and React hydration boundaries |
| `src/layouts/Landing.astro` | Landing HTML, metadata, and page-scoped CSS imports |
| `src/components/` | Landing sections and interactive React components |
| `src/content/docs/docs/` | Handbook Markdown and MDX, routed under `/docs/` |
| `src/content/docs/404.md` | Site-wide Starlight error page |
| `src/content.config.ts` | Starlight content collection schema |
| `src/components/docs/` | Documentation brand and metadata overrides |
| `src/index.css` | Landing Tailwind styles and tokens |
| `src/styles/docs.css` | Custom Starlight theme |
| `astro.config.mjs` | Astro, React, Tailwind, Starlight, and explicit sidebar |
| `site.config.mjs` | Shared site origin and social image metadata |
| `public/` | Shared fonts, icons, screenshots, and social preview |

The landing's Tailwind stylesheet is imported only by its layout. Starlight
loads its own stylesheet and theme so the two page layouts do not reset each
other's styles.

## Author documentation

Add `.md` or `.mdx` pages under `src/content/docs/docs/`, with `title` and
`description` frontmatter. Add their `docs/…` slugs to the sidebar in
`astro.config.mjs`. Internal handbook links use `/docs/…/` paths.

Starlight supplies navigation, heading links, code highlighting, and Pagefind
search. Search is generated during the production build and is available in
preview; the development server does not generate the search index. The landing
is excluded from the documentation search index.

## Branding and assets

Cormorant Garamond is hosted locally with its OFL license under `public/fonts`.
Geist is used for handbook body text. Agent names and icons are copied from the
ACP registry; the local snapshot is `src/lib/agents.json` and its license is
retained alongside the icons.

The SVG logo, multi-resolution ICO, and Apple touch icon use the desktop's
branding. Both parts of the site share `public/social-preview.png`, a 1200×630
image. `site.config.mjs` provides the same absolute image URL for both layouts,
while each documentation page retains its own title and description.

The closing photograph is [Misty forest covered mountains in soft light](https://unsplash.com/photos/misty-forest-covered-mountains-in-soft-light-2GeemxzkzY8)
by Damien Dufour, under the [Unsplash license](https://unsplash.com/license).

Downloads share a GitHub latest-release request and fall back to the releases
page when assets cannot be fetched. Only published installer formats are shown;
signatures and updater archives are excluded.
