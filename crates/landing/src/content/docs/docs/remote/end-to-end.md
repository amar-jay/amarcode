---
title: Paired end-to-end encryption
description: Connect a native desktop or CLI so a relay cannot read the daemon token, project traffic, or terminal stream.
---

Paired E2EE adds a mutually authenticated TLS 1.3 session between your native
client and the daemon, inside the WebSocket connection. Cloudflare or another
relay can carry the connection without decrypting the daemon's RPC token,
project requests, attachments, events, or terminal traffic.

It requires daemon and native-client builds with E2EE and CRL support. Use the
matching build provided by your administrator if these features are not yet
available in your downloaded release.

:::caution[Native clients only]
Use the native Amarcode desktop or `amarcoded ssh`. Browser E2EE is not supported.
Do not paste this profile's private keys or token into the hosted browser client.
An encryption-required daemon rejects browser ticket requests and legacy remote
connections; it does not silently downgrade them.
:::

## Get your device profile

Your administrator should give you:

1. The daemon's Remote URL and access token.
2. Its public server CA bundle and **expected identity name**.
3. Your device's client certificate and private key.
4. A server CRL bundle, if the organization requires client-side server revocation checks.

Verify the profile through an approved channel independent of the relay. Do not
accept a replacement CA merely because the server or a browser page asks you to.
Each device should have its own certificate and private key so it can be revoked
without sharing another user's identity. The paired CA is also trusted to issue
identities; a party controlling its signing key can issue certificates that your
profile trusts. Keep that authority separate from a relay you intend to exclude.

The expected identity is a SAN in the **inner** server certificate, such as
`daemon.internal.example`. It can differ from the public Cloudflare hostname in
your Remote URL. Keep the exact value your administrator supplied.

## Configure your native desktop

Store the files outside your project. On Linux/macOS, in the shell that will
launch the desktop:

```sh
export AMARCODE_E2EE_CA=/secure/device/server-ca.pem
export AMARCODE_E2EE_SERVER_NAME=daemon.internal.example
export AMARCODE_E2EE_CLIENT_CERT=/secure/device/device-chain.pem
export AMARCODE_E2EE_CLIENT_KEY=/secure/device/device-key.pem
chmod 600 "$AMARCODE_E2EE_CLIENT_KEY"
```

If supplied, also set:

```sh
export AMARCODE_E2EE_SERVER_CRL=/secure/device/server-crls.pem
```

On Windows PowerShell:

```powershell
$env:AMARCODE_E2EE_CA = 'C:\secure\server-ca.pem'
$env:AMARCODE_E2EE_SERVER_NAME = 'daemon.internal.example'
$env:AMARCODE_E2EE_CLIENT_CERT = 'C:\secure\device-chain.pem'
$env:AMARCODE_E2EE_CLIENT_KEY = 'C:\secure\device-key.pem'
```

Restrict the key's Windows permissions to your account. Fully quit the desktop,
then [launch its executable from that same shell](/docs/remote/certificates/#launch-the-desktop-from-that-shell).
These settings are process-wide; a running app does not acquire them when you
open a new window.

In **Settings → Connection**:

1. Choose **Remote daemon**.
2. Enter the supplied Remote URL and token.
3. Confirm that the connection form says the native profile requires paired
   end-to-end encryption. If that indication is missing, stop and check the build
   and launch environment before submitting the token.
4. Select **Verify and apply**.
5. Open a project using its absolute path on the daemon host.

The native client verifies the paired server name and CA and presents your device
identity before sending the bearer token inside the encrypted session. If any
step fails, it does not retry with an ordinary token-only connection.

The regular `AMARCODE_CLIENT_TLS_*` variables configure the **outer** TLS
connection if your direct endpoint needs them. They do not replace the
`AMARCODE_E2EE_*` profile above.

## Use the CLI

Set the same E2EE variables in your CLI shell, then run:

```sh
amarcoded ssh wss://your-remote-host.example/rpc
```

Paste the daemon token at the hidden prompt, or use
[`--token-file`](/docs/remote/ssh/#use-a-token-file). The terminal stream uses the
same paired encrypted connection. `amarcoded ssh` still does not start the daemon.

## Configure the daemon host

This section is for the operator. First prepare the normal
[remote installation](/docs/remote/setup/) or an approved
[direct TLS endpoint](/docs/remote/certificates/#run-a-direct-tls-daemon).
Provide a server identity and a trusted device CA with a current signed CRL:

```sh
export AMARCODE_E2EE_TLS_CERT=/secure/daemon/server-chain.pem
export AMARCODE_E2EE_TLS_KEY=/secure/daemon/server-key.pem
export AMARCODE_E2EE_CLIENT_CA=/secure/daemon/device-ca.pem
export AMARCODE_E2EE_CLIENT_CRL=/secure/daemon/device-crls.pem
export AMARCODE_E2EE_SERVER_NAME=daemon.internal.example
export AMARCODE_E2EE_PROBE_CA=/secure/daemon/server-ca.pem
amarcoded --remote
```

Use absolute paths. The server key must be private to the service user. The
client CA and signed client CRL are mandatory for E2EE mode. Configuring these
server settings makes paired encryption mandatory on its WebSocket listener;
ordinary remote clients and browser tickets are rejected.

`PROBE_CA` lets the daemon authenticate its encrypted public readiness check.
If omitted it uses the client CA, which only works if that same CA issued the
server identity. The probe returns status/encryption capability, has no project
or terminal access, and sends no bearer token through a public HTTP endpoint.
It checks reachability, not whether a particular device is authorized.

The mode works for a foreground daemon without a desktop, an isolated remote
instance, or a service-managed host. Set the environment on the process that
actually runs the host. For a service, use its service-manager configuration and
restart it; exporting variables in a different terminal does not update it.

Normal shared `--remote` mode retains its existing local TCP listener. To require
E2EE on **all** listeners, use the stricter
[company policy and custom service setup](/docs/remote/administration/#require-end-to-end-encryption).
If policy also prohibits plaintext loopback headers, combine E2EE with
[origin TLS](/docs/remote/certificates/#add-tls-to-the-cloudflare-origin-hop).

## Pair, renew, and revoke devices

Pairing means issuing a device certificate from an approved CA and distributing
the daemon CA/name and token through your trusted enrollment process. There is
no automatic trust-on-first-use prompt. Keep the issuer's private key offline.
For production, use the company CA; the repository's
[development helper](/docs/remote/certificates/#obtain-or-generate-a-certificate)
is for local testing.

Before expiry, renew a device's certificate and key and restart its native client
with the updated paths. Server identity files are reloaded for new inner TLS
connections. CA changes require coordinated trust updates on both endpoints.

To revoke a lost device, ask the issuer to revoke its certificate and publish a
new signed CRL. The daemon checks new handshakes immediately and rechecks active
clients every 30 seconds. See [revocation steps](/docs/remote/administration/#revoke-a-device).

## Limits

The relay still sees hostnames, connection timing, and traffic volume, and can
interrupt service. Tunnel provisioning credentials remain part of the provider's
control plane. The daemon, its agents, local storage, and model-provider requests
remain separate trust boundaries; E2EE does not hide data from the daemon host.

Use only a native client build you trust. End-to-end encryption cannot protect
secrets from a compromised client or a modified application that already handles
the plaintext. Device certificates currently share the daemon's existing
permissions model; they do not introduce per-device project roles.
