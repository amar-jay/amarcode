# amarcode-daemon

Background service for Amarcode. Owns durable workspace state, talks to ACP
coding agents as subprocesses, and exposes a local TCP JSON-line API for the
editor / CLI.

> **Scope note for contributors:** the client wire contract lives in the
> workspace's `amarcode-protocol` crate and is shared with the desktop shell.
> Do not re-declare those wire types in either consumer.

---

## What this process is for

```
┌──────────────┐   TCP JSON lines    ┌─────────────────────────────┐
│ Editor / CLI │ ◄─────────────────► │       amarcode-daemon       │
└──────────────┘   (subscribe for    │                             │
                    live events)     │  rpc  →  service  →  store  │
                                     │                 ↘           │
                                     │                   acp (stdio│
                                     │                   JSON-RPC) │
                                     └───────────┬─────────────────┘
                                                 │ spawn / pipes
                                                 ▼
                                        ┌─────────────────┐
                                        │  ACP agent bin  │
                                        │ (claude, copilot│
                                        │  codex, grok, …)│
                                        └─────────────────┘
```

The daemon is the **system of record** for chats, messages, runs, and agents.
The UI is a client: it does not own the agent process or the SQLite file.

---

## Architecture (layers)

Strict layering. Dependencies point **down** only.

| Layer | Path | Responsibility |
|-------|------|----------------|
| **Entrypoint** | `main.rs` | Load config, init logging, build `App`, run until signal |
| **App** | `app.rs` | Owns `Config`, `Store`, event bus; binds TCP and serves |
| **Client RPC** | `rpc/` | TCP accept, one JSON object per line, method dispatch, event subscriptions |
| **Protocol** | `../protocol/` | Versioned client wire types and shared domain enums; generates the React TypeScript contract |
| **ACP protocol** | `protocol/acp_types.rs` | Daemon-private vocabulary for the agent subprocess protocol |
| **Service** | `service/` | Product use-cases (chat CRUD, start run, prompt, fan-out). *Mostly scaffolded* |
| **Store** | `store/` | SQLite persistence (WAL, FKs, migrations) |
| **ACP** | `acp/` | Spawn agent, stdio JSON-RPC, correlate requests, inbound notifications |

### Intended call path

```
TCP line
  → rpc::connection        parse RpcRequest
  → rpc::handler           match method
  → service::*Manager      orchestrate (only join point)
       ├─► acp             talk to agent (no SQL)
       ├─► store           durable write (no ACP, no TCP)
       └─► events bus      live push (after store succeeds)
  → rpc::connection        write result / error / event lines
```

**Rules of thumb**

- `rpc` does not open SQL or spawn agents.
- `store` does not know about TCP or ACP method names as control flow.
- `acp` does not know about chats or UI events.
- `service` is the only place that may join store + ACP + event bus.
- `amarcode-protocol` is the shared client vocabulary; keep it boring and
  increment `PROTOCOL_VERSION` for incompatible wire changes.

### Store-first write path (critical)

`store` and `acp` are **segmented on purpose** — neither imports the other.
That does **not** mean writes are unordered. Orchestration lives in
`service` (especially `session`), and the durability rule is:

> **After ACP produces a meaningful result (or inbound notification),
> persist to the store first, then notify the client.**

Never emit an `EditorEvent` or return an RPC `result` that claims durable
state the SQLite file does not yet contain. Reload / reconnect must match
what the UI already saw.

#### 1. Client-initiated turn (e.g. prompt)

```
RPC prompt(chat_id, text, agent_id)
  │
  ▼ service::session
  │
  ├─ 1. STORE first (intent)
  │     create/update run → starting|running
  │     insert user message
  │     (optional) EditorEvent after those rows commit
  │
  ├─ 2. ACP
  │     spawn/reuse AcpClient, agent.prompt / createSession, …
  │
  ├─ 3. STORE again (outcome of this RPC step)
  │     acp_session_id, run status, raw acp_events row for the request
  │
  └─ 4. RPC result  { run_id, … }
        only after step 3 succeeds for what the result claims
```

