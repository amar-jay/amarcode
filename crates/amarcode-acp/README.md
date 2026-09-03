# Amarcode ACP

`amarcode-acp` is a standalone ACP adapter for OpenAI-compatible chat
completion APIs. It communicates with Amarcode over newline-delimited JSON-RPC
on stdin/stdout and reads provider configuration from a JSON file.

Run it with an explicit configuration file:

```bash
amarcode-acp --config /path/to/amarcode-acp.json
```

If `--config` is omitted, it reads `amarcode-acp.json` from the current
directory.

Configuration:

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

`name` is the stable identifier advertised to Amarcode. It must contain only
lowercase ASCII letters, digits, `.`, `_`, or `-`. The display title is derived
by splitting the name on `.`, `_`, and `-`, then capitalizing each word; for
example, `local_qwen-coder.v2` is displayed as `Local Qwen Coder V2`. This lets
several configured `amarcode-acp` instances advertise distinct identities.

`provider.baseUrl` and `provider.apiKey` are also accepted for compatibility
with camelCase configuration producers. Keep this file private because it
contains the API key.

## Protocol behavior

The adapter uses the official typed ACP Rust SDK over stdio. Each `session/new`
request creates an independent UUID-addressed in-memory session, and subsequent
requests are routed by their explicit `sessionId`. Prompt turns run concurrently
with protocol input so `session/cancel` can stop an active provider request or
stream and return a `cancelled` stop reason.

The adapter currently advertises only the optional `session/close` capability.
Session load, resume, list, delete, authentication, images, audio, embedded
context, and MCP capabilities are not advertised because they are not yet
implemented. Session state remains in memory and is lost when the process exits.
