# Amarcode

Amarcode is a desktop app that lets you work with AI coding agents in one
place. Open a project, choose an agent, and chat with it while it helps you
understand and improve your code.

<img
  src="./assets/amarcode-theme-split.png"
  alt="Amarcode shown in dark and light themes"
  width="100%"/>

<!-- <p align="center">
  <sub>Your coding conversations, carried across agents.</sub>
</p> -->

## What you can do

- Chat with different coding agents.
- Ask questions about your project.
- Follow the agent's progress as it works.
- Review file changes inside the app.
- Leave tasks running in the background without supervision.
- Preserve conversational context semantically across agents

Amarcode uses the open [Agent Client Protocol](https://agentclientprotocol.com/),
allowing it to work with a growing range of compatible agents, including
ChatGPT, Claude, Grok, Antigravity, and Cursor.

## Semantic continuation across agents

One of Amarcode's main advantages is **semantic continuation**. An ACP session
belongs to the agent that created it, and agents differ in how they persist,
resume, compact, or discard their internal state. A native session therefore
cannot be restored portably once that state is unavailable, especially when a
conversation moves between different types of agents.

Amarcode addresses this by treating its own durable chat history as the source
of truth. It stores the visible user and assistant messages independently of an
agent's ACP session ID. When the native session cannot be resumed, Amarcode
creates a new session and supplies the saved transcript as prior context with
the next user prompt. The new agent does not receive the original hidden model
state, but it receives a measurable, inspectable historical record from which
it can continue the conversation. This is the closest portable approximation
to continuity that can be applied consistently across heterogeneous ACP
agents.

### Guarantees and limitations

- Semantic continuation is not native state restoration. Hidden reasoning,
  provider-side conversation state, model caches, permissions, and other
  vendor proprietary data cannot be reconstructed from ACP messages.
- Hydration currently includes non-empty user and assistant text. Structured
  tool calls, tool results, thinking, and prior attachments are retained for
  Amarcode's UI where applicable but are not replayed into the new agent
  session as historical turns.
- Injected history is limited to 60,000 characters. Amarcode removes the oldest
  turns until the transcript fits and marks that earlier conversation was
  omitted.
- The transcript is sent as context in one new user prompt; ACP does not provide
  a portable API for importing arbitrary historical user and assistant messages
  into a session.
- A live prompt can be cancelled, but individual historical messages cannot be
  mutated or deleted through ACP.

In short, Amarcode first uses native ACP resumption when available and uses
local transcript hydration as the interoperable fallback. This lets one durable
conversation survive agent restarts and differences in agent session support
without pretending that opaque native state can be reproduced exactly.

## Project status

Amarcode is currently under active development. Features and supported agents may change
as the project evolves.
