---
title: Conversation continuity
description: What Amarcode recovers after a connection drops, an agent restarts, or you switch agents.
---

Your conversations stay saved when you close the app, lose a connection, or switch agents.

![One conversation’s activity timeline, with successive runs by codex-acp, grok-build, and amarcode-acp.](/images/workspace/agent-handoff.webp)

## Reconnect to your work

When the app reconnects to a running daemon, it restores the current conversation status, including active work and pending requests. You do not need to resend your prompt.

Restarting the daemon interrupts agent work. Saved messages remain available for a follow-up.

## Resume or switch agents

Amarcode tries to resume the agent's earlier session. If it cannot, or you choose another agent, it supplies conversation history with your next prompt.

A restored conversation gives the agent context, but may not include every detail of its earlier session.

## What carries over

| Information | What to expect |
| --- | --- |
| Messages | Saved and available as conversation context |
| Tool activity | Visible in the app; a new agent may receive only a summary |
| Attachments | Saved in the app; reattach files a new agent needs |
| Permissions and model settings | Depend on the selected agent and session |
| Hidden model state | Does not transfer |

## Continue a long task

Include the current goal, key decisions, and remaining work in your next prompt. Reattach relevant files and ask the agent to confirm its understanding.

Agents have context limits. If a conversation is too large to continue, start a new chat with a concise handoff.

[Remote terminal sessions](/docs/remote/ssh/#connection-lifetime) have a separate lifetime; use `tmux` to keep terminal work running across disconnects.
