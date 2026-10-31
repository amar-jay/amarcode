---
title: Conversation continuity
description: What you still have after you close the app, lose a connection, or switch agents.
---

Chats stay saved when you close the app, lose a connection, or switch agents.

![One conversation’s activity timeline, with successive runs by codex-acp, grok-build, and amarcode-acp.](/images/workspace/agent-handoff.webp)

## Reconnect

When the app reconnects to a running daemon, it shows the current chat, including work that is still running. You do not resend the prompt.

Restarting the daemon stops agent work. The saved messages are still there.

## Switch agents

Amarcode tries to continue the agent’s session. If it cannot, or you pick another agent, it sends the saved conversation with your next prompt.

The new agent gets that context. It may not have every detail of the earlier session.

| What | What to expect |
| --- | --- |
| Messages | Saved, and available as context |
| Tool activity | Visible in the app; a new agent may only get a summary |
| Attachments | Saved; attach again if the new agent needs the file |
| Permissions and model | Follow the agent you selected |
| Hidden model state | Does not transfer |

For a long task, say the goal, what you decided, and what is left. If the chat is too large to continue, start a new one with a short handoff.

A [remote terminal](/docs/remote/ssh/#connection-lifetime) is separate. Use `tmux` there if you want the shell to survive a disconnect.
