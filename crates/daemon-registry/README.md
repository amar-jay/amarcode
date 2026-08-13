# Daemon distribution Worker

This Worker exposes versioned `amarcode-daemon` binaries stored in the private
`amarcode-daemons` R2 bucket. It is intentionally read-only; authenticated
publishing happens through Wrangler, never through a public HTTP route.

## Routes

- `GET /health`
- `GET /v1/daemon/latest.json`
- `GET /v1/daemon/latest.json.sig`
- `GET /v1/daemon/:version/manifest.json`
- `GET /v1/daemon/:version/manifest.json.sig`
- `GET /v1/daemon/:version/:rust-target`

`HEAD` is supported for every route. Binary responses support HTTP byte ranges,
ETags, and immutable caching.

## First-time Cloudflare setup

```sh
bunx wrangler login
bunx wrangler r2 bucket create amarcode-daemons
bun run daemon:publish
```

For CI, provide `CLOUDFLARE_API_TOKEN` and `CLOUDFLARE_ACCOUNT_ID` as encrypted
secrets. Scope the token to this account and to the required Workers/R2 edits.

## Publishing

The main command builds Linux and Windows first, uploads both binaries, updates
the version and latest manifests once, then deploys the Worker once:

```sh
bun run daemon:publish
```

Useful variants:

```sh
bun run daemon:publish -- --dry-run
bun scripts/publish-daemon.ts --target x86_64-unknown-linux-gnu
bun scripts/publish-daemon.ts --target x86_64-pc-windows-gnu --skip-deploy
```

The lower-level script accepts repeated `--target` arguments. It builds every
requested target before making Cloudflare changes and advances `latest.json`
only after all binary uploads succeed.

### Cross-compile Windows from Linux

The daemon can be cross-compiled with Rust's Windows GNU target. Install the
target standard library and a 64-bit MinGW toolchain once:

```sh
rustup target add x86_64-pc-windows-gnu
# Debian/Ubuntu
sudo apt-get install mingw-w64
```

Then publish the Windows executable from Linux:

```sh
bun scripts/publish-daemon.ts --target x86_64-pc-windows-gnu
```

The bundled SQLite feature compiles SQLite into the executable. The resulting
daemon imports only standard Windows system DLLs, so no additional runtime files
need to be uploaded alongside `amarcode-daemon.exe`.

The manifest includes the daemon/protocol versions, source commit, byte size,
and SHA-256 of every target. A downloader must verify both `size` and `sha256`
before making the file executable.

Every manifest is signed with Ed25519. The publisher reads the private key from
`AMARCODE_DAEMON_SIGNING_KEY`, falling back to
`~/.config/amarcode/daemon-release-signing-key.pem`. Only the matching public
key is embedded in the desktop application; never commit the private key.

## Desktop application integration

At startup, the Tauri backend first verifies that the native per-user service is
registered. Only then does it accept a compatible daemon health response or ask
the service manager to start it. The desktop never launches a long-lived daemon
child process, including in development, and closing the desktop never stops
the service.

Ordinary bootstrap does not download or install a service. When installation
is missing, the desktop presents an explicit consent action. Only that action
fetches `latest.json`, verifies its Ed25519 signature, selects the platform
artifact, verifies its byte size and SHA-256, and invokes the daemon's
short-lived `install` and `restart` lifecycle commands.

After a successful bootstrap, the desktop performs a best-effort signed update
check in the background. A newer semantically-versioned release with the same
protocol version produces a non-blocking UI notification. Updating always
requires user confirmation because restarting the service interrupts active
agent turns.

The update is staged before the running service is touched: the signature,
artifact size, SHA-256, and lifecycle CLI are all verified first. Activation
then re-registers the versioned executable, restarts the service, and requires
an exact version/protocol health response before caching the new manifest as
current. On failure the previous signed release is re-registered and restarted.
An fsynced activation marker lets the next desktop launch recover the release
selected by the cached manifest if the desktop exits during this transaction.

Installed releases are kept below Tauri's application-local data directory in
`daemon/<version>/<rust-target>/`. The last successfully launched signed
manifest is cached there as well, allowing the application to use an already
verified release when Cloudflare is temporarily unreachable.

Packaged daemon lifecycle is owned by the operating system and is independent
of the desktop application:

- Linux uses `amarcode-daemon.service` under the systemd user manager.
- macOS uses `~/Library/LaunchAgents/com.amarcode.daemon.plist`.
- Windows uses the current user's `Amarcode Daemon` logon task.

Re-registering a release updates the executable path and restarts the managed
daemon. Closing Amarcode does not stop it.

Native registration and lifecycle control live in the daemon binary:
`run`, `install`, `uninstall`, `start`, `stop`, `restart`, and `status`.
`install` registers login persistence without starting the service; the
desktop's explicit installation action invokes `restart` separately.

Runtime overrides useful for development and staging:

- `AMARCODE_DAEMON_SERVICE_EXECUTABLE` selects a daemon binary for short-lived
  `status` and `start` lifecycle commands. It must already be registered as the
  native user service; the desktop never invokes it in `run` mode.
- `AMARCODE_DAEMON_RELEASE_URL` changes the `latest.json` endpoint; its
  signature must be available at the same URL with `.sig` appended and must
  still match the public key compiled into the application.
- `AMARCODE_DAEMON_ADDR` changes the daemon health/RPC address.

For local development, register the debug binary once and let the operating
system own it:

```sh
bun run daemon:install-dev
bun run daemon:restart
bun run tauri:dev
```

Debug desktop builds automatically find `target/debug/amarcode-daemon` for
lifecycle commands. Set `AMARCODE_DAEMON_SERVICE_EXECUTABLE` when using a
binary elsewhere. Use `bun run daemon:status`, `daemon:stop`, or
`daemon:uninstall` to manage the development service explicitly.

For CI publishing, store the private PEM as an encrypted secret, write it to a
temporary file with restricted permissions, set `AMARCODE_DAEMON_SIGNING_KEY`
to that path, and run `bun run daemon:publish`.
