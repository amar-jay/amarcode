# Amarcode landing page

Bun workspace with Vite, React, TypeScript, Tailwind CSS v4, shadcn/ui, and Lucide icons. The page includes a cloud hero, app comparison slider, agent directory, daemon overview, and downloads.

From the repository root:

```sh
bun install
bun run landing:dev
bun run landing:build
bun run landing:preview
```

Type-check and lint:

```sh
bun run --cwd crates/landing check
bun run --cwd crates/landing lint
```

Add shadcn components from this directory:

```sh
bunx shadcn@latest add card
```

Source lives in `src/`, shared UI components in `src/components/ui/`, and the theme in `src/index.css`. The `@/` alias resolves to `src/`. No environment variables are required.

Typography uses locally hosted Cormorant Garamond from Google Fonts (300 and 400), with its OFL license in `public/fonts/OFL.txt`. Agent icons and names are copied from `vendor/acp-registry`; `src/lib/agents.json` records the local snapshot. All icons are retained; quarantined entries are omitted from the page. Registry licensing is retained alongside the icons.

The application logo in `public/images/app/` is copied from `crates/application/src-tauri/icons/icon.svg`. Light and dark screenshots are captured from the actual application renderer with synthetic demo data. The original split-theme reference remains copied from `assets/`. `AppPreview` uses a native, keyboard-accessible comparison slider to reveal the light and dark screenshots. Card, Badge, Separator, Accordion, and Dropdown Menu are installed for future sections.

All page layout, responsive styling, texture overlays, and interaction effects use Tailwind utilities in the components. `src/index.css` contains only Tailwind imports, local font declarations, theme tokens, and the shadcn base layer. Dynamic agent icon URLs are passed as a CSS variable consumed by a Tailwind mask utility.

Closing-section photograph: [Misty forest covered mountains in soft light](https://unsplash.com/photos/misty-forest-covered-mountains-in-soft-light-2GeemxzkzY8) by Damien Dufour on Unsplash, downloaded as `public/images/misty-mountains.webp`. Source: `https://images.unsplash.com/photo-1756817371084-d4433d191d9d`. [Unsplash license](https://unsplash.com/license).

## Downloads and social sharing

The split download button opens an **Other platforms** menu. Both controls share one GitHub latest-release request per page load. Only published installer files are listed (`.exe`, `.msi`, `.AppImage`, `.deb`, `.rpm`, `.dmg`); signatures and updater archives are excluded. Missing installers are labeled unavailable, and **All releases** remains usable if the request fails.

Open Graph and Twitter metadata are included in the built HTML, so link-preview crawlers do not need JavaScript. The public URL is `https://amarcode.amarjay.com`, hosted on Vercel. Metadata uses this URL by default. Override it for another deployment when building:

```sh
SITE_URL=https://your-domain.example bun run landing:build
```

`public/social-preview.png` is the 1200×630 sharing image. Its editable Tailwind source is `scripts/social-preview.html`; open `/scripts/social-preview.html` in the Vite dev server and capture it at a 1200×630 viewport after fonts and images load to regenerate the PNG.

Validate installer filtering with `bun test ./crates/landing/scripts/releases.test.ts` from the repository root.

## Regenerating application screenshots

The screenshot harness renders the real app components. Its isolated API fixture contains a sample Codex/Claude conversation and never connects to a daemon, runs an agent, or writes to real workspace data. Both themes use the same 1600×900 viewport at 2× resolution (3200×1800 PNGs), fixed timestamps, local fonts, and matching scroll position.

```sh
# One-time browser setup:
bunx --cwd crates/landing playwright install chromium
# Terminal 1 — isolated application renderer:
bun run --cwd crates/application dev --port 1421
# Terminal 2 — generate both screenshots:
bun run --cwd crates/landing screenshots
```

For an existing Chromium installation, set `PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH`. Override the renderer URL with `APP_PREVIEW_URL` if needed. `scripts/screenshots/demo-api.mjs` is the editable demo content; it is only substituted by Playwright during capture, never included in the app build. Updating these fixtures does not change app behavior.

After changing the screenshots, rebuild the landing page and regenerate the sharing image with `bun run --cwd crates/landing screenshots:social`.

The public `/install.sh` URL redirects to the registry using the redirect in `vercel.json`. The installer source lives in
`crates/amarcode-registry/assets/install.sh`.

Deploy the registry Worker before deploying this landing page so the redirect
target is available. Verify both endpoints after deployment:

```sh
curl -fsSI https://updates.amarcode.amarjay.com/install.sh
curl -fsSL https://amarcode.amarjay.com/install.sh -o /tmp/amarcode-install.sh
bash -n /tmp/amarcode-install.sh
```
