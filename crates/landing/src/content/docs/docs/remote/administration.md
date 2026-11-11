---
title: Company policy and revocation
description: Enforce connection requirements, keep security settings across restarts, and revoke lost or compromised devices.
---

This guide is for the person operating the daemon or distributing company
connection profiles. If you received a ready-made profile, start with
[certificates](/docs/remote/certificates/) or [paired E2EE](/docs/remote/end-to-end/).

Policy and signed-CRL features need a build that implements them. Confirm the
capabilities of the daemon and native client you distribute; environment settings
do not add these features to an older binary.

## Install a machine policy

The daemon reads a fixed administrator-controlled file:

| System  | Policy file                                           |
| ------- | ----------------------------------------------------- |
| Linux   | `/etc/amarcode/security.json`                         |
| macOS   | `/Library/Application Support/Amarcode/security.json` |
| Windows | `C:\ProgramData\Amarcode\security.json`               |

For example, require direct TLS with device certificates and revocation lists,
and prohibit managed Cloudflare tunnels:

```json
{
  "allow_managed_tunnels": false,
  "allow_plaintext_loopback": false,
  "require_client_certificate": true,
  "require_certificate_revocation": true,
  "require_e2ee": false
}
```

| Setting                          | Default | Effect                                                                           |
| -------------------------------- | ------- | -------------------------------------------------------------------------------- |
| `allow_managed_tunnels`          | `true`  | Set `false` to reject managed tunnel enrollment and connector startup            |
| `allow_plaintext_loopback`       | `true`  | Set `false` to reject local TCP and WebSocket listeners without outer TLS        |
| `require_client_certificate`     | `false` | Require outer mTLS or paired inner mTLS on the WebSocket listener; reject TCP    |
| `require_certificate_revocation` | `false` | Require signed client CRLs where client-certificate authentication is configured |
| `require_e2ee`                   | `false` | Require the paired inner TLS identity and reject legacy/TCP listeners            |

On Unix, the file and its containing directories must be root-owned and not
writable by group or other users. The policy file must be a regular file, not a
symlink. On Windows, use administrator-controlled write ACLs for the file and its
containing directory. Unknown fields, malformed JSON, unreadable policies, and
invalid permissions fail startup.

There is no environment-variable override for this file. A missing file keeps
the defaults above. These are controls in this executable, not a replacement for
endpoint management, network restrictions, or controls on older/unrelated software.
Restart existing daemon processes after changing policy.

## Require end-to-end encryption