Streaming work continues **after** the RPC returns: the prompt call is not
the whole turn. Further agent output is handled on the inbound loop below.

#### 2. Agent-initiated stream (ACP inbound → UI)

Reader thread / event loop receives `AcpInbound` → `session`:

```
AcpInbound::Notification | Request
  │
  ├─ 1. STORE
  │     append acp_events (raw envelope)
  │     upsert messages / message_parts / run status as needed
  │
  ├─ 2. EVENTS  (only if store write succeeded)
  │     EditorEvent::MessageUpdated | RunUpdated | ApprovalRequired | …
  │
  └─ 3. if AcpInbound::Request (permission/input)
        wait for client RPC answer → STORE decision → ACP respond
```

Order for each unit of progress:

```
ACP signal  →  store commit  →  EditorEvent fan-out  →  (RPC reply if any)
```

If the store write fails: **do not** publish the event; log and surface an
error. Prefer a slightly delayed UI over a UI that shows content SQLite never
had.

#### 3. Why this shape

| Concern | Choice |
|---------|--------|
| Isolation | `store` ↔ `acp` stay decoupled modules |
| Join point | only `service` |
| System of record | SQLite wins over in-memory / UI |
| Subscribe sockets | events are a *projection* of stored state |
| Crash mid-turn | restart can rebuild from store + `stop_interrupted_runs` |

#### 4. What is *not* store-first

- Pure reads (`list_chats`, `health`) — store or memory only, no ACP.
- Transport ack for `subscribe_events` — not durable product state.
- In-memory correlation ids inside `AcpClient` — not product state.

---

## Runtime lifecycle

1. **Config** (`config` + `app_dir`)  
   - `AMARCODE_APPDIR` / platform default (`~/.amarcode` on Linux)  
   - `AMARCODE_DAEMON_ADDR` (default `127.0.0.1:43821`)  
   - `AMARCODE_STORE_PATH` (default `{app_dir}/workspace.sqlite3`)  
   - Logging filter: `AMARCODE_LOG` → `RUST_LOG` → `amarcode_daemon=info`

2. **Logging** — stderr + `{app_dir}/daemon.log`

3. **`App::new`**  
   - Open SQLite, apply migrations  
   - Seed preset agents  
   - Mark any leftover `starting`/`running` runs as `stopped`  
   - Create `EditorEvent` broadcast bus  

4. **`App::run`** — bind TCP, accept clients until Ctrl-C / SIGTERM

---

## Client protocol (TCP JSON-line)

One JSON object per line. No HTTP, no length prefixes.

| Direction | Shape |
|-----------|--------|
| Request | `{ "method": "...", "params": { ... } }` (`params` optional) |
| Success | `{ "result": ... }` |
| Failure | `{ "error": "..." }` |
| Live event (after subscribe) | `{ "event": { "type": "...", "payload": { ... } } }` |

### Methods

| Method | Manager | Behavior |
|--------|---------|----------|
| `health` | — | status, daemon version, protocol version, bind addr |
| `version` | — | daemon version and protocol version |
| `subscribe_events` | — | ack, then stream `EditorEvent` lines (`chat_id` / `run_id` / `session_id` filters) |
| `list_agents` | agents | preset + custom agent definitions |
| `create_chat` | chats | `{ workspace_path, title? }` → chat row + `ChatUpdated` |
| `list_chats` | chats | optional `workspace_path` filter |
| `get_chat` | chats | `{ chat_id, include_messages? }` (messages+parts by default) |
| `prompt` | sessions | `{ chat_id, agent_id, text }` → store user msg, ACP turn, return run ids |
| `cancel` | sessions | `{ chat_id }` stop live run |
| `respond_permission` | sessions | answer `ApprovalRequired` (`request_id` + `result` or `error`) |
| `respond_input` | sessions | answer `QuestionRequired` (same params shape) |

### Live events (`EditorEvent`)

