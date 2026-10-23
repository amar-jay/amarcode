---
title: Updates and troubleshooting
description: Update a remote daemon and resolve connection, authentication, and agent installation problems.
---

## Update a remote host

Rerun the installer on the remote machine:

```bash
curl -fsSL https://amarcode.amarjay.com/install.sh | bash
```

Stop the old `amarcode-daemon --remote` process, then run it again. Reconnect using the newly printed hostname. The installer preserves the access token and data; restarting the daemon stops its active agent processes.

Desktop-managed daemon updates and repairs apply to local services. Remote hosts are updated with their installer.

## Update a local installation

The desktop checks for app and daemon updates separately. Use the update action it offers for each. Daemon updates verify the signed artifact, activate the candidate, and check its exact version and protocol before committing. Failed activation attempts restore the verified previous release.

Managed daemon self-updates currently target supported Linux and Windows services with standard configuration. Custom transports, addresses, data paths, and development binaries can report updates as unavailable.

## The remote connection fails

Check these in order:

1. The foreground remote command is still running on the host.
2. The URL is from the current run and ends in `/rpc`.
3. You entered the access token separately, without query parameters in the URL.
4. The desktop and daemon use the same protocol version.
5. The host and desktop have network access to the tunnel endpoint.

An old quick-tunnel URL cannot be reused after the hostname changes.

## Authentication is rejected

Read the token from the host's `access-token` file and re-enter it. For manually configured hosts, `AMARCODE_WS_TOKEN_FILE` must point to a valid private token file. On Unix, its permissions must exclude group and other access, such as mode `0600`.

If the desktop says authentication is not enforced, install a daemon release with remote support and configure the token on that host. The desktop intentionally rejects an unauthenticated remote daemon.

## An agent will not install

Check `data/daemon.log` inside the remote data directory. Installation failures include a bounded tail of package-command output.

Verify outbound network access and agent platform support. If the installation predates managed Bun and uv support, rerun the current installer and restart remote mode.

## The port is already in use

Remote mode normally uses loopback port `43721`. Stop the previous daemon process before starting another. If you used an earlier installer that registered `amarcode-remote.service`, stop that service before using foreground remote mode on the same port.

## Share a useful issue report

Include your operating system, desktop and daemon versions, the exact error, and relevant log excerpts. Remove access tokens, API keys, private prompts, and sensitive project paths before posting to [GitHub issues](https://github.com/amar-jay/amarcode/issues).
