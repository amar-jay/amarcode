---
title: Conversation continuity
description: What Amarcode recovers after a connection drops, an agent restarts, or you switch agents.
---

Amarcode stores visible conversation history independently of an agent's native session. This gives a chat a durable identity even when the agent process changes.

![One conversation’s activity timeline, with successive runs by codex-acp, grok-build, and amarcode-acp.](/images/workspace/agent-handoff.webp)

## Recover after a desktop disconnect

When the desktop reconnects to a running daemon, it reloads the current session
state, including the active turn and queued approval or input requests. A turn
completed or a request answered by another client while you were offline is
reflected in the recovered view. Reconnecting does not resend your prompt.

[SSH connections](/docs/remote/ssh/) automatically recreate a lost desktop tunnel.
The remote daemon and its agents can continue working while the desktop is
disconnected. Restarting the daemon is different: it interrupts agent processes
and clears in-memory pending requests, while retaining saved conversation history.

## Native resumption comes first

When starting an agent for an existing chat, the daemon looks for an earlier run of that agent with an ACP session ID and attempts to resume it.

If another agent added messages since that session was last active, Amarcode can supply those intervening messages as additional context with the next prompt.

## Historical context is the fallback

If resumption fails or there is no prior session for that agent, the daemon creates a new session. With your next prompt, it supplies a quoted transcript from the chat.

The transcript identifies the original speakers and separates historical data from your new request. It includes user and assistant text. Assistant activity can contribute a bounded digest of thinking and tool titles; structured tool results and historical attachments are not replayed as native turns.

## What carries over

| Information                              | Restoration behavior                                                    |
| ---------------------------------------- | ----------------------------------------------------------------------- |
| User and assistant text                  | Forwarded in fallback context; destination agent applies context limits |
| Assistant thinking and tools             | May contribute a short activity digest                                  |
| Structured tool calls and results        | Saved for the UI; not restored as native tool history by the fallback   |
| Earlier attachments                      | Retained for the UI; not replayed by fallback hydration                 |
| Hidden reasoning, caches, provider state | Cannot be reconstructed by Amarcode                                     |
| Session permissions and configuration    | Agent-specific; do not assume they transfer                             |

## Context has limits

The daemon forwards the full eligible transcript without a fixed byte cutoff. It does not discard older messages to make room for a large recent message. For a resumed session, only messages after that session's watermark are eligible. The destination agent owns its model's context budget and decides whether to compact or reject an oversized prompt.

The built-in Amarcode ACP adapter has a separate compaction mechanism for its own provider history. It uses a configured context window, provider model metadata, or a 128,000-token fallback. It estimates tokens, prunes large tool output, and can summarize older exchanges while retaining the original and current user requests. Restored transcripts arrive as prompt text rather than native historical turns. If that pinned input exceeds the available budget, the adapter returns an explicit error instead of silently discarding it. Other agents manage their own context differently and may also reject oversized prompts.

:::tip[Help a new agent pick up the task]
For long tasks, include the current objective, key decisions, and remaining work in your next prompt. Reattach any files or images the agent needs to inspect.
:::

## Continuity is not identical model state

A restored transcript gives the agent an inspectable historical record. It does not reproduce the original model's hidden state or guarantee identical behavior. Review the agent's understanding before relying on it for consequential follow-up work.
