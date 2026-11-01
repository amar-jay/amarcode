# Remote web application

The web build reuses the desktop UI and connects directly to an authenticated remote daemon. The static host does not proxy daemon requests or store access tokens.

```sh
bun run web:build
bun run --cwd crates/application web:preview
```

The remote domain stays on Vercel. Set the existing Vercel project’s root to `crates/application`; the checked-in `vercel.json` builds `dist-web` with security headers and SPA routing. Link the repository root to that existing project using `vercel link --project amarcode-remote --scope amarjays-projects`, then deploy with `bun run web:deploy` from the repository root. This requires Vercel authentication and access to the project that owns `remote.amarcode.amarjay.com`. No DNS changes are needed.


For frontend development, use `bun run web:dev` and open `/web.html`. Connect to a daemon with a trusted TLS certificate and explicitly allow the dev page's exact origin using `AMARCODE_WS_ALLOW_ORIGIN`. Production allows `https://remote.amarcode.amarjay.com` by default for ticket authentication. Desktop bearer authentication is unchanged.

The browser exchanges the access token through `POST /web/session` for a 30-second, origin-bound, single-use ticket. The ticket authenticates `/rpc` during the WebSocket handshake. Neither credential goes in a URL. Each RPC uses an independent socket; event subscriptions have their own socket and reuse the application's existing reconnect/snapshot recovery. Uncertain mutations are never automatically retried. Protocol and browser API versions are checked before opening the application.

Workspace defaults and pinned chats are scoped to the normalized daemon URL. Browser access grants the same authority as the daemon token; use only trusted daemon URLs and keep tokens private. The initial web release includes chats, approvals, queues, agents, files, and Git. Native SSH/X11 and an interactive browser terminal are outside this release.
