`websocket-cert.pem` and `websocket-key.pem` are a public, self-signed localhost
certificate and matching **test-only** key. They exercise actual TLS handshakes
without disabling certificate verification. Never use these fixtures to serve a
real daemon. Tests copy the key to a private temporary file before using it.
