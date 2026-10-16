# Amarcode ACP

`amarcode-acp` is a standalone ACP adapter for OpenAI-compatible chat
completion APIs. It communicates with Amarcode over newline-delimited JSON-RPC
on stdin/stdout and reads provider configuration from a JSON file.

Run it with an explicit configuration file:

```bash
amarcode-acp --config /path/to/amarcode-acp.json
```

`--config` is required.

Configuration:

```json
{
  "name": "openai-codex",
  "provider": {
    "base_url": "https://api.openai.com/v1",
    "api_key": "sk-...",
    "model": "gpt-4.1",
    "reasoning": { "enabled": true }
  },
  "persistence": {
    "enabled": true,
    "ttl_seconds": 86400,
    "path": "amarcode-acp.sessions.json"
  },
  "agent": {
    "max_tool_rounds": 30,
    "max_tool_calls": 120,
    "max_turn_tokens": 200000,
    "max_turn_seconds": 900,
    "repeated_tool_limit": 3
  },
  "reliability": {
    "connect_timeout_seconds": 10,
    "request_timeout_seconds": 180,
    "stream_idle_timeout_seconds": 45,
    "max_retries": 3,
    "retry_base_ms": 250,
    "retry_max_ms": 10000
  },
  "context": {
    "max_tool_result_bytes": 16384,
    "max_workspace_bytes": 24576,
    "summary_tokens": 1024
  }
}
```

`name` is the stable identifier advertised to Amarcode. It must contain only
lowercase ASCII letters, digits, `.`, `_`, or `-`. The display title is derived
by splitting the name on `.`, `_`, and `-`, then capitalizing each word; for
example, `local_qwen-coder.v2` is displayed as `Local Qwen Coder V2`. This lets
several configured `amarcode-acp` instances advertise distinct identities.

`provider.baseUrl` and `provider.apiKey` are also accepted for compatibility
with camelCase configuration producers. Keep this file private because it
contains the API key.

`provider.reasoning` is optional and is forwarded unchanged to compatible
providers. Reasoning defaults to `{ "enabled": true }` for `openrouter.ai`;
set it explicitly to `{ "enabled": false }` to disable it. Other providers
remain opt-in. Streamed `reasoning_details`, `reasoning`, and
`reasoning_content` are rendered as ACP thought chunks. Structured
`reasoning_details` are reassembled and preserved across tool-call rounds.
Reasoning is presented through an ACP tool-call lifecycle with `kind: "think"`,
including cumulative streamed content and a final completed status.

For every configured provider, the adapter attempts to fetch available models from the
provider's `/models` endpoint and exposes them in the ACP session model
selector. Results are cached for one hour. `provider.model` remains the default
and is the fallback if discovery fails. Model selections are scoped to each
session and persisted with it. Vercel's catalog is filtered to language models.
Discovery also reads each model's `context_length` (or Vercel's `context_window`)
and output token limit. Before every provider
call the adapter estimates prompt size with a bytes/4 heuristic, prunes large
tool results while retaining their beginning and end, then summarizes older
exchanges if necessary. The original request and latest user request stay
pinned, and retained assistant/tool groups remain intact. Summaries include
decisions, changes, verification evidence, and remaining work. Summary requests
are bounded and batched for large histories; individual oversized transcript
messages are explicitly pruned. A failed summary reports an error instead of
silently forgetting earlier work. Successful compaction is persisted with the
session, replacing the summarized exchanges. Space is reserved for the
system prompt, tool schemas, and an 8,192-token completion (capped at a
quarter of the window). When context length is unknown the adapter uses a
128,000-token default. Set `context.context_length` to override discovery or
configure a provider that does not advertise its window. Oversized pinned user
requests report an error instead of being discarded. Token estimates are
approximate; model-specific tokenizers are not bundled. Images use an estimated
4,096-token allowance instead of counting base64 transfer bytes as text tokens.

Every turn discovers bounded workspace context on a blocking worker: root
`AGENTS.md`, README/contribution excerpts, a depth-two file tree, Git HEAD when
available, OS, shell, and UTC timestamp. The policy instructs the agent to read
nested instructions, inspect git status/diffs through the permissioned terminal,
preserve user changes, use targeted edits, and run relevant verification.

## Limits and provider reliability

The `agent`, `reliability`, and `context` sections are optional; partial sections
inherit the defaults shown above. Invalid zero limits/timeouts are rejected at
startup. `repeated_tool_limit` must be at least two and `max_retries` at most ten.

