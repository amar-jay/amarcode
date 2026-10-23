---
title: Your first conversation
description: Choose a workspace, select an agent, and start a conversation in Amarcode.
---

## Choose your project

Use the folder control in the prompt composer to select a project directory. This becomes the workspace for the conversation and the working directory for its agent.

For remote connections, enter an absolute directory on the **remote host**, such as `/home/me/projects/my-app`. Your desktop's local folders are not available to that daemon.

## Select an agent

Open the agent picker. Choose an installed agent, or install an agent from the catalog. Complete any sign-in or provider configuration it requires.

You can set a default agent and project folder in **Settings → Agent defaults**.

## Send a prompt

Start with a small, concrete request:

```text
Read this project and explain its entry points. Do not change any files yet.
```

The conversation opens when you send your prompt. As the agent works, Amarcode shows its response and structured activity, including file changes and terminal commands when the agent reports them.

## Respond to requests

An agent may ask for permission before acting, or ask you a question. Read the request and choose a response. Available permission choices come from the agent.

Use the stop control to cancel an active turn. Completed messages remain in the conversation.

## Return to your work

Choose a conversation from the sidebar to reopen its saved messages. You can select another agent for a follow-up. Amarcode first attempts native session resumption and otherwise supplies saved context to the agent.

See [conversation continuity](/docs/concepts/continuity/) for what this preserves and what it cannot restore.
