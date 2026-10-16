# Amarcode landing page

Bun workspace with Vite, React, TypeScript, Tailwind CSS v4, shadcn/ui, and Lucide icons. The page is a placeholder; visual design comes next.

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

Application assets are copied without modification into `public/images/app/`: the logo from `crates/application/src-tauri/icons/icon.svg`, and light, dark, and split-theme screenshots from `assets/`. `AppPreview` uses shadcn Tabs to display light and dark screenshots. Card, Badge, Separator, Accordion, and Dropdown Menu are installed for future sections.

All page layout, responsive styling, texture overlays, and interaction effects use Tailwind utilities in the components. `src/index.css` contains only Tailwind imports, local font declarations, theme tokens, and the shadcn base layer. Dynamic agent icon URLs are passed as a CSS variable consumed by a Tailwind mask utility.