Turn limits cover tool rounds, individual calls (including calls in a batch),
tokens across model and summary requests, and elapsed time. Provider-reported
token usage replaces estimates when available; each completion's `max_tokens`
is capped by the remaining budget and model output reserve. Identical rounds
and alternating two-round loops stop when the configured repetition threshold
is reached with the same arguments and results. Budget stops publish the reason,
return `max_tokens` or `max_turn_requests`, and save completed tool results with
balanced call/result pairs so the next prompt can continue. A time stop signals
cancellation to active terminals and permits up to five seconds for cleanup.

Optionally set `agent.max_cost_usd` to a positive USD amount. This stops further
model requests after reported turn cost reaches the budget. It requires a
provider that supplies `usage.cost`, includes summary cost, and may overshoot by
the last request; it is not a prepaid spending cap. ACP `usage_update` events
carry context usage and cumulative reported session cost. Token totals and the
estimate flag are available under `_meta.amarcode`. Unreported spend, including
requests interrupted before usage arrives, cannot be measured.

Provider requests have connection and total request timeouts plus a stream idle
timeout. Rate limits (429), server errors, and transient request failures are
retried up to `max_retries` times using exponential backoff with jitter. Numeric
and HTTP-date `Retry-After` values are honored; cancellation and the turn
deadline interrupt backoff. Authentication/other client errors are not retried.
If the provider explicitly rejects streaming, a non-streaming request is used.
Published partial streams are never replayed. Streams ending without a finish
reason or `[DONE]` fail before any returned tool calls execute. SSE parsing
preserves UTF-8 characters split across network chunks.

Session persistence is enabled by default with a 24-hour TTL. Unless `path` is
configured, sessions are stored beside the configuration file by replacing its
extension with `.sessions.json`. Relative paths are resolved from the
configuration file's directory. The store contains conversation history,
workspace paths, selected model, and timestamps, but never the provider
API key or tool permission decisions. Set `persistence.enabled` to `false` to
disable it.

## Protocol behavior

The adapter uses the official typed ACP Rust SDK over stdio. Each `session/new`
request creates an independent UUID-addressed in-memory session, and subsequent
requests are routed by their explicit `sessionId`. Prompt turns run concurrently
with protocol input so `session/cancel` can stop an active provider request or
stream and return a `cancelled` stop reason.

With persistence enabled, the adapter advertises ACP session load, resume, list,
delete, and close capabilities. Loading replays stored user and assistant text;
resuming restores context without replay. Closing detaches the live session but
keeps it available until its TTL expires, while deleting removes it immediately.
Image prompts are advertised and translated to OpenAI-compatible image content.
Authentication, audio, embedded context, and MCP capabilities are not advertised.

## Tools

OpenAI-compatible function calling is executed as an iterative model → tool →
model loop. The built-in tools are `read_file`, `list_directory`, `search_text`,
`edit_file`, `write_file`, `move_file`, `delete_file`, and `run_command`.
`edit_file` performs one exact,
uniquely matched text replacement, rejecting stale or ambiguous edits without
changing the file. `read_file` accepts UTF-8 byte `offset` and `limit` values;
partial reads return the exact `next_offset` needed to continue. All paths are
restricted to the session workspace, symlink
escapes are rejected, and output is bounded. Moving never overwrites an existing
destination, and deletion accepts files only. `edit_file`, `write_file`,
`move_file`, `delete_file`, and `run_command` require approval through ACP
`session/request_permission`. Permission prompts offer allow/reject once and
allow/reject for the session. Remembered decisions use exact operation keys:
the target path for writes, and the executable, full argument vector, and
working directory for commands. They are discarded when the ACP session closes.

Commands use ACP's client-owned terminal lifecycle: `terminal/create`, an
embedded terminal tool-call update, `terminal/wait_for_exit`, `terminal/output`,
and `terminal/release`. Cancellation sends `terminal/kill` before release. The
executable and argument vector are kept separate rather than interpreted by a
shell. Amarcode requires a permission matching the exact command, arguments,
and workspace-relative working directory before it starts a process; an
`allow_always` permission remains reusable for that ACP session.

Tool calls are streamed to the client using typed ACP `tool_call` and
`tool_call_update` events. Calls and results remain structured in provider
history so the model can continue after each tool execution. A turn defaults to
30 tool-call rounds and remains cancellable while waiting for the provider,
retry backoff, compaction, terminal execution, or permission.
