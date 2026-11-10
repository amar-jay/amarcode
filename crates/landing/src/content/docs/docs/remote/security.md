---
title: Encryption and trust
description: Understand what is encrypted, who can read remote traffic, and when certificates or end-to-end encryption are needed.
---

For the default remote setup, you do not need to generate or install TLS
certificates. Cloudflare manages the public certificate. Your access token is
still required to use the daemon.

## Default Cloudflare connection

The connection travels through these steps:

1. Your client connects to Cloudflare using HTTPS/WSS with TLS.
2. Cloudflare carries traffic through its encrypted tunnel to `cloudflared` on
   your remote host.
3. `cloudflared` forwards to the daemon over `127.0.0.1`. By default, that final
   hop uses plaintext HTTP/WebSocket and stays on the same machine.

**Cloudflare terminates public TLS and can read the unencrypted application
traffic in this mode**, including the daemon access token and project data.
The connection is encrypted across external networks, but it is not end-to-end
private from Cloudflare. Cloudflare describes the encrypted connector connection
in its [Tunnel documentation](https://developers.cloudflare.com/tunnel/troubleshooting/https-origins/).

Anyone who can access your daemon host and its files is also part of your trust
boundary. Network encryption does not protect a compromised client or host.

## Choose the protection you need

| Requirement                                                           | Use                                                                                 |
| --------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| Ordinary remote access with Cloudflare approved as a trusted provider | Default remote setup                                                                |
| No public tunnel; you already have SSH access                         | [SSH forwarding](/docs/remote/ssh/#connect-the-desktop-through-ssh-port-forwarding) |
| A company-managed private endpoint or VPN                             | [Direct TLS](/docs/remote/certificates/#run-a-direct-tls-daemon)                    |
| Require client certificates as well as the access token               | [Direct mTLS](/docs/remote/certificates/#require-a-client-certificate-mtls)         |
| Encrypt the final loopback hop to `cloudflared`                       | [Origin TLS](/docs/remote/certificates/#add-tls-to-the-cloudflare-origin-hop)       |
| Prevent a relay from reading project traffic and the RPC token        | [Paired E2EE with a native client](/docs/remote/end-to-end/)                        |

Origin TLS alone does not hide data from Cloudflare. Paired E2EE adds a separate
TLS 1.3 session between the native client and daemon inside the relay connection.
The relay can still observe routing information, traffic sizes, and timing, and
can interrupt the connection.

## Company networks

Check which access method your organization approves. A certificate does not
permit an otherwise prohibited tunnel or exempt traffic from company policy.
An administrator can [enforce restrictions](/docs/remote/administration/) in the
daemon rather than relying only on client preferences.

On the daemon host's network, Cloudflare Tunnel needs outbound port **7844**:
UDP for QUIC or TCP for HTTP/2. Allowing ordinary HTTPS on port 443 alone may not
be enough. Ask IT to check the approved destinations in
[Cloudflare's firewall requirements](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/configure-tunnels/tunnel-with-firewall/).
Adding a certificate to the loopback hop does not unblock these outbound ports.

On the client's network, the public hostname, WSS connections, or certificate
chain may be restricted by company security controls. Use the approved private
endpoint or contact IT; do not turn off certificate verification to get connected.

## What encryption does not change

Projects and credentials are still available to the daemon and the agents it
runs. Existing storage and model-provider connections are separate from the
client-to-daemon encryption described here. Review the selected
[agent's permissions](/docs/agents/permissions/) and
[provider settings](/docs/agents/provider/) for that remote host.

Browser access uses the browser's own trust and certificate settings. It does not
use the native application's environment-based certificate profile. Paired E2EE
currently requires the native desktop or CLI; an encryption-required daemon
rejects browser ticket requests.
