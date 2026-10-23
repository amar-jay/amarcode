---
title: Development setup
description: Run the Amarcode desktop, daemon, landing page, and documentation from the monorepo.
---

## Get the repository

Install Git, Bun, and a Rust toolchain. Desktop builds also need the native prerequisites for Tauri on your operating system. The desktop release workflow records the toolchain and Linux system packages used for published builds.

```bash
git clone --recurse-submodules https://github.com/amar-jay/amarcode.git
cd amarcode
bun install
```

If you already cloned without submodules:

```bash
git submodule update --init --recursive
```

## Run the desktop

For development with a registered debug daemon:

```bash
bun run daemon:install-dev
bun run daemon:restart
bun run app:dev
```

These commands register and start a real user service. Closing the app leaves it running. Stop it with `bun run daemon:stop` and remove its registration with `bun run daemon:uninstall` when appropriate.

The desktop development hook prepares the bundled debug daemon before launching the renderer. `bun run dev` starts the renderer alone; native features require the Tauri shell.

## Run the daemon directly

```bash
bun run daemon
```

Use this instead of a managed daemon on the same address, not alongside it. Only one process can own the database and listener.

## Run the website

```bash
bun run landing:dev
```

The same Astro server serves the landing at `http://localhost:4321/` and the
handbook at `http://localhost:4321/docs/`. The `docs:dev` command is an alias for
this server; you only need one process.

Build the complete public site with:

```bash
bun run landing:build
```

This checks and builds one Astro project into `crates/landing/dist`, including
the landing, handbook, shared assets, sitemap, and documentation search index.
The existing Vercel project uses this output directly.

## Repository map

| Directory | Responsibility |
| --- | --- |
| `crates/application` | React desktop interface and Tauri shell |
| `crates/daemon` | RPC, sessions, workspace operations, SQLite, updates |
| `crates/protocol` | Shared Rust wire contract and TypeScript generation |
| `crates/amarcode-acp` | Built-in provider adapter and tools |
| `crates/amarcode-registry` | Release Worker, installer, publishing tools |
| `crates/landing` | Astro landing page, React islands, and Starlight handbook |

Continue with [protocol and testing](/docs/contributing/protocol/).
