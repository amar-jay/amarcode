# OpenRouter Worker proxy

Cloudflare Worker with separate configured models for general inference and dictation, and a private OpenRouter token. The proxy enforces the configured model while preserving OpenRouter-compatible endpoints and payloads. JSON request options, transcription uploads, and response streams pass through without SDK-specific translation.

## Configure

Use Bun to install dependencies:

```sh
bun install
cp .dev.vars.example .dev.vars
```

Set your model IDs and OpenRouter token in `.dev.vars` (ignored by Git). Start locally with `bun run dev`.

For production, set `OPENROUTER_MODEL` and `OPENROUTER_TRANSCRIPTION_MODEL` in `wrangler.jsonc`, then configure secrets and deploy:

```sh
bunx wrangler secret put OPENROUTER_API_KEY
bun run deploy
```

`OPENROUTER_MODEL` controls the existing inference routes. `OPENROUTER_TRANSCRIPTION_MODEL` controls `/audio/transcriptions` and is set to `openai/whisper-large-v3-turbo`. The proxy overrides client model selections and removes fallback `models` fields. Each inference route requires its corresponding model variable. Never put real secrets in `wrangler.jsonc` or commit `.dev.vars`.

## Clients

Set your client's OpenAI-compatible base URL to `https://<worker-host>/v1` with no proxy authentication. If your SDK requires an API key, use any placeholder such as `unused`. `/api/v1` is also supported for clients expecting an OpenRouter-style base URL.

```sh
curl http://localhost:8787/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{"messages":[{"role":"user","content":"Hello"}],"stream":true}'
```

OpenAI-compatible JavaScript client configuration (in your client project):

```js
const client = new OpenAI({
  baseURL: 'https://<worker-host>/v1',
  apiKey: 'unused', // SDK placeholder; the proxy does not require authentication.
});
const result = await client.chat.completions.create({
  model: 'worker-configured-model', // Overridden by OPENROUTER_MODEL.
  messages: [{ role: 'user', content: 'Hello' }],
});
```

## Dictation

Send audio to `/v1/audio/transcriptions` (or `/api/v1/audio/transcriptions`) to use the configured Whisper Turbo model:

```sh
curl http://localhost:8787/v1/audio/transcriptions \
  -F 'model=openai/whisper-large-v3-turbo' \
  -F 'file=@dictation.wav' \
  -F 'language=en'
```

The response contains `text` and upstream usage data. Omit `language` for automatic detection. JSON requests can instead provide `input_audio: { "data": "<base64>", "format": "wav" }` alongside `model`. The proxy forwards transcription parameters and upstream errors unchanged. The proxy assigns `OPENROUTER_TRANSCRIPTION_MODEL` regardless of the client’s model field.

## Routes and behavior

Both `/v1` and `/api/v1` support:

| Method | Path after prefix |
| --- | --- |
| POST | `/chat/completions` |
| POST | `/completions` |
| POST | `/responses` |
| POST | `/embeddings` |
| POST | `/audio/transcriptions` |
| GET | `/models` |
| GET | `/embeddings/models` |
| GET | `/providers` |
| GET | `/models/{author}/{slug}/endpoints` |

The proxy does not require client authentication. Upstream requests use the server’s `OPENROUTER_API_KEY`. Browser OPTIONS preflights are unauthenticated, and CORS permits any origin.

Inference requests accept JSON objects with `Content-Type: application/json`. Transcriptions also accept `multipart/form-data`. The Worker assigns `OPENROUTER_TRANSCRIPTION_MODEL` for transcription and `OPENROUTER_MODEL` for the other inference routes, overriding `model` and removing `models`. Other fields, including tools, multimodal content, provider options, and streaming options, pass through. The chosen model must support the requested capability. Catalog responses describe OpenRouter’s catalog.

Account, credits, key administration, and unlisted routes are blocked. Only Accept, HTTP-Referer, and X-Title client headers pass upstream, alongside the Worker’s authorization and the appropriate request content type. Upstream JSON errors and SSE bodies pass through unchanged. Network failures and redirects return generic 502 errors; requests are never automatically retried. Requests are not cached. No per-user quotas or rate limiting are implemented; any caller who can reach this handler can consume upstream credits.

## Checks

```sh
bun run check
bun run build
bun run cf-typegen
```

`build` performs a Wrangler deployment dry run without publishing.
