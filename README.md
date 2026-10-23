<p align="center">
  <img src="./crates/application/src-tauri/icons/128x128.png" width="88" alt="Amarcode"/>
</p>

<h2 align="center">Amarcode</h2>
<br/>

<p align="center">
  <img src="./assets/amarcode-dark.png#gh-dark-mode-only" alt="Amarcode in dark theme" width="100%"/>
  <img src="./assets/amarcode-light.png#gh-light-mode-only" alt="Amarcode in light theme" width="100%"/>
</p>

## Advantages

Amarcode uses the open [Agent Client Protocol](https://agentclientprotocol.com/),
allowing it to work with a growing range of compatible agents, including
ChatGPT, Claude, Grok, Antigravity, and Cursor.

One of Amarcode's main advantages is **semantic continuation**. An ACP session
belongs to the agent that created it, and agents differ in how they persist,
resume, compact, or discard their internal state. A native session therefore
cannot be restored portably once that state is unavailable, especially when a
conversation moves between different types of agents.

**How Amarcode keeps a conversation portable?**
Amarcode addresses this by treating its own durable chat history as the source
of truth. It stores the visible user and assistant messages independently of an
agent's ACP session ID. When the native session cannot be resumed, Amarcode
creates a new session and supplies the saved transcript as prior context with
the next user prompt. The new agent does not receive the original hidden model
state, but it receives a measurable, inspectable historical record from which
it can continue the conversation. This is the closest portable approximation
to continuity that can be applied consistently across heterogeneous ACP
agents.

In short, Amarcode first uses native ACP resumption when available and uses
local transcript hydration as the interoperable fallback. This lets one durable
conversation survive agent restarts and differences in agent session support
without pretending that opaque native state can be reproduced exactly.

### Guarantees and limitations

- Hydration currently includes non-empty user and assistant text. Structured tool calls, tool results, thinking, and prior attachments are retained for Amarcode's UI where applicable but are not replayed into the new agent session as historical turns.

- Injected history is limited to the context window of the model. Amarcode compresses the oldest turns until the transcript fits.

- A live prompt can be cancelled, but individual historical messages cannot be mutated or deleted through ACP.


> [!IMPORTANT]
> Semantic continuation is not native state restoration. Hidden reasoning,
> provider-side conversation state, model caches, permissions, and other
> vendor proprietary data cannot be reconstructed from ACP messages.