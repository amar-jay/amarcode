# Amarcode desktop backend

The desktop backend adapts the frontend to daemon RPC and native desktop
services. Workspace files, Git operations, chats, and agent execution belong
to the daemon.

## Module boundaries

| Module | Responsibility |
| --- | --- |
| `src/lib.rs` | Assemble plugins, startup hooks, managed dependencies, and the command registry. |
| `src/commands/` | All `#[tauri::command]` adapters, grouped by domain. Preserve frontend arguments, construct shared request types, and delegate. |
| `src/daemon/client.rs` | Typed daemon API, response-envelope unwrapping, and protocol compatibility checks. Accept shared `amarcode-protocol::rpc` parameter types. |
| `src/daemon/bridge.rs` | TCP connections, JSON-line encoding/decoding, and the event-subscription socket. This is the only request-serialization boundary. |
| `src/daemon/manager.rs` | Invokes the bundled daemon CLI for bootstrap/repair/cleanup and observes daemon update jobs. |
| `crates/daemon/src/update/bootstrap.rs` | Daemon-owned installation, startup, repair, and older-daemon update compatibility. |
| `src/app_update.rs` | Native desktop updates. |
| `src/commands/clipboard_files.rs` | Clipboard command, private native read/encoding helpers, and clipboard tests. |
| `src/window.rs`, `src/microphone.rs` | Desktop startup integration. |
| `src/config.rs` | Runtime configuration. |

`commands/` contains `application`, `daemon`, `agents`, `chats`, `sessions`,
`workspace`, `clipboard_files`, and `events`. New commands belong in the corresponding domain,
with their registration added to that domain's group in `lib.rs`.

For a daemon-backed operation, the call path is:

```text
frontend invoke
  -> commands/<domain>.rs       frontend arguments -> shared request struct
  -> daemon/client.rs          typed RPC method and result
  -> daemon/bridge.rs          serialized request -> TCP -> decoded response
  -> amarcode-daemon           domain behavior and durable state
```

Use shared protocol types directly; do not re-declare wire types or manually
construct request JSON in command adapters or client methods. Methods without
parameters send `()` (JSON `null`). Frontend argument shapes may differ from
daemon request shapes; adapt them in the command, preserving the existing IPC
contract.

The client is stateless. Each request uses its own connection so a long-running
prompt does not block cancellation, reads, or permission responses. Event
subscriptions use a separate connection and recheck compatibility on reconnect.

The desktop bundles `amarcode-daemon` as the `amarcode-bootstrap` sidecar.
`bootstrap ensure` checks/starts an existing service; `bootstrap install` performs
first installation or explicit repair, entirely offline. The helper copies the
bundled binary into immutable user-data storage before service registration, so
the service survives desktop updates and never runs from an AppImage mount or
installer directory. It emits shared protocol progress/results as JSON lines.
The desktop has no Rust dependency on the daemon library and no release verifier,
release downloader, or native service registration logic.

Normal updates use `start_daemon_update` and `get_daemon_update` RPC. During RPC
restarts, `bootstrap update-status` reads the durable journal. Older daemons use
the bundled CLI's `check-update` / `start-update` compatibility commands; the
independent native supervisor still owns activation and rollback. Closing the
desktop does not cancel an update job.

Tauri's development/build hooks run `scripts/prepare-desktop-daemon.ts` before
compiling the desktop. The script builds the daemon for Tauri's target and stages
the target-suffixed sidecar required by `externalBin`. For direct Cargo commands,
prepare it first with `bun scripts/prepare-desktop-daemon.ts --debug` from the
repository root (add `--target <triple>` when cross-compiling).

Development uses the same bundled flow. Set `AMARCODE_DAEMON_SERVICE_EXECUTABLE`
explicitly to manage a developer-installed service instead; automatic update
checks are disabled for that override.

Native services report progress and return errors. Command adapters own
publishing terminal failures to frontend status channels, using the common
`commands::report_failure` helper. The events adapter owns translating its
subscription into frontend event and connection-status channels.

Clipboard reads stay on the desktop machine. `read_pasted_files` runs native
file-list lookup, image-file reads, and screenshot PNG encoding in one blocking
task. Copied files take precedence over screenshot formats. Screenshot pixels
are validated and encoded before IPC, so the frontend receives compressed image
bytes rather than raw RGBA. The frontend creates attachment `File` objects from
those bytes; it does not read the native clipboard or encode screenshots.
Screenshot dimensions are limited to 8192 pixels per side and 40 million pixels
total. Compressed screenshots and copied image files share a 10 MB limit.

Keep structural refactors separate from lifecycle scheduling or behavioral
changes. Native window, clipboard, update, and microphone integration can use
Tauri APIs; daemon RPC and transport code must remain independent of Tauri.

## Verification

```sh
bun scripts/prepare-desktop-daemon.ts --debug
cargo check -p amarcode --all-targets
cargo test -p amarcode --lib
cargo clippy -p amarcode --all-targets -- -D warnings
cargo test -p amarcode-protocol
```
