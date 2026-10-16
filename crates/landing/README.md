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
