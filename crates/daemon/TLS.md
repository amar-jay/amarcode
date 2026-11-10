# TLS, certificates, and company deployment

Amarcode supports three configurations. Existing installations keep their current
behavior unless explicitly configured or restricted by machine policy.

| Configuration | Encryption | Trust boundary |
| --- | --- | --- |
| Paired E2EE over WSS | Inner mutually authenticated TLS 1.3 carried through the relay; token and all application frames encrypted | Paired native client and daemon; relay sees routing and traffic metadata |
| Direct WSS | Client to daemon TLS; optional mandatory client certificates | The daemon and client machines; any approved TLS inspection still applies |
| Cloudflare with origin TLS | Client to Cloudflare TLS, encrypted tunnel, verified TLS from cloudflared to the daemon | Cloudflare terminates client TLS and can read RPC, tokens, source, and terminal traffic |
| Default Cloudflare | Same external encryption; plaintext final hop over loopback | Cloudflare and the host machine |

Origin TLS is not application-level end-to-end encryption. An administrator must
approve the deployment and the handling of project data. No settings here bypass
company firewalls, approve a tunnel, or install a CA into the OS/browser trust store.

## Generate certificates

OpenSSL and Python 3 are required. From the repository root:

```sh
python3 scripts/tls-certificates.py dev-ca \
  --output credentials/my-dev-tls --hostname localhost --hostname 127.0.0.1
```

This creates a private development CA, a server certificate with the requested
SANs, and a client certificate. Leaf certificates expire after 30 days; the CA
expires after 365 days. Output directories must be new. On Unix, directories and
keys are private to the generating user. On Windows, protect their ACLs before use.
`credentials/` is ignored by Git. Keep `ca-key.pem` offline; deploy only the CA's
public certificate and the appropriate endpoint identity. Do not share one client
private key among users or devices. Generate distinct company-issued identities.

For a company CA, generate a server key and CSR instead:

```sh
python3 scripts/tls-certificates.py csr \
  --output credentials/company-request --hostname daemon.internal.example
```

Send `server.csr` to IT; never send `server-key.pem`. Request a serverAuth
certificate with these SANs, its intermediate chain, and the trusted root CA.
Client identities need clientAuth certificates issued by the approved client CA.
Use company certificate enrollment and renewal procedures for production.

## Direct TLS/mTLS

Run on the approved host/interface, using absolute paths (replace examples):

```sh
export AMARCODE_DAEMON_ADDR=10.0.0.20:43721
export AMARCODE_WS_TOKEN_FILE=/secure/amarcode/access-token
export AMARCODE_WS_TLS_CERT=/secure/amarcode/server-chain.pem
export AMARCODE_WS_TLS_KEY=/secure/amarcode/server-key.pem
export AMARCODE_WS_CLIENT_CA=/secure/amarcode/client-ca.pem
cargo run -p amarcode-daemon --bin amarcode-daemon -- --transport websocket run
```

The access-token file remains required: use an existing private Amarcode token,
or generate a private token with `openssl rand -hex 32` under `umask 077`. Every
client must present a valid certificate chained to `AMARCODE_WS_CLIENT_CA` and
still authenticate with the bearer token. Omitting the client CA selects server
TLS without mTLS. No anonymous-client fallback is enabled when a client CA is set.
Non-loopback listeners always require TLS.

For the native desktop client or `amarcoded ssh`, set these before launching the
client process:

```sh
export AMARCODE_CLIENT_TLS_CA=/secure/client/server-ca.pem
export AMARCODE_CLIENT_TLS_CERT=/secure/client/client-chain.pem
export AMARCODE_CLIENT_TLS_KEY=/secure/client/client-key.pem
```

Connect to `wss://daemon.internal.example:43721/rpc`. Its hostname must match the
server certificate's SAN. These process-wide client settings apply to native
remote connections; use a dedicated process/profile for the company deployment.
Explicitly configured files must load successfully; verification is never disabled.
The browser client uses the browser/OS certificate and trust configuration instead.
Private keys and CA paths are not sent into the webview.

Direct custom instances currently do not use the standard managed daemon's
self-update workflow. Deploy and restart them through the company's service
manager. Keep certificate/key paths and these settings in that service's environment.
Machine-policy changes require a daemon restart. Configured signed CRLs and their
trust anchors are reloaded for every new handshake and checked against active
client sessions every 30 seconds. Inner TLS identities are also reloaded for each
new connection. Without CRL configuration, outer TLS identity changes still need a
restart. mTLS identities do not yet have separate per-device authorization roles.
See the revocation section below for fail-closed CRL behavior and distribution.

## Machine policy

Administrators can place JSON at the fixed machine location:

