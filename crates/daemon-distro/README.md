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

At startup, the Tauri backend checks whether a compatible daemon is already
listening. For local development it then prefers `AMARCODE_DAEMON_COMMAND` or a
daemon in the workspace's `target/debug` directory. Packaged applications fetch
`latest.json` and its signature, verify the embedded Ed25519 public key, select
the current platform artifact, and verify both its byte size and SHA-256 before
launching it.

Installed releases are kept below Tauri's application-local data directory in
`daemon/<version>/<rust-target>/`. The last successfully launched signed
manifest is cached there as well, allowing the application to use an already
verified release when Cloudflare is temporarily unreachable. The application
owns the child process it starts and terminates it on application exit.

Runtime overrides useful for development and staging:

- `AMARCODE_DAEMON_COMMAND` launches a specific local executable.
- `AMARCODE_DAEMON_RELEASE_URL` changes the `latest.json` endpoint; its
  signature must be available at the same URL with `.sig` appended and must
  still match the public key compiled into the application.
- `AMARCODE_DAEMON_ADDR` changes the daemon health/RPC address.

For CI publishing, store the private PEM as an encrypted secret, write it to a
temporary file with restricted permissions, set `AMARCODE_DAEMON_SIGNING_KEY`
to that path, and run `bun run daemon:publish`.
