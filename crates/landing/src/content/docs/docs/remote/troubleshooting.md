---
title: Updates and troubleshooting
description: Update Amarcode and fix connection or install problems.
---

## Update

Choose **Check for updates**. One dialog covers the app and the daemon you are connected to. On a remote connection, check the machine name before you continue.

A daemon update stops active agent work and remote shells. Saved chats remain. If it fails, read the message and retry.

A foreground daemon does not update itself. Stop it and start it again after you install the new version.

## The connection fails

1. The daemon is still running. In a container, use the [tmux steps](/docs/remote/setup/#codespaces-and-containers-without-systemd).
2. The URL is the current one and includes `/rpc`.
3. The token is from that same machine.
4. The app and daemon are current.

Use the full `wss://…/rpc` URL printed by that host. Normal restarts reuse the
saved hostname; resetting the installation, deleting its identity, or selecting
a different root may change it. Retired short links are not supported.

## Authentication is rejected

Copy the token again from the remote machine. A token from your laptop is not the remote token.

If you use a token file, run `chmod 600` on it.

## Connection refused

The daemon is not listening. A service file or a finished install does not mean the process is running. Start it with `amarcoded --remote`, or with `amarcoded tunnel` only when `amarcoded status` shows it is actually running.

## The port is already in use

Identify whether the registered service or a foreground process already owns
that port or database. `amarcoded status` reports the registered service, not
arbitrary foreground instances. Stop the instance you intend to replace; use
`amarcoded restart` only for the registered service. Do not delete database or
lock files to force a second daemon to start.

See [run without the desktop](/docs/remote/standalone/) for process lifetimes.

## The remote shell fails

Run `amarcoded ssh` from an interactive terminal, with the current URL and token.

- **Unauthorized:** use that machine’s token.
- **Not found:** update the remote daemon.
- **Timeout or connection refused:** the daemon or its remote access is not running.
- **Token file error:** fix the path, or unset `AMARCODE_WS_TOKEN_FILE` and paste the token.
- **Disconnected:** connect again and reattach `tmux` if you were using it.

## The installer says GitHub rate limit

Wait until the time in the error, then run the installer again. If you have `GH_TOKEN` or `GITHUB_TOKEN` set, the installer uses it for the download lookup.

## An agent will not install

Confirm the machine has internet access and retry from the agent picker.

## The daemon stops when I disconnect

Closing the desktop does not stop a remote daemon. Closing the terminal that
launched a foreground daemon can. Start it in tmux or a service as described in
[run without the desktop](/docs/remote/standalone/). A stopped container or host
also stops its daemon; tmux does not survive that.

A foreground `--remote` process owns its connector. If tunnel startup or the
connector fails and the command exits, restart that foreground command after
fixing the connection. A separate `amarcoded tunnel` process instead attaches to
an existing daemon; stopping that connector leaves the daemon running.

## Certificate validation fails

- **Unknown issuer:** use the CA bundle supplied by your administrator. Installing
  a CA into the OS does not necessarily configure the native client; set its
  [native CA path](/docs/remote/certificates/#configure-the-native-client).
- **Name mismatch:** the URL must match the outer server certificate. For E2EE,
  `AMARCODE_E2EE_SERVER_NAME` must separately match the paired inner identity,
  which may differ from the Cloudflare hostname.
- **Expired or not yet valid:** check the device/host clock and ask the issuer
  for a renewed certificate.
- **Missing client certificate:** supply the client certificate and matching
  private key, not the server's key. The daemon access token is still required.
- **Private key permissions:** on Unix, use `chmod 600` on the key. On Windows,
  use the company's private-key ACL procedure.

Do not disable verification or trust a replacement CA received from an unexpected
page to get past an error. See [certificates](/docs/remote/certificates/).

## The app ignores my certificate or E2EE settings

Set variables before launching the **native** application and fully quit its old
process first. Another window may reuse the running process. Launch from that
configured shell; a desktop launcher or existing service may have a different
environment. Confirm the app/daemon build supports the feature.

An E2EE profile needs the paired server CA, expected identity name, and client
certificate/key together. The native connection form should indicate that it
requires paired E2EE. If that indication is absent, check the build and process
environment before submitting the token.

## A paired device is rejected or disconnected

Ask the administrator whether its certificate was revoked or its CRL expired.
The daemon checks new connections and rechecks active clients every 30 seconds.
A missing, malformed, expired, or incorrectly signed configured CRL fails closed;
removing the file does not turn checking off.

The issuer must publish a current signed list covering the appropriate chain.
A revoked device needs a new authorized identity. A successful public readiness
probe only proves the server is reachable; it does not prove your device is
currently authorized. See [revocation](/docs/remote/administration/#revoke-a-device).

## Browser access says a paired native client is required

This is expected for an E2EE-required daemon. Use the
[native desktop or CLI profile](/docs/remote/end-to-end/), not the browser. Native
certificate environment settings do not configure browser trust or provide
browser E2EE.

## Company policy rejects startup or tunnel enrollment

The machine policy is an administrator-controlled file, not a desktop preference.
A strict policy can prohibit the standard local TCP listener, require paired E2EE,
or reject managed tunnels entirely. Ask IT for the approved deployment instead
of changing certificate flags at random. See
[company policy](/docs/remote/administration/).

## The tunnel cannot connect on a company network

Allowing web traffic on port 443 does not by itself allow Cloudflare Tunnel's
outbound port 7844. Ask IT to check approved destinations and TCP/UDP access.
Adding origin TLS does not change that network requirement. See
[company networks](/docs/remote/security/#company-networks).

## Ask for help

Include the app and daemon versions, connection mode, error text, and whether the
host is foreground, tmux, or service-managed. The host's `daemon.log` is in its
configured data directory. Omit access tokens, private keys, and private project
content from logs or screenshots you share.