- Linux: `/etc/amarcode/security.json`
- macOS: `/Library/Application Support/Amarcode/security.json`
- Windows: `C:\ProgramData\Amarcode\security.json`

Example for direct mTLS only:

```json
{
  "allow_managed_tunnels": false,
  "allow_plaintext_loopback": false,
  "require_client_certificate": true
}
```

Missing fields keep defaults (`true`, `true`, `false` respectively). Optional
`require_e2ee` and `require_certificate_revocation` default to `false`. Unknown
fields and unreadable/malformed existing policies fail startup. Unix policy files
must be root-owned, not group/other writable, and not symlinks. Administrators must
also protect the containing directory; on Windows protect the file/directory with
administrator-only write ACLs. No environment variable selects an alternative
policy file. These settings restrict this daemon executable, not arbitrary older
binaries or unrelated tunneling software; enforce executable/network controls
through the company's endpoint-management system as needed.

`allow_plaintext_loopback: false` rejects local TCP too. The normal desktop-managed
TCP service will therefore fail closed; use a separately managed direct WSS
instance and configure the desktop to connect remotely. `require_client_certificate`
requires outer or paired inner mTLS on all WebSocket listeners. Direct origin mTLS
is incompatible with the managed connector, but inner mTLS can traverse it.
`require_e2ee` rejects every TCP listener and every WebSocket listener without the
paired inner TLS identity. `require_certificate_revocation` requires signed CRLs
wherever client certificate authentication is configured.
Tunnel restrictions are checked before enrollment and connector startup. There
is no fallback to a public tunnel if a direct connection fails. Restart existing
processes after changing policy.

## Optional Cloudflare origin TLS

Deploy the matching registry worker change before using this feature. Configure
`AMARCODE_WS_TLS_CERT` and `AMARCODE_WS_TLS_KEY` for the existing managed loopback
access listener, plus:

```sh
export AMARCODE_TUNNEL_ORIGIN_SERVER_NAME=localhost
export AMARCODE_TUNNEL_ORIGIN_CA=/secure/amarcode/server-ca.pem
```

The server name defaults to `localhost`; it must appear in the origin certificate's
SAN. The CA path is optional for publicly trusted origin certificates and must
otherwise be an existing absolute PEM path on the **cloudflared host**. Only the
path and server name go to the registry; private keys and CA contents do not.
Enrollment updates the tunnel ingress to HTTPS, sets `originServerName`/`caPool`,
and keeps `noTLSVerify: false`. The daemon refuses to start the connector unless
the allocator confirms the requested TLS settings. No plaintext retry is made.
Managed tunnels intentionally reject outer/origin mTLS: cloudflared is not
configured here to present that client identity. Use direct mTLS or paired inner
mTLS for client authentication.

Origin validation follows Cloudflare's documented origin parameters:
https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/configure-tunnels/origin-parameters/


## Paired end-to-end encryption for native clients

This mode uses TLS 1.3 inside binary WebSocket frames, followed by the existing
RPC or terminal WebSocket inside that TLS channel. It does not invent a key
exchange or record cipher. Outer TLS can terminate at Cloudflare; the inner TLS
session terminates only at the native client and daemon. Bearer authentication,
RPC, subscriptions, attachments, terminal bytes, and X11 frames use the inner
session. The outer handshake has only the public `amarcode.e2ee.tls13.v1`
subprotocol and contains no bearer token. TLS session resumption and early data
are disabled; TLS record authentication/sequence numbers reject tampering and
replay. The relay can still observe hostnames, timing, traffic volume, and denial
of service. Tunnel provisioning/control-plane credentials still belong to the
provider; this protects the daemon RPC bearer token and application payloads. The daemon still sees plaintext, stores project data as before, and
makes provider requests under its existing configuration.

Pairing is explicit certificate provisioning, not trust on first use: distribute
the daemon CA and expected identity name to each trusted native client out of
band; issue a distinct clientAuth certificate/key for every device from a CA the
daemon trusts. Keep the issuer key offline. Use the company CA workflow in
production. Changing the daemon key under the same approved CA/name rotates it
without teaching clients to trust an arbitrary relay-provided identity.

For local development generate a **new** directory with a signed initial CRL:

```sh
python3 scripts/tls-certificates.py dev-ca \
  --output credentials/my-e2ee --hostname localhost --hostname 127.0.0.1
```

The server process needs absolute paths:

```sh
export AMARCODE_E2EE_TLS_CERT=/secure/daemon/server-cert.pem
export AMARCODE_E2EE_TLS_KEY=/secure/daemon/server-key.pem
export AMARCODE_E2EE_CLIENT_CA=/secure/daemon/client-ca.pem
export AMARCODE_E2EE_CLIENT_CRL=/secure/daemon/clients.crl.pem
export AMARCODE_E2EE_SERVER_NAME=daemon.internal.example
export AMARCODE_E2EE_PROBE_CA=/secure/daemon/server-ca.pem
```

