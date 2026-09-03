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
  "base_url": "https://api.openai.com/v1",
  "api_key": "sk-...",
  "model": "gpt-4.1"
}
```

`baseUrl` and `apiKey` are also accepted for compatibility with camelCase
configuration producers. Keep this file private because it contains the API
key.
