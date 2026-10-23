---
title: Conversation continuity
description: What Amarcode restores when you restart an agent or switch agents, and what stays agent-specific.
---

Amarcode stores visible conversation history independently of an agent's native session. This gives a chat a durable identity even when the agent process changes.

## Native resumption comes first

When starting an agent for an existing chat, the daemon looks for an earlier run of that agent with an ACP session ID and attempts to resume it.

If another agent added messages since that session was last active, Amarcode can supply those intervening messages as additional context with the next prompt.

## Historical context is the fallback

If resumption fails or there is no prior session for that agent, the daemon creates a new session. With your next prompt, it supplies a quoted transcript from the chat.

The transcript identifies the original speakers and separates historical data from your new request. It includes user and assistant text. Assistant activity can contribute a bounded digest of thinking and tool titles; structured tool results and historical attachments are not replayed as native turns.

## What carries over

| Information | Restoration behavior |
| --- | --- |
| User and assistant text | Included in fallback context, subject to its limit |
| Assistant thinking and tools | May contribute a short activity digest |
| Structured tool calls and results | Saved for the UI; not restored as native tool history by the fallback |
| Earlier attachments | Retained for the UI; not replayed by fallback hydration |
| Hidden reasoning, caches, provider state | Cannot be reconstructed by Amarcode |
| Session permissions and configuration | Agent-specific; do not assume they transfer |

## Context has limits

The daemon's fallback transcript currently has a **60,000-byte limit** before its wrapper and the live prompt. It removes the oldest records until the history fits. This is a fixed size bound, not a model-specific token budget or a summary of every omitted turn.

The built-in Amarcode ACP adapter has a separate compaction mechanism for its own provider history. It estimates tokens, prunes large tool output, and can summarize older exchanges while retaining the original and current user requests. Other agents manage their own context differently.

:::tip[Help a new agent pick up the task]
For long tasks, include the current objective, key decisions, and remaining work in your next prompt. Reattach any files or images the agent needs to inspect.
:::

## Continuity is not identical model state

A restored transcript gives the agent an inspectable historical record. It does not reproduce the original model's hidden state or guarantee identical behavior. Review the agent's understanding before relying on it for consequential follow-up work.
