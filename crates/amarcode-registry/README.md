# Amarcode registry Worker

This Worker exposes signed, versioned `amarcode-daemon` and `amarcode-acp`
binaries stored in the private `amarcode-daemons` R2 bucket, and proxies the
desktop application's update manifest. Artifact routes are read-only; authenticated
publishing happens through Wrangler. The Worker also provisions persistent named
tunnels through an authenticated Durable Object.

## Routes

- `GET /health`
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

The desktop bundles the daemon executable as `amarcoded` beside the application. At startup it
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
bun run daemon:cli install
bun run daemon:cli restart
bun run tauri:dev
```

Debug desktop builds automatically find `target/debug/amarcode-daemon` for
lifecycle commands. Set `AMARCODE_DAEMON_SERVICE_EXECUTABLE` when using a
binary elsewhere. Use `bun run daemon:cli status --json`, `daemon:cli stop`, or
`daemon:cli uninstall` to manage the development service explicitly.

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

The remote installer is maintained in `assets/install.sh` and served at
`https://updates.amarcode.amarjay.com/install.sh`. The landing site redirects
`https://amarcode.amarjay.com/install.sh` to this canonical endpoint.

Remote daemons print direct named WSS URLs. The retired `/r/` short-link
service is no longer served.

## Persistent named tunnels

`PUT /v1/tunnels/:installation-id` enrolls or reconnects an installation.
`DELETE /v1/tunnels/:installation-id` permanently revokes it. Both require
`Authorization: Bearer <installation-credential>` (64 lowercase hex characters).
Initial registration is automatic and requires a JSON `{"port":43721}` body. Installation IDs are UUID v4 values generated by the daemon.
Responses contain credentials and always use `Cache-Control: no-store`.

The `TunnelAllocator` Durable Object serializes provisioning and persists each
step before continuing. The default capacity is five registered installations,
including offline and partially provisioned installations. Repeated enrollment
with the same identity resumes provisioning instead of consuming another slot.
Removal deletes DNS, rotates credentials, disconnects connectors, deletes the tunnel, and retains a
hostname tombstone. Failed removal can be retried with the same identity.
A daily Cron Trigger (`0 0 * * *`, midnight UTC) checks Cloudflare's connector
status. After seven days of observed disconnection, it deletes the tunnel and
its DNS record and frees capacity. Healthy, degraded, or unrecognized statuses
reset the timer; API failures skip that installation. Existing records without
an inactivity timestamp receive a full grace period on their first check.
Authenticated allocation requests also reset the timer to allow connector startup.
Daily sampling can miss short connections between checks.

Automatic cleanup preserves the installation credential hash and hostname. A
returning owner can provision a new tunnel with the same identity and hostname,
without manual registration, provided capacity is available. Explicit removal is
permanent and still requires a new installation identity afterward. Cleanup never
force-disconnects connectors. Partial cleanup retains capacity and retries the
following day; reconnects during pending cleanup return a retryable error.
No daemon heartbeat or extra authentication mechanism is required. The schedule
calls the allocator through its private Worker binding, without a public cleanup
endpoint. Logs contain only reclaimed and failed counts.

### Operator configuration

Configure these on the existing Worker before enabling named tunnels:

| Setting | Purpose |
| --- | --- |
| `TUNNEL_CF_API_TOKEN` | Scoped Cloudflare API token with Tunnel Write on the account and DNS Write on the selected zone |
| `TUNNEL_CF_ACCOUNT_ID` | Account holding the tunnels and DNS zone |
| `TUNNEL_CF_ZONE_ID` | DNS zone for machine hostnames |
| `TUNNEL_DOMAIN` | Parent domain; production uses `amarjay.com`, producing `<8-hex-characters>.amarjay.com` |
| `TUNNEL_CAPACITY` | Positive integer, default `5` in Wrangler configuration |

Use a Worker secret for the API token. Other settings may be
Worker variables or secrets. For example, run `bunx wrangler secret put
TUNNEL_CF_API_TOKEN` from this directory, and repeat for the required settings.
Never put credentials in source control. Deploy with the checked-in Durable
Object binding and `tunnel-allocator-v1` SQLite migration.

Production uses first-level names under `amarjay.com`, covered by the zone's
Universal SSL. Do not switch to nested names without provisioning matching edge
certificates. Short names are random eight-character hex labels, checked against
retained installations, migration history, and tombstones. Updated daemons send
`X-Amarcode-Tunnel-Hostnames: short-v2`; older daemons receive compatible
`m-<installation-id>.amarjay.com` names. Existing nested or UUID allocations
migrate on authenticated reconnect without losing the installation credential or
consuming another slot. Migration persists both DNS names until the new route is
configured and the old DNS record has been removed; interrupted migrations retry.
DNS records are proxied CNAMEs targeting each installation's own
`<tunnel-id>.cfargotunnel.com`. No unrelated installations share tunnel tokens.
The allocator does not proxy application traffic or receive daemon access tokens.

Provisioning is not a single Cloudflare transaction. The allocator recovers lost
create responses by finding its deterministic tunnel name and hostname. Retry
failed operations with the same local identity. Do not delete the Durable Object
storage while provisioned resources exist: it holds ownership and capacity state.
Back up each daemon's private `named-installation.json`. If it is lost, the daily
cleanup eventually reclaims its disconnected resources. A new identity cannot
take ownership of the old hostname or credential record.

First-time allocation is public and does not require a shared enrollment secret.
Existing installation updates and deletion require their private credential.
The capacity limit and inactivity cleanup still apply, but public registration
can consume available slots; it is not an admission-control mechanism.

### Daemon enrollment

The connector requires `cloudflared` 2025.4.0 or later for `--token-file`.

```sh
amarcoded --remote
```

The daemon defaults to named tunnels, saves a private persistent identity, and
prints its stable URL only after the public browser ticket endpoint responds with
the correct CORS origin and API version. First-time registration and subsequent
starts use the automatically generated private installation credential. Newly provisioned DNS records include a short propagation
grace period before the daemon probes the public hostname; normal reconnects
reuse the persisted deadline. Connector credentials live in a temporary private file, not
process arguments. The daemon's application access token remains separate.

Stop the connector, then run `amarcoded tunnel --rm` to revoke the allocation
before purging the installation. Stopping a daemon keeps its allocation until
the daily inactivity cleanup reclaims it after the seven-day grace period.
Public access uses named tunnels exclusively; `--no-tunnel` stays loopback-only.
The browser accepts full named WSS URLs without any UI changes.

The five-installation limit is administrative, not a Cloudflare tunnel quota.
An organization must allow the chosen hostnames and the connector's outbound
Cloudflare Tunnel traffic. Named hostnames do not change connector egress needs.
