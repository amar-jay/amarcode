# TLS, certificates, and company deployment

Amarcode supports three configurations. Existing installations keep their current
behavior unless explicitly configured or restricted by machine policy.

| Configuration | Encryption | Trust boundary |
| --- | --- | --- |
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
Certificate rotation, trust changes, and machine-policy changes require a daemon
restart. This implementation does not fetch CRLs/OCSP for client-certificate
revocation: use short-lived certificates, and remove/rotate trusted CAs and tokens
with a restart when revocation is needed. mTLS identities do not yet have separate
per-device authorization roles.

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

Missing fields keep defaults (`true`, `true`, `false` respectively). Unknown
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
requires mTLS on all listeners and also prohibits managed Cloudflare tunnels.
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
Managed tunnels intentionally reject daemon mTLS: cloudflared is not configured
here to present a client identity. Use the direct mTLS mode for that requirement.

Origin validation follows Cloudflare's documented origin parameters:
https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/configure-tunnels/origin-parameters/