Configuring the inner identity makes encryption mandatory on that WebSocket
listener. A client CA and signed CRL are mandatory in this mode. `SERVER_NAME`
is the stable SAN identity in the inner server certificate, independent of the
Cloudflare hostname. For development use `localhost`. `PROBE_CA` is used by the
daemon to verify its public encrypted readiness probe; if omitted it uses the
client CA (appropriate only when the same CA issued both identities).

On each native desktop/CLI client, before launching its dedicated profile:

```sh
export AMARCODE_E2EE_CA=/secure/device/server-ca.pem
export AMARCODE_E2EE_SERVER_NAME=daemon.internal.example
export AMARCODE_E2EE_CLIENT_CERT=/secure/device/device-chain.pem
export AMARCODE_E2EE_CLIENT_KEY=/secure/device/device-key.pem
# Optional signed CRLs for the origin's server chain:
export AMARCODE_E2EE_SERVER_CRL=/secure/device/server-crls.pem
```

The expected name is required. The inner trust store contains only the explicitly
paired CA, not the public WebPKI root store. The normal outer connection still
uses `AMARCODE_CLIENT_TLS_*` for its own TLS. Native connection settings report
that this profile requires E2EE. Failed negotiation, certificate validation, or
inner authentication never retries without encryption.

Existing standard shared mode can use this to protect its remote WebSocket
listener, while retaining its existing local TCP listener. If machine policy
requires E2EE on **all** listeners, the standard TCP service is deliberately
rejected. Instead run the custom daemon with `--transport websocket run`. To
attach a separately managed connector use `--transport websocket tunnel` with
`AMARCODE_DAEMON_ADDR` and `AMARCODE_WS_ADDR` pointing at the same fixed loopback
port, the existing remote setup, and the same identity environment. Neither the
attachment health check nor the public readiness check sends the bearer token
through a public HTTP endpoint in E2EE mode. The encrypted `/e2ee-health` probe
authenticates the server but returns only status/encryption capability; it has no
RPC/terminal access and does not authorize a device.

If `allow_plaintext_loopback` is false, also configure outer/origin TLS as above:
E2EE alone hides application content but does not encrypt the outer HTTP upgrade
headers on loopback. If managed tunnels are prohibited, use the approved direct
WSS endpoint with this same inner encryption profile instead.

Browser E2EE is **not supported** in this version. Encrypted listeners reject the
browser ticket endpoint and legacy RPC/terminal handshakes. Do not paste a paired
native profile's bearer token or private keys into the hosted browser client:
its ordinary HTTPS authentication request is not an E2EE request. Browser support
requires a separately reviewed client implementation/distribution trust model;
loading code through an untrusted intermediary cannot establish that trust by
itself. Use the paired native application for confidential relay access.

## Automatic signed-CRL checks

Set `AMARCODE_WS_CLIENT_CRL` alongside `AMARCODE_WS_CLIENT_CA` for direct mTLS.
The E2EE listener uses `AMARCODE_E2EE_CLIENT_CRL`. Native clients may also set
`AMARCODE_CLIENT_TLS_CRL` for the outer server chain and
`AMARCODE_E2EE_SERVER_CRL` for the inner server chain.

CRLs must be signed by the appropriate issuers. For intermediate chains, supply
all CRLs needed for the verified chain. Unknown revocation status, invalid CRL
signatures, expired `nextUpdate`, missing files, and malformed/empty bundles fail
closed. There is no soft-fail or anonymous fallback. Existing client sessions
are rechecked by the daemon every 30 seconds and closed if revoked or if their
configured CRL/trust data becomes unusable. Client-side server CRLs are checked
on every new connection; changing them does not forcibly close an already-open
native socket. Close/reconnect client sessions after emergency server revocation.

The company CA/distribution service must atomically publish updated PEM CRL
bundles at these paths before expiration. Amarcode automatically reloads/checks
the configured files; it does not fetch arbitrary certificate-advertised URLs or
perform online OCSP. Administrators can schedule their approved distribution
mechanism without giving the daemon the CA signing key.

The development CA tool generates an initial CRL valid for one day. Refresh it:

```sh
python3 scripts/tls-crl.py --ca-directory credentials/my-e2ee
```

Revoke the example development device and publish a replacement CRL:

```sh
python3 scripts/tls-crl.py --ca-directory credentials/my-e2ee \
  --revoke client-cert.pem
```

That operation is intended for the development CA tooling only. Use the company's
CA to revoke production devices and publish its signed CRLs. Do not put the CA
private key on the running daemon merely to refresh a CRL. Old development
bundles created before CRL support should be replaced with a fresh output
directory, not overwritten.
