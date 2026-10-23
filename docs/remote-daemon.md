# Remote daemon over Cloudflare Tunnel

The desktop supports local TCP or a remote authenticated WebSocket endpoint.
Chats, agents, credentials, project files, Git commands, and terminals belong to
the selected daemon host. Closing the desktop leaves its agent processes running.
Switching connections reloads the UI and keeps workspace defaults and chat pins
separate for each endpoint. Save editor drafts before switching.

## Release prerequisites

Deploy the registry Worker and publish a **new daemon release from this source**
before using the public installer URL. The signed manifest must advertise both
`authenticated-websocket` and `remote-cli`. The installer rejects older releases
that cannot enforce token authentication or run `--remote`. The additive
`remote_auth_required` health field lets the desktop check that the remote
service actually enables authentication. Protocol version remains 12.

The Worker serves the Bash installer at `/install.sh`. It requires Linux, Bash,
Python 3, and OpenSSL with Ed25519 support. The registry must contain an artifact
for the host architecture (current publishing defaults include Linux x86_64).
Cloudflared supports both x86_64 and ARM64. No Rust, Bun, root privileges, or
systemd user session is required.

## One-time installation

```bash
curl -fsSL https://updates.amarcode.amarjay.com/install.sh | bash
# Alternatively:
wget -qO- https://updates.amarcode.amarjay.com/install.sh | bash
```

The installer downloads and verifies both binaries, saves a private access token,
and installs `amarcode-daemon` into `~/.local/bin`. It prints the access token
only when first created; subsequent installs preserve it and your chat data.
It does not start a service, daemon, or tunnel. If `~/.local/bin` is absent from
PATH, apply the export command printed by the installer:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

Add this to your shell configuration if your login environment does not already
include that directory. The installer does not edit shell startup files.

To inspect before execution, save the script and run `bash install.sh`.
Optional flags are `--version <daemon-version>`,
`--cloudflared-version <cloudflared-version>`, and `--bin-dir <directory>`.
`--start` additionally starts remote mode immediately after setup.
The earlier `--tunnel` spelling remains an alias for `--start`.

Daemon downloads require a pinned Ed25519 manifest signature plus exact size and
SHA-256 verification. Cloudflared downloads come from the official Cloudflare
GitHub release, checked against its size and SHA-256 digest before execution.
GitHub API rate limits or missing release digests stop installation with an
error. Existing commands outside this installation are never overwritten;
choose another `--bin-dir` if `amarcode-daemon` already exists there.

## Start remote access

```bash
amarcode-daemon --transport websocket --remote
# Equivalent shorthand:
amarcode-daemon --remote
```

Remote mode loads the installed configuration and access token, starts the
daemon on `127.0.0.1:43721`, and waits until its own listener is bound before
spawning `cloudflared`. The command prints a formatted remote WebSocket URL as
soon as Cloudflare assigns one, plus the token file path and connection steps.
Runtime output does not include the token value. Read it from its private file
if you did not save it during installation.

Cloudflare forwards to the loopback origin and terminates public TLS. No inbound
firewall port is needed. Keep the foreground command running while using the
remote connection. Ctrl+C or SIGTERM stops both processes; if cloudflared fails,
the daemon stops with an error so it is clear that remote access is unavailable.
Run the same command again to reconnect. The access token and chats are stable;
the quick-tunnel hostname changes between runs.

Remote mode implies WebSocket transport. Explicit TCP or lifecycle combinations
such as `install --remote` are rejected. It also rejects non-loopback origins,
ephemeral port 0, and origin TLS settings. Ordinary local daemon commands retain
their existing behavior. Custom deployments can override `AMARCODE_REMOTE_ROOT`,
`AMARCODE_APPDIR`, `AMARCODE_DAEMON_ADDR`, and `AMARCODE_WS_TOKEN_FILE`;
`--remote` still requires a fixed loopback HTTP/WebSocket origin.

Files live in `${XDG_DATA_HOME:-$HOME/.local/share}/amarcode-remote/`:

- `remote.json`: installed cloudflared executable location.
- `access-token`: private bearer credential, preserved on reinstall.
- `data/`: remote chats, attachments, agent installations, and credentials.
- `releases/<version>/<sha256>/`: verified daemon binaries and release evidence.
- `cloudflared/<version>/<sha256>/cloudflared`: verified Cloudflare connector.
- `run.sh` / `tunnel.sh`: compatibility helpers; `tunnel.sh` now delegates to the
  same native `amarcode-daemon --remote` lifecycle.

