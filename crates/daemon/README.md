# amarcode-daemon

Background service for Amarcode. It stores workspace and chat state in SQLite,
runs ACP coding agents, and exposes RPC and live events to the desktop and CLI.
The daemon runs independently of desktop windows.

## Local development

Run these commands from the repository root:

```bash
bun run daemon:cli install
bun run daemon:cli restart
bun run daemon:cli status --json
bun run app:dev
```

Installation registers the debug executable as a per-user service. Closing the
app leaves that service running. Manage it with:

```bash
bun run daemon:cli start
bun run daemon:cli stop
bun run daemon:cli uninstall
```

Uninstall preserves user data. `bun run daemon:cli purge --confirm-data-loss`
also deletes the platform-default daemon data directory, including chats and
logs. It refuses custom `AMARCODE_APPDIR` paths.

For daemon-only debugging, run in the foreground:

```bash
bun run daemon
```

The desktop expects a registered service. Stop the service before running a
foreground daemon against the same data directory.

## Configuration

| Variable                       | Purpose                                                                                        |
| ------------------------------ | ---------------------------------------------------------------------------------------------- |
| `AMARCODE_APPDIR`              | Data directory; defaults to the platform's Amarcode local app-data directory's `data` child.   |
| `AMARCODE_DAEMON_ADDR`         | Listener address; defaults to `127.0.0.1:43821` for TCP or `127.0.0.1:43721` for WebSockets.   |
| `AMARCODE_STORE_PATH`          | SQLite path; defaults to `{app_dir}/workspace.sqlite3`.                                        |
| `AMARCODE_LOG`                 | Logging filter; falls back to `RUST_LOG`, then `amarcode_daemon=info`.                         |
| `AMARCODE_ACP_REGISTRY_SOURCE` | Agent registry Git URL or local repository; an empty value disables startup synchronization.   |
| `AMARCODE_ACP_RELEASE_URL`     | Built-in Amarcode ACP release manifest URL override; releases still require a valid signature. |

Logs are written to stderr and `{app_dir}/daemon.log`. Environment changes for
an installed service must be applied to its service environment before restart.

## Agents

The daemon refreshes its agent catalog from the ACP registry at startup and
keeps a cached checkout under `{app_dir}/acp-registry` for offline use.
The built-in `amarcode-acp` preset is installed separately from signed releases;
configure its provider and credentials in the desktop's Agent defaults settings.

To initialize the pinned registry checkout for repository development:

```bash
git submodule update --init --recursive
```

## Remote access

Local desktop connections use loopback TCP with one JSON RPC object per line.
For remote connections, use the URL and token from
[remote setup](../landing/src/content/docs/docs/remote/setup.md).
[`amarcoded ssh`](../landing/src/content/docs/docs/remote/ssh.md) opens a shell
on that machine. It is not a desktop connection type.

## Checks and debugging

```bash
cargo check -p amarcode-daemon
cargo test -p amarcode-daemon
cargo test -p amarcode-daemon --test vertical_slice
bun run daemon:test-cli --help
```

The manual test client supports health checks, agent and chat operations,
event subscriptions, and a REPL. Run `bun run daemon:test-cli health` against a
running local daemon.

Client wire types live in the shared [protocol crate](../protocol), rather than
being declared separately by the daemon and desktop. After changing the contract:

```bash
bun run protocol:generate
bun run protocol:check
```

See the [development guide](../landing/src/content/docs/docs/contributing/development.md)
and [protocol guide](../landing/src/content/docs/docs/contributing/protocol.md)
for contributor documentation.


## Persistent public tunnel enrollment

`amarcoded --remote` and `amarcoded tunnel` use a named tunnel by default. The
registry Worker must be configured with Cloudflare account/zone settings; see [allocator setup](../amarcode-registry/README.md#persistent-named-tunnels).
First registration is automatic. The private
`named-installation.json` identity lives beside `remote.json`, outside the data
child directory. Preserve it across restarts; no heartbeat lease is required.
Each installation has a distinct tunnel and stable hostname. Initial capacity is
five installations, including offline ones during a seven-day inactivity grace
period. The Worker checks daily and reclaims disconnected tunnels after that
period. Keep the local identity: it can recover the same hostname without an
manual setup when capacity is available.

Run `amarcoded tunnel --rm` before purging an installation to revoke its named
allocation and free capacity. Public access uses named tunnels exclusively.
`--no-tunnel` continues to bypass public provisioning.