Set `require_e2ee` to `true` and configure the host's
[paired inner identity](/docs/remote/end-to-end/#configure-the-daemon-host).
If `allow_plaintext_loopback` is also `false`, provide outer TLS too: E2EE encrypts
application traffic but does not encrypt the outer loopback HTTP upgrade headers.

The standard shared installation has a local TCP listener. A policy that requires
E2EE on every listener therefore rejects that service. Stop it before running a
custom WSS instance with the same data directory, and start the configured host:

```sh
amarcoded --transport websocket run
```

For an approved direct deployment, configure the interface IP, server certificate,
key, and token as described in [direct TLS](/docs/remote/certificates/#run-a-direct-tls-daemon).
Clients then use that direct `wss://…/rpc` endpoint with their paired profile.
Custom instances are updated through the company's service deployment process,
not the standard daemon updater.

If policy permits a managed tunnel, use a fixed loopback WSS address and attach
its connector separately. In the host's configured environment, set
`AMARCODE_DAEMON_ADDR` and `AMARCODE_WS_ADDR` to the same loopback IP and port, then
run this in a second terminal or separately managed service:

```sh
amarcoded --transport websocket tunnel
```

This needs an existing valid remote installation and the same identity, token,
origin TLS, and E2EE environment as the host. It does not open another database.
Stopping the connector leaves the custom daemon running. If
`allow_managed_tunnels` is `false`, this command is rejected rather than falling
back to another public route.

## Control tunnel enrollment

Anyone who can reach the public installer can otherwise register an installation
and consume one of the allocator's slots. The tunnel Worker gates new
registrations with a shared enrollment code:

```sh
bunx wrangler secret put TUNNEL_ENROLLMENT_CODE --name amarcode-registry
```

| State of the secret | Behavior                                                    |
| ------------------- | ----------------------------------------------------------- |
| Set                 | New installation IDs return `403` without the matching code |
| Deleted             | Enrollment is open again                                    |

Enforcement is exactly the presence of the secret; there is no separate switch.
Existing installations reconnect, update, and delete using their private
installation credential, so setting or rotating the code never locks out a
machine that already registered. Rejected attempts are refused before capacity
is reserved, so they consume no slots and make no Cloudflare API calls.

Give the code to whoever installs machines, and pass it with
`install.sh --enrollment-code` or `AMARCODE_TUNNEL_ENROLLMENT_CODE`. It is stored
per machine in `remote.json` (`0600`). Distribute it the way you would a shared
password, not in project files or shell history.

Before enabling enforcement, make sure the daemons you support can send it:
releases older than the ones that read the code fail with `403` on first
enrollment. Rotating the value is safe because no client keeps it — every
machine reads its own copy from `remote.json` or the environment at enrollment
time. To retire the control entirely, delete the secret and new installs enroll
freely again.

## Persist settings in the right process

Exporting variables in a terminal affects programs launched from that shell. It
does not update an already-running service or desktop process.

For the standard Linux user service, an administrator can configure a drop-in:

```sh
systemctl --user edit amarcode-daemon.service
```

For example, if enabling E2EE on its remote WebSocket listener while retaining
the standard local TCP listener is permitted:

```ini
[Service]
Environment="AMARCODE_E2EE_TLS_CERT=/secure/daemon/server-chain.pem"
Environment="AMARCODE_E2EE_TLS_KEY=/secure/daemon/server-key.pem"
Environment="AMARCODE_E2EE_CLIENT_CA=/secure/daemon/device-ca.pem"
Environment="AMARCODE_E2EE_CLIENT_CRL=/secure/daemon/device-crls.pem"
Environment="AMARCODE_E2EE_SERVER_NAME=daemon.internal.example"
Environment="AMARCODE_E2EE_PROBE_CA=/secure/daemon/server-ca.pem"
```

Then apply it:

```sh
systemctl --user daemon-reload
amarcoded restart
```

The standard service must already be installed, remote access must be enabled,
and any separately running `amarcoded tunnel` process needs the same profile in
its own environment. Configure custom all-WSS instances through their own service
manager. On macOS or Windows, have IT configure the corresponding launch/service
environment rather than assuming a shell export updates it.

For a native desktop profile, fully quit the app and
[launch it from the configured environment](/docs/remote/certificates/#launch-the-desktop-from-that-shell).
Keep certificate paths and profile launchers outside projects and source control.

## Revoke a device

Use this when a device or its client private key is lost or no longer authorized:

1. Have the issuing CA revoke that device's client certificate.
2. Publish an updated signed PEM CRL bundle at the daemon's configured path.
   Replace the file atomically; do not leave a partially written bundle in use.
3. Check that the daemon service user can read it. Keep the CA signing key offline.
4. New handshakes reject the revoked device. Active client connections are
   rechecked every 30 seconds and disconnected, including when RPC work is busy.
5. If the bearer token was also exposed, rotate its file and restart the daemon
   so its loaded token changes. Distribute the replacement to authorized clients.

Revocation closes access; it does not automatically cancel agent work accepted
before revocation. Cancel that work from an authorized client if needed.

Use `AMARCODE_WS_CLIENT_CRL` with `AMARCODE_WS_CLIENT_CA` for direct mTLS, or
`AMARCODE_E2EE_CLIENT_CRL` for paired E2EE. Lists must cover the issuers in the
verified chain. Unknown revocation status, forged signatures, expired
`nextUpdate`, and missing, empty, or malformed configured lists fail closed.
A missing file does not disable checking.

Your CA/distribution service must deliver renewed lists **before** they expire.
Amarcode automatically reloads and checks those files; it does not fetch arbitrary
certificate-advertised URLs or perform online OCSP.

### Revoke a daemon certificate

Native clients can check server-chain CRLs with `AMARCODE_CLIENT_TLS_CRL` for outer
TLS and `AMARCODE_E2EE_SERVER_CRL` for inner TLS. Updates take effect on each new
connection. After emergency server revocation, close/restart existing native
connections too; changing the client-side file does not forcibly close an already
open socket.

### Test with the development CA

From a matching source checkout, the development certificate helper creates an
initial CRL valid for one day. Refresh it without revoking anyone:

```sh
python3 scripts/tls-crl.py --ca-directory credentials/my-dev-tls
```

Revoke the example client and publish a new list:

```sh
python3 scripts/tls-crl.py --ca-directory credentials/my-dev-tls \
  --revoke client-cert.pem
```

These commands require the CA material created by the
[development helper](/docs/remote/certificates/#obtain-or-generate-a-certificate).
They are for testing; use company CA tooling for production devices. Older
example bundles without a CA database/CRL should be replaced with a new output
directory, not overwritten.

## Renew identities and keep trust current

Issue a distinct client identity for every device. Before expiry, distribute its
replacement and restart that native client. Keep the expected daemon identity
name stable while rotating its server key under an approved CA. Coordinate CA
changes on both endpoints; do not teach clients to accept an unexpected issuer
as a connection repair.

Certificate identities currently use the daemon's existing token/permissions
model. They do not create separate per-device project roles. Review company data
handling and agent permissions separately from transport encryption.