The command in `~/.local/bin` points to the verified installed daemon binary.
You can put the native remote command under your preferred process supervisor
for unattended use. If upgrading an installation made with the earlier service
installer, stop its `amarcode-remote.service` before running the new foreground
command on the same port.

## Connect from Amarcode

Open **Settings → Connection**, or **Connection settings** on the startup dialog:

1. Enable **Connect to a remote daemon**.
2. Enter `wss://<hostname-from-cloudflared>/rpc`.
3. Read `access-token` on the remote host and paste its contents into **Access token**.
4. Choose **Apply and reload**. The app checks authentication, health, and protocol
   compatibility before saving the connection.

The credential is sent in the WebSocket `Authorization: Bearer …` header, never
in a URL. Native desktop storage holds it; the settings API returns only whether
a token is present. On Unix, the settings file is private (0600). All remote
hosts require `wss://`; plain `ws://` is allowed only for local loopback testing.
The daemon retains its browser origin allowlist in addition to token checks.

Remote project selection asks for an absolute directory on the **daemon host**,
e.g. `/home/me/projects/my-app`, and checks that directory through daemon RPC.
Local clipboard attachments are uploaded to that host. Agents and provider keys
are installed/configured on the selected host through the existing settings.

Local installation, repair, and desktop-managed daemon update actions do not
operate on remote services. To update a remote host, rerun the installer (the
access token and data are preserved), then stop and restart `amarcode-daemon
--remote` to use the new binary. Existing chat history remains available; active
agent processes stop when the daemon stops. To return to local use,
uncheck the remote option and apply. A failed remote connection never silently
falls back to the local daemon.

## Permanent Cloudflare hostname

Quick tunnels print a random `*.trycloudflare.com` hostname and run only while
`tunnel.sh`/`cloudflared` is running. Their hostname changes on restart. For
persistent use, create a named Cloudflare Tunnel and run its connector under a
host process supervisor. Configure a public hostname with origin service
`http://127.0.0.1:43721`. Locally managed tunnel configuration has this shape:

```yaml
tunnel: <tunnel-uuid>
credentials-file: /home/me/.cloudflared/<tunnel-uuid>.json

ingress:
  - hostname: daemon.example.com
    service: http://127.0.0.1:43721
  - service: http_status:404
```

Use `wss://daemon.example.com/rpc` in Amarcode. Cloudflare Tunnel carries
WebSocket upgrades automatically. Keep the daemon's bearer authentication
enabled even for named tunnels. Cloudflare Access interactive login and Access
service-token headers are not implemented in this desktop connector; if you add
an Access policy, it must allow this authenticated connection to reach the origin.

Official references: [Tunnel configuration](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/do-more-with-tunnels/local-management/configuration-file/)
and [cloudflared downloads](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/downloads/).

## Running an existing binary manually

Create a private token file containing 32–512 URL-safe ASCII characters (a
64-character random hex string works) and start the daemon:

```sh
umask 077
openssl rand -hex 32 > "$HOME/amarcode-access-token"
AMARCODE_WS_TOKEN_FILE="$HOME/amarcode-access-token" \
  amarcode-daemon run --transport websocket
# In another terminal:
cloudflared tunnel --url http://127.0.0.1:43721
```

Do not bind a public listener without authentication. If you use WSS directly,
keep the existing `AMARCODE_WS_TLS_CERT` and `AMARCODE_WS_TLS_KEY` settings. A token
file with invalid contents or public Unix permissions prevents listener startup.
Rotate the token by replacing its contents and restarting the daemon; reconnect
clients with the new token. Never place tokens in command-line URLs or logs.

To uninstall, stop remote mode and remove the `amarcode-daemon` symlink from
your selected bin directory. Remove the remote data directory separately only
if you want to delete chat history and credentials. Project directories are
outside it.

## Verification

```sh
python3 scripts/test-remote-installer.py
bun run worker:test
bun run worker:check
bun run app:test
bun run tsc
cargo test -p amarcode-daemon
cargo test -p amarcode --lib
bun run protocol:check
```

Installer tests use generated signing keys and isolated temporary directories;
no real service or Cloudflare account is changed. WebSocket tests exercise WS,
WSS, required authentication, origin/path checks, event subscriptions, and
heartbeats. A live end-to-end test requires published artifacts and a networked remote host;
unit/integration tests do not provision a Cloudflare tunnel. Native remote-mode tests use a
fake connector to verify listener readiness, URL output, signal cleanup,
connector failure, and occupied-port handling.
