---
title: Choose a remote connection
description: Decide how to run Amarcode on another computer and which connection is right for you.
---

Run the daemon on the computer that has your project. Connect to it from the
Amarcode desktop, a supported browser, or `amarcoded ssh`. The desktop does not
need to be installed or open on the remote computer.

Files, Git operations, agents, chat history, and provider settings belong to that
remote computer. Closing the desktop disconnects your view; it does not stop a
running daemon or its agent work.

## Start here

| What you want                                                           | Follow this guide                                                                        |
| ----------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| Use a Linux server, Codespace, or development container                 | [Set up a remote host](/docs/remote/setup/)                                              |
| Keep the daemon running without the desktop or after closing a terminal | [Run without the desktop](/docs/remote/standalone/)                                      |
| Connect your desktop or browser to a running host                       | [Connect to your host](/docs/remote/connect/)                                            |
| Use an existing SSH account instead of a public tunnel                  | [SSH port forwarding](/docs/remote/ssh/#connect-the-desktop-through-ssh-port-forwarding) |
| Connect through a company VPN or private network                        | [Certificates and direct TLS](/docs/remote/certificates/#run-a-direct-tls-daemon)        |
| Keep the relay from reading project traffic                             | [Paired end-to-end encryption](/docs/remote/end-to-end/)                                 |
| Apply company restrictions or revoke a device                           | [Company policy and revocation](/docs/remote/administration/)                            |

## The usual setup

On a Linux host, [install the daemon](/docs/remote/setup/), then run:

```sh
amarcoded --remote
```

A new installation may need the operator's
[enrollment code](/docs/remote/setup/#enrollment-code) before it can register a
public hostname. Ask for it if enrollment is refused; later starts do not need it.

Keep that process running. It prints a Remote URL and access token. In the desktop,
open **Settings → Connection**, select **Remote daemon**, and use those values.
You do not need to generate certificates for the default Cloudflare tunnel.

`amarcoded` and `amarcode-daemon` refer to the daemon executable. `amarcode` is the
desktop application; use `amarcoded --remote` for the remote-host commands in
these guides.

## Understand the connection you choose

| Connection                                      | What protects the connection?                                                    | Browser access                                        |
| ----------------------------------------------- | -------------------------------------------------------------------------------- | ----------------------------------------------------- |
| Default Cloudflare tunnel                       | Public TLS and an encrypted tunnel; Cloudflare can read traffic                  | Yes, with a browser-enabled daemon                    |
| SSH port forwarding                             | Your existing SSH connection; the forwarded daemon listener stays on loopback    | Use the native desktop or CLI                         |
| Direct TLS or mTLS                              | A verified server certificate; mTLS also requires a client certificate           | Requires browser/OS certificate setup when applicable |
| Paired E2EE through a tunnel or direct endpoint | An additional authenticated TLS 1.3 session between the native client and daemon | Not supported                                         |

See [encryption and trust](/docs/remote/security/) before choosing a mode for
company projects. A public `wss://` URL alone does not mean the relay cannot read
the traffic.

:::note[Advanced security features]
Paired E2EE and automatic CRL checks require daemon and native-client builds that
include those features. If your downloaded release does not include them yet,
use the matching build supplied by your administrator. Configuring environment
variables on an older build does not add encryption support.
:::