Stable **editor-facing** events (camelCase `type` + `payload`), produced by
`session` once wired:

- `chatUpdated`, `runUpdated`, `turnUpdated`, `messageUpdated`, `messagePartAdded`
- `approvalRequired`, `questionRequired`
- `workspaceFilesChanged`, `agentConnectionChanged`

`turnUpdated` is the client signal for “is this prompt still running?”
(`started` → `completed` | `cancelled` | `failed`). Prefer it over `runUpdated`
for composer busy state — a run/session can span many turns.

These are **not** raw ACP notifications. ACP traffic is translated in service.

---

## Persistence (`store`)

SQLite file, embedded migrations under `migrations/`.

| Table | Purpose |
|-------|---------|
| `agents` | Preset + user agent definitions (command, args, env) |
| `chats` | Conversations scoped by `workspace_path` |
| `agent_runs` | One execution of an agent inside a chat |
| `messages` | Chat messages |
| `message_parts` | Structured parts (text, tool call, thinking, …) |
| `acp_events` | Append-only raw ACP JSON-RPC log per run |

`Store` is a `Mutex<Connection>` with table-focused methods in
`agents` / `chats` / `runs` / `messages` / `events`.

---

## ACP layer (`acp`)

Talks to an external agent binary over **stdio**, one JSON-RPC message per line.

```
service::session
        │
        ▼
   AcpClient::spawn(command, args, env, cwd)
        │  stdin  ──► agent
        │  stdout ◄── agent (reader thread)
        │
        ├── request(method, params, timeout)  → correlated result
        ├── notify / respond / respond_error
        └── Receiver<AcpInbound>
              Notification | Request | InvalidMessage | Disconnected
```

Daemon-private ACP method names live in `protocol::acp_types`:

- `AgentRpcMethod` — daemon → agent (`initialize`, `session/new`, `session/prompt`, …)
- `AgentEventMethod` — agent → daemon (`session/update`, `session/request_permission`, …)
- `RpcEnvelope` — in-memory direction + method + JSON payload  
  (persist with `Store::save_acp_envelope` / `AcpEvent::from_envelope`)

Implementation is `acp::client` only (`AcpClient`).

---

## Service layer

| Manager | Role |
|---------|------|
| `agent_manager` | List/save/create agents, resolve executable (`tools_dir` / PATH) |
| `chat_manager` | Create/list/archive/title chats, load history + parts; emits `ChatUpdated` after store |
| `session` | Start run, own live `AcpClient`, prompt/cancel, store-first ACP inbound → `EditorEvent` |

Wired on `App` as `agents`, `chats`, `sessions`. Client RPC methods dispatch into these managers (reads → agents/chats; agent turns → sessions).

---

## Module map

```
src/
  main.rs              process entry
  lib.rs               crate root
  app.rs               App state + run
  config.rs            env config
  app_dir.rs           data directory
  logging.rs           tracing setup
  error.rs             shared Error / Result
  protocol/
    mod.rs             re-exports the shared client protocol
    acp_types.rs       daemon-private ACP methods and envelopes
  rpc/
    server.rs          TCP accept + shutdown
    connection.rs      per-socket read/write + subscribe loop
    handler.rs         method switchboard
  service/             use-cases (scaffold)
  store/               SQLite + row types (protocol enums)
  acp/client.rs        AcpClient
  bin/test-client-cli.rs   manual protocol tester (scaffold)
migrations/
  0001_initial.sql

../protocol/
  src/rpc.rs           client RPC request/response types
  src/events.rs        EditorEvent wire shapes
  src/types.rs         RunStatus, MessageRole, persisted wire models, …
  src/bin/generate-types.rs

../application/src/generated/protocol.ts
                       checked-in generated React contract
```

The protocol crate's drift test compares the generator output byte-for-byte
with the checked-in TypeScript file. Run `bun run protocol:generate` after a
contract change and `bun run protocol:check` in verification/CI.

---

## Related concepts (not duplicates)

