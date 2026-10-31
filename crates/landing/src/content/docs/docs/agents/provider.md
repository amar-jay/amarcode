---
title: Configure a provider
description: Connect Amarcode ACP to an OpenAI-compatible model provider.
---

The built-in **Amarcode ACP** agent is ready to use with Amarcode Free. You only need these settings if you want to use your own model provider.

## Add a provider

1. Open **Settings → ACP provider** and choose **Add provider**.
2. Give it a name.
3. Enter your provider's **Base URL**, **Model ID**, and **API key**.
4. Choose **Save and use**, then start a new chat.

Use the endpoint and model ID from your provider's documentation. For example, a base URL may include an API prefix such as `https://provider.example/v1`.

## Switch providers

Choose a saved provider from the **Provider** selector. Choose **Use default** to return to Amarcode Free; your saved providers remain available.

**Delete provider** removes a saved provider and its key. Deleting the active provider returns to the default. Provider changes apply to new chats.

## Manage keys

Keys are saved on the computer running the daemon. When connected remotely, you are changing that remote host's provider settings.

Leave the key field blank to keep its saved value. If you change the base URL, enter the key again.

Dictation works without provider setup and is unaffected by these settings. See [attachments and dictation](/docs/workspace/attachments-and-dictation/).
