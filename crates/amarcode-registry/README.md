# Amarcode registry Worker

This Worker exposes signed, versioned `amarcode-daemon` and `amarcode-acp`
binaries stored in the private `amarcode-daemons` R2 bucket, and proxies the
desktop application's update manifest. It is intentionally read-only;
authenticated publishing happens through Wrangler, never through a public HTTP
route.

## Routes

- `GET /health`
<!-- - `GET /install.sh` (signed remote-daemon installer bootstrap - moved to landing page) -->
- `GET /v1/daemon/versions.json`
- `GET /v1/daemon/latest.json`
- `GET /v1/daemon/latest.json.sig`
- `GET /v1/daemon/:version/manifest.json`
- `GET /v1/daemon/:version/manifest.json.sig`
- `GET /v1/daemon/:version/:rust-target`
- `GET /v1/acp/versions.json`
- `GET /v1/acp/latest.json`
- `GET /v1/acp/latest.json.sig`
- `GET /v1/acp/:version/manifest.json`
- `GET /v1/acp/:version/manifest.json.sig`
- `GET /v1/acp/:version/:rust-target`
- `GET /v1/app/latest.json`

`HEAD` is supported for every route. Binary responses support HTTP byte ranges,
ETags, and immutable caching.

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
bun run publish -- daemon --target x86_64-pc-windows-gnu --version 0.6.13
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

The desktop bundles a daemon executable as `amarcode-bootstrap`. At startup it
invokes `bootstrap ensure`, which verifies the existing native service and its
managed executable before accepting health or starting the service. It never
performs first installation without the user's install action.

First installation and explicit repair invoke `bootstrap install`. The helper
copies the bundled binary into immutable user-data storage and registers the
copy, so installation works offline and the service outlives desktop package
replacement. The desktop does not download daemon artifacts, verify manifests,
or import the daemon Rust library. Bundled installs have a distinct hash/size
record whose trust originates in the desktop package; downloaded releases still
require the pinned manifest signature.

Normal updates use daemon RPC and durable jobs. For older daemons lacking update
RPC, the bundled CLI checks the signed channel and starts an independent update
supervisor for the exact approved version. That supervisor stages and verifies
the signed artifact, activates it, checks exact version/protocol and native
service health, and commits `current-release.json` atomically. Failure restores
the previous verified executable, including a bundled baseline. Linux systemd
and Windows Task Scheduler keep update jobs alive across daemon replacement and
desktop exit. During RPC restarts, the bundled CLI returns journal status to the
desktop.

All verification and storage live in the daemon's `update/` modules. Signed
releases use `daemon/<version>/<rust-target>/`; bundled copies use
`daemon/bundled/<version>/<sha256>/` (with a fresh instance directory for repair
of a damaged immutable executable). Legacy signed manifest/signature files
remain readable for migration.

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
to that path, and run `bun run publish -- daemon --version <version> --non-interactive`.

## ACP publishing

Changing the ACP version updates both `crates/amarcode-acp/Cargo.toml` and
`Cargo.lock`. Manifests record the source commit, dirty-worktree state,
artifact sizes, and SHA-256 checksums, signed with the Amarcode Ed25519
release key (`AMARCODE_ACP_SIGNING_KEY`, then `AMARCODE_DAEMON_SIGNING_KEY`,
then `~/.config/amarcode/daemon-release-signing-key.pem`).

The published executable remains a configurable ACP adapter. Installation must
pair it with a private provider configuration containing the model, endpoint,
and API key; the public release manifest intentionally contains none of those
credentials.

Remote installer and release requirements: [remote daemon guide](../../docs/remote-daemon.md).
