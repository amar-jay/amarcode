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



## Remote daemon

Run this once on your remote Linux host to install the daemon and `cloudflared`:

```bash
curl -fsSL https://updates.amarcode.amarjay.com/install.sh | bash
```

Or with wget:

```bash
wget -qO- https://updates.amarcode.amarjay.com/install.sh | bash
```

The installer prints your access token, installed paths, and any PATH setup
needed. It installs `amarcode-daemon` in `~/.local/bin` without starting a service
or tunnel. Requires Bash, Python 3, and OpenSSL with Ed25519 support; no sudo,
Rust, Bun, or systemd is needed.

After setup, start remote access whenever you need it:

```bash
amarcode-daemon --transport websocket --remote
# Shorthand:
amarcode-daemon --remote
```

This starts the authenticated daemon and `cloudflared` together, then prints the
remote `wss://<tunnel-host>/rpc` URL. Enter that URL and your saved access token in
Amarcode’s **Settings → Connection**. Ctrl+C stops both processes. Quick tunnel
URLs change each time you restart; your token and chat data are preserved.

See [installation and connection instructions](docs/remote-daemon.md) for PATH
setup, version selection, and permanent hostnames.

The installer endpoint requires the registry Worker to be deployed and a daemon
release with authenticated remote CLI support to be published from this source.
