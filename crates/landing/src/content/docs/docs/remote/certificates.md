---
title: Certificates and direct TLS
description: Connect using company certificates, run a direct TLS daemon, or encrypt the local Cloudflare origin hop.
---

**Most remote users do not need to generate certificates.** The default
[Cloudflare setup](/docs/remote/setup/) already has a public TLS certificate.
Use this guide when your administrator supplies a certificate profile or asks you
to run a direct TLS endpoint.

mTLS and CRL options require a build that supports them. Use the matching daemon
and client builds supplied by your administrator if those features are not yet
in your installed release.

## Receive a connection profile

For a direct company endpoint, ask your administrator for:

- The `wss://…/rpc` URL and daemon access token.
- The server's public CA bundle, if it is not publicly trusted.
- Your own client certificate and private key, if client certificates are required.
- Any required server revocation-list bundle and renewal instructions.

Keep private keys outside your project and source-control checkout. On Unix:

```sh
chmod 600 /absolute/path/to/client-key.pem
```

On Windows, restrict the private key's file permissions to your account using
the company-approved procedure. A CA certificate is public; a CA **private key**
should never be included in an ordinary user's connection bundle.

## Configure the native client

In the shell that will launch the desktop or CLI:

```sh
export AMARCODE_CLIENT_TLS_CA=/absolute/path/to/server-ca.pem
export AMARCODE_CLIENT_TLS_CERT=/absolute/path/to/client-chain.pem
export AMARCODE_CLIENT_TLS_KEY=/absolute/path/to/client-key.pem
```

Set `TLS_CA` only when a custom CA is needed. Set `TLS_CERT` and `TLS_KEY`
together only when mTLS is required. If IT supplies server-chain CRLs, also set:

```sh
export AMARCODE_CLIENT_TLS_CRL=/absolute/path/to/server-crls.pem
```

These settings apply to native connections made by that process. They are not
sent into the webview, and they do not configure your browser. Explicitly
configured missing or invalid files cause connection failure rather than a
less-secure retry.

### Launch the desktop from that shell

Fully quit the desktop first; opening another window may reuse its existing
process and old environment. Then launch from the configured shell:

```sh
amarcode
```

On macOS, launch the installed executable directly so it inherits the environment:

```sh
/Applications/amarcode.app/Contents/MacOS/amarcode
```

For PowerShell, set variables with `$env:` and launch your actual executable:

```powershell
$env:AMARCODE_CLIENT_TLS_CA = 'C:\secure\server-ca.pem'
$env:AMARCODE_CLIENT_TLS_CERT = 'C:\secure\client-chain.pem'
$env:AMARCODE_CLIENT_TLS_KEY = 'C:\secure\client-key.pem'
& 'C:\path\to\amarcode.exe'
```

In **Settings → Connection**, choose **Remote daemon**, enter the supplied URL
and token, and select **Verify and apply**. Use a dedicated process/profile for
company credentials; environment-based certificate settings apply to its remote
connections, not just one saved URL.

The CLI uses the same native certificate settings:

```sh
amarcoded ssh wss://daemon.internal.example:43721/rpc
```

It asks for the access token unless you provide a private token file. Client
certificates supplement the token; they do not replace it.

## Run a direct TLS daemon

This section is for the person operating the remote host. You need a server
certificate whose **Subject Alternative Name (SAN)** matches the DNS name or IP
that clients will use. A company VPN/private DNS record must already route to
that host. Amarcode does not configure those network services for you.

On the host, using absolute paths:

```sh
export AMARCODE_DAEMON_ADDR=10.0.0.20:43721
export AMARCODE_WS_TOKEN_FILE=/secure/amarcode/access-token
export AMARCODE_WS_TLS_CERT=/secure/amarcode/server-chain.pem
export AMARCODE_WS_TLS_KEY=/secure/amarcode/server-key.pem
amarcoded --transport websocket run
```

Replace `10.0.0.20` with an interface IP on that host. The bind setting takes an
IP address, not a DNS name. Clients use, for example,
`wss://daemon.internal.example:43721/rpc`, where the DNS name matches the SAN.
The certificate file should include the server certificate and necessary
intermediates. The private key must match it and be private to the service user.

If you need a new token file, create it once on the host:

```sh
umask 077
openssl rand -hex 32 > /secure/amarcode/access-token
```

Create its private parent directory first. Do not overwrite a token used by an
existing installation during routine setup. Give authorized clients the token
through your approved channel.

Stop any existing daemon using the same data directory or port before starting
this custom instance. Leave this process running or have IT manage it as a
service. Custom TLS instances do not use the standard managed daemon's self-update
workflow; update them through the administrator's deployment process.

