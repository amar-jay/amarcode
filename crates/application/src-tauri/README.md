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
| `src/daemon/manager.rs` | Native daemon service installation, startup, updates, rollback, and cleanup. Health requests go through `DaemonClient`. |
| `src/daemon/release.rs` | Signed daemon manifest and artifact verification, caching, and installation. |
| `src/app_update.rs`, `src/clipboard_files.rs` | Native desktop update and clipboard operations. |
| `src/window.rs`, `src/microphone.rs` | Desktop startup integration. |
| `src/config.rs` | Runtime configuration. |

`commands/` contains `application`, `daemon`, `agents`, `chats`, `sessions`,
`workspace`, and `events`. New commands belong in the corresponding domain,
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
cargo check -p amarcode --all-targets
cargo test -p amarcode --lib
cargo clippy -p amarcode --all-targets -- -D warnings
cargo test -p amarcode-protocol
```
