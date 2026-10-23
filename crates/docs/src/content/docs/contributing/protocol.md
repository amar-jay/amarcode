---
title: Protocol and testing
description: Keep Amarcode's shared contract current and validate changes in the right layers.
---

## Keep the contract in one place

Client wire types live in `crates/protocol`. Both the daemon and desktop shell consume this Rust crate. The React interface uses the generated `crates/application/src/generated/protocol.ts` file.

After changing shared types:

```bash
bun run protocol:generate
bun run protocol:check
```

Do not edit generated bindings by hand. Increment `PROTOCOL_VERSION` for incompatible wire changes. The desktop checks exact protocol compatibility during connection.

## Respect daemon layers

The intended call path is RPC → service → store or ACP. The service layer joins durable persistence, agent traffic, and UI events.

- Keep SQLite concerns in `store/`.
- Keep subprocess protocol framing in `acp/`.
- Keep RPC parsing and dispatch in `rpc/`.
- Persist product state before emitting events that claim it is durable.

Filesystem and Git operations belong to daemon services so they behave consistently for local and remote clients. Native folder picking and clipboard access remain desktop concerns.

## Run relevant checks

```bash
# Frontend and registry tests
bun run test

# Application and registry type checks
bun run tsc
bun run worker:check

# Shared contract and daemon tests
cargo test -p amarcode-protocol
cargo test -p amarcode-daemon

# Built-in adapter, including protocol transcripts
cargo test -p amarcode-acp

# Signed remote installer fixtures
python3 scripts/test-remote-installer.py

# Documentation types, content, and production build
bun run docs:check
bun run docs:build
```

Daemon transport tests bind local sockets, and some package tests need network or writable runtime caches. Use an environment that permits those operations. The live release-download test is ignored by default.

For desktop Rust tests, prepare the daemon sidecar first:

```bash
bun scripts/prepare-desktop-daemon.ts --debug
cargo test -p amarcode --lib
```

## Maintain these docs

Pages live in `crates/docs/src/content/docs/` as Markdown or MDX. Every page needs a `title` and `description` in its frontmatter. Add new pages to the explicit sidebar in `astro.config.mjs`.

Use `/docs/…/` links for documentation pages. Starlight supplies search, heading anchors, a table of contents, code highlighting, and previous/next links. The custom theme is in `src/styles/amarcode.css`.

Verify commands, UI labels, and behavior against the implementation. In particular, distinguish native session resumption, daemon transcript hydration, and the built-in adapter's provider compaction.