### Require a client certificate (mTLS)

Add the approved client CA before starting the host:

```sh
export AMARCODE_WS_CLIENT_CA=/secure/amarcode/client-ca.pem
export AMARCODE_WS_CLIENT_CRL=/secure/amarcode/client-crls.pem
```

Each device must now present a certificate issued by that CA **and** authenticate
with the access token. Its certificate must permit client authentication. CRL
checking is enabled when the CRL path is configured; see
[revocation](/docs/remote/administration/#revoke-a-device).

Without `AMARCODE_WS_CLIENT_CA`, the daemon uses server TLS and token authentication
without requiring a client certificate. Non-loopback listeners still require TLS.

## Obtain or generate a certificate

For production, ask your company CA for the server certificate and a separate
client identity for each device. Your administrator should control issuance,
renewal, revocation, and CA trust.

To create a server key and certificate signing request (CSR) yourself, use a new
private directory on the server:

```sh
umask 077
mkdir -p "$HOME/.config/amarcode/server-tls"
cd "$HOME/.config/amarcode/server-tls"
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:prime256v1 \
  -out server-key.pem
openssl req -new -key server-key.pem \
  -subj '/CN=daemon.internal.example' \
  -addext 'subjectAltName=DNS:daemon.internal.example' \
  -out server.csr
```

Replace the name with your actual endpoint. Use a new directory so you do not
replace an existing key. Send `server.csr` to IT; keep `server-key.pem` on the host.
A CSR is not a certificate and cannot start a TLS listener until it is signed.

The source checkout also includes a development helper:

```sh
python3 scripts/tls-certificates.py dev-ca \
  --output credentials/my-dev-tls --hostname localhost --hostname 127.0.0.1
```

Run it from a matching Amarcode repository checkout with Python 3 and OpenSSL.
It refuses an existing output directory. It creates a development CA, server and
client identities, and an initial signed CRL. Leaf certificates last 30 days;
the development CA lasts 365 days. These certificates are not automatically
trusted by other devices or browsers.

| File                                   | Where it belongs                                          |
| -------------------------------------- | --------------------------------------------------------- |
| `server-cert.pem` and `server-key.pem` | Daemon host                                               |
| `client-cert.pem` and `client-key.pem` | That authorized native client                             |
| `ca.pem`                               | Explicit trust configuration on the appropriate endpoints |
| `clients.crl.pem`                      | Daemon's configured CRL path; renew before expiry         |
| `ca-key.pem`                           | Issuer only; keep offline, not on the running daemon      |

Development material is for testing, not company approval. Do not distribute a
single development client key to a team or copy the whole CA directory onto a
production server.

## Add TLS to the Cloudflare origin hop

Use this only when Cloudflare is an approved relay and you want to encrypt the
last connection from `cloudflared` to the daemon. **Cloudflare can still read
application data unless you also use paired E2EE.**

On a host with [remote setup](/docs/remote/setup/) already prepared:

```sh
export AMARCODE_WS_TLS_CERT=/secure/amarcode/origin-chain.pem
export AMARCODE_WS_TLS_KEY=/secure/amarcode/origin-key.pem
export AMARCODE_TUNNEL_ORIGIN_SERVER_NAME=daemon.internal.example
export AMARCODE_TUNNEL_ORIGIN_CA=/secure/amarcode/origin-ca.pem
amarcoded --remote
```

The origin name must match the origin certificate's SAN. It defaults to
`localhost` if omitted. The CA path must be an existing absolute PEM path on the
**cloudflared host**, not your laptop. Publicly trusted origin certificates can
omit the custom CA path. The connector verifies the certificate; verification
is not disabled on failure.

This requires a registry worker that understands origin TLS. The daemon refuses
tunnel startup if the allocator does not confirm the requested TLS settings.
Ask the service operator to update it rather than reverting to unverified TLS.

Leave `AMARCODE_WS_CLIENT_CA` unset for the managed connector: it does not present
an outer client certificate. Use direct mTLS or
[paired inner mTLS](/docs/remote/end-to-end/) for device authentication.

## Renew and rotate

Use IT's renewal procedure before certificates expire. Keep the same trusted
identity name and deploy the matching certificate/key pair. Restart a normal TLS
listener after changing its identity unless its configured CRL mode reloads it
for new connections. Paired inner TLS identities reload for new connections.

Update client CA bundles before moving to a new issuer. Revoked devices must
get a new authorized identity; reconnecting with the revoked certificate does
not repair it. See [company policy and revocation](/docs/remote/administration/).
