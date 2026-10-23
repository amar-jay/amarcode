---
title: Configure a provider
description: Connect Amarcode ACP to an OpenAI-compatible model provider.
---

The built-in **Amarcode ACP** agent uses an OpenAI-compatible chat-completion API. You supply the provider endpoint, model ID, and API key.

## Save your provider settings

1. Open **Settings → ACP provider**.
2. Enter the provider's **Base URL**, including its API prefix when required, such as `https://provider.example/v1`.
3. Enter the exact **Model ID** accepted by your provider.
4. Enter your **API key** and save the settings.
5. Select **Amarcode ACP** in the agent picker.

Use your provider's documentation for its endpoint, model IDs, pricing, and account requirements. Chat-completion compatibility alone does not guarantee that a model supports tool calls or images.

## Where the key lives

The daemon stores the provider configuration in its private credentials directory as `amarcode-acp.json`. The UI receives whether a key is present, rather than receiving the saved key back.

With a remote connection, these settings and credentials belong to the **remote daemon**. Switching back to a local daemon uses that host's own configuration.

Leaving the API-key field empty preserves an existing key unless you explicitly clear it.

## Model selection

The adapter attempts to discover models from the provider. When discovery succeeds, its session configuration can expose model choices. If the catalog is unavailable or invalid, the configured model remains the fallback.

Context lengths and completion limits may come from provider metadata; the adapter uses defaults when metadata is absent. It estimates usage when the provider does not report it.

## Dictation uses this provider too

Dictation sends audio to this provider's `/audio/transcriptions` endpoint using the configured API key. It currently requests `openai/whisper-large-v3-turbo`, so the provider must support that endpoint and model even if your chat model works.

[Attachments and dictation →](/docs/workspace/attachments-and-dictation/)