These names look similar on purpose; each has one job. Do not merge them.

| Concept | Role |
|---------|------|
| **Client RPC** (`protocol::rpc` + `rpc::*`) | TCP JSON lines to editor/CLI |
| **ACP JSON-RPC** (`AcpClient`) | stdio to agent binary |
| **`EditorEvent`** | Live UI projection after store commit |
| **`AgentEventMethod` / `AcpInbound`** | Raw agent notification/request |
| **`RpcEnvelope` → `AcpEvent`** | In-memory frame → SQLite row (`save_acp_envelope`) |
| **Domain enums** (`RunStatus`, …) | Single vocabulary; store binds `as_str()` / `parse()` at SQL edge |
| **`Error` vs `AcpError`** | Library boundary vs ACP transport errors |

**Service** adds orchestration (store-first + ACP + events). It must not
re-declare parallel enums or re-implement SQL that already lives in `store`.

---

## Implementation status (snapshot)

| Area | Status |
|------|--------|
| Config, logging, app lifecycle | Done |
| TCP server, connection loop, subscribe | Done |
| RPC methods (agents/chats/prompt/cancel/respond) | Done |
| Protocol types + `EditorEvent` | Done |
| Store + migrations + presets | Done |
| `AcpClient` basic handler | Done |
| Service managers | Done |
| Vertical slice e2e (`vertical_slice` + `mock-acp-agent`) | Done |
| Test CLI (`daemon-test-cli`) | Done |

---

## Local run

```bash
# from workspace root
cargo run -p amarcode-daemon

# optional
export AMARCODE_DAEMON_ADDR=127.0.0.1:43821
export AMARCODE_APPDIR=~/.amarcode
export AMARCODE_LOG=amarcode_daemon=debug
```

Smoke:

```bash
echo '{"method":"health"}' | nc -q 1 127.0.0.1 43821
echo '{"method":"subscribe_events","params":{}}' | nc 127.0.0.1 43821
```

### Vertical slice (e2e)

Integration test proves the store-first path with a mock agent:

```text
create_chat → prompt → mock-acp-agent (stdio ACP)
  → SQLite (messages, runs, acp_events)
  → EditorEvent → subscribe_events client
```

```bash
cargo test -p amarcode-daemon --test vertical_slice
```

- `src/bin/mock-acp-agent.rs` — tiny JSON-RPC agent (initialize / createSession / prompt stream)
- `tests/vertical_slice.rs` — boots daemon, registers mock agent, subscribes, prompts, asserts store + events
- `src/bin/test-client-cli.rs` (`daemon-test-cli`) — manual TCP client (see below)

### Manual client (`daemon-test-cli`)

Clap-based subcommands (`--help` on any command).

```bash
# terminal A
cargo run -p amarcode-daemon

# terminal B — full slice with mock agent
cargo build -p amarcode-daemon --bin mock-acp-agent --bin daemon-test-cli
cargo run -p amarcode-daemon --bin daemon-test-cli -- slice \
  --mock-agent ./target/debug/mock-acp-agent \
  --workspace /tmp/amarcode-ws \
  --text "hello mock"

# or individual commands
daemon-test-cli --help
daemon-test-cli health
daemon-test-cli list-agents
daemon-test-cli create-chat -w /tmp/ws -t Demo
daemon-test-cli prompt -c <chat_id> -a mock-acp -t hi
daemon-test-cli subscribe
daemon-test-cli repl
```

---

## Design principles (keep)

1. **Daemon owns state and agents**; clients are disposable.  
2. **Thin RPC, fat service, dumb store, isolated ACP.**  
3. **One JSON object per line** on both TCP and agent stdio.  
4. **Editor events are stable**; ACP is allowed to be messier and is logged raw.  
5. **Migrations are append-only**; do not rewrite applied SQL in place for prod DBs.  
6. **No SQL or agent spawn in `rpc`.**  
7. When something looks duplicated, check [Related concepts](#related-concepts-not-duplicates) before adding another type.
