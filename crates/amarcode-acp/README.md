# Amarcode ACP

`amarcode-acp` is a standalone ACP adapter for OpenAI-compatible chat
completion APIs. It communicates with Amarcode over newline-delimited JSON-RPC
on stdin/stdout and reads provider configuration from a JSON file.

Run it with an explicit configuration file:

```bash
amarcode-acp --config /path/to/amarcode-acp.json
```

`--config` is required.

Minimal standalone configuration:

```json
{
  "name": "openai-codex",
  "provider": {
    "base_url": "https://api.openai.com/v1",
    "api_key": "sk-...",
    "model": "gpt-4.1"
  }
}
```

The desktop manages this runtime file automatically. Saved providers and the
active profile ID live separately in `credentials/amarcode-acp.profiles.json`,
owned by the daemon. The daemon migrates older combined files while preserving
saved credentials, then derives the selected `provider` in `amarcode-acp.json`.
The built-in adapter uses the stable name `amarcode-acp`.

Optional `persistence`, `agent`, `reliability`, and `context` blocks override
runtime defaults; they do not need to be present. Persistence defaults to enabled,
a 24-hour TTL, and a session store beside the config. Agent defaults are a
900-second turn budget and a repeated-result limit of 3. Provider-specific
`reasoning` is optional; OpenRouter enables it by default.

Each user turn has fixed limits of 1,000 tool rounds and 1,000 tool calls.
These limits cannot be overridden by configuration; legacy `agent.max_tool_rounds`
and `agent.max_tool_calls` fields are ignored.

`name` is the stable identifier advertised to Amarcode. It must contain only
lowercase ASCII letters, digits, `.`, `_`, or `-`. The display title is derived
by splitting the name on `.`, `_`, and `-`, then capitalizing each word; for
example, `local_qwen-coder.v2` is displayed as `Local Qwen Coder V2`. This lets
several configured `amarcode-acp` instances advertise distinct identities.

The desktop selects Amarcode Free by default and writes its service URL and
default model into the runtime file. Standalone configurations require
`provider.base_url` and `provider.model`. Dictation does not use this provider block.

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

For custom providers, the adapter attempts to fetch available models from the
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
and elapsed time. Growing context is compacted automatically so the agent can
continue the same task. Cumulative token usage is reported, without a turn token
ceiling; the legacy `agent.max_turn_tokens` setting is ignored. Each completion's
`max_tokens` respects the model output limit and reserved context space. Identical rounds
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

## Slash commands

The adapter advertises `/help` and `/compact` through ACP
`available_commands_update` notifications on session creation, load, and resume.
Send commands as the first text block of a normal `session/prompt` request.

- `/help` lists commands locally without a provider request.
- `/compact` summarizes conversation history using the session's selected model,
  even below the automatic compaction threshold. It preserves the original user
  request verbatim and replaces later exchanges with a bounded summary. The
  summary is persisted and used on subsequent turns; compaction does not run
  tools or continue the task. An empty session reports that there is nothing to
  compact. Provider usage and cost are reported normally.

Both commands take no arguments or attachments. Unknown commands and invalid
arguments return invalid-params errors. Command text and help/status output are
excluded from model history. Additional text blocks supplied by the host for
history restoration are not interpreted as command arguments. Compaction uses
the existing cancellation, deadline, batching, and cost limits; failed or
cancelled summaries leave the previous conversation context intact.

## Tools

OpenAI-compatible function calling is executed as an iterative model → tool →
model loop. The built-in tools are:

* `run_command` runs reads, searches, git, builds, tests, and other executable
  work through the ACP client's terminal service. Couple related steps into one
  call with the platform shell instead of many small calls (for example
  `sh -c "cat hello.txt && git status"` on Linux/macOS, or
  `powershell -Command "..."` on Windows). Its working directory stays inside
  the session workspace, symlink escapes are rejected, and output is bounded.
* `file_command` performs direct UTF-8 text-file work without a shell and needs
  no terminal. With `operation: "read"` it returns a file's contents or lists a
  directory (bounded, and always non-sensitive); with
  `operation: "create"` it creates a file using `content` (replacement requires
  `overwrite: true`); with `operation: "edit"` it replaces exactly one
  occurrence of `old_text` with `content`. Edits fail when the old text is
  absent or ambiguous. Paths must be workspace-relative and parent directories
  must already exist. Create rejects symlink destinations; edit resolves links
  and rejects targets outside the workspace. Successful writes are reported as
  ACP diffs. Models should prefer `operation: "read"` for inspecting files so
  they can navigate a workspace even when terminal access is restricted.

Sessions expose two permission modes: `auto-approve` (default) and `full-access`.
In auto-approve, the model sets `sensitive: false` for routine authorized work;
`sensitive: true` or an omitted declaration requests approval. This relies on
model judgment, not a command sandbox. Full access bypasses approval prompts.
Both commands and file writes follow this policy. Permission dialogs offer an
allow-once choice and a full-access grant; dismissing denies the action.

The adapter signals a non-sensitive declaration to the host through the
terminal request's reserved `_meta` key (`amarcode.sensitive`), so the host
does not additionally require a recorded terminal permission for commands the
agent already auto-approved. This signal is honored only for the built-in
amarcode-acp adapter; a third-party ACP agent cannot set the flag to bypass
host permission checks. Commands that went through a permission dialog keep
the host's exact-command enforcement. Reading files never requests approval.
Modes can be changed through the session's Permissions setting and reset on restore.
File writes run on blocking workers and use synced temporary files plus rename.
Completed tool results are checkpointed so interruption does not forget changes.

Commands use ACP's client-owned terminal lifecycle: `terminal/create`, an
embedded terminal tool-call update, `terminal/wait_for_exit`, `terminal/output`,
and `terminal/release`. Cancellation sends `terminal/kill` before release. The
executable and argument vector are kept separate rather than interpreted by a
shell. Approval follows the selected session mode and the tool sensitivity declaration.

Tool calls are streamed to the client using typed ACP `tool_call` and
`tool_call_update` events. Calls and results remain structured in provider
history so the model can continue after each tool execution. A turn defaults to
30 tool-call rounds and remains cancellable while waiting for the provider,
retry backoff, compaction, terminal execution, or permission.
