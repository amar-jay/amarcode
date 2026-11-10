#!/usr/bin/env python3
"""Generate development TLS identities or a server CSR for a company CA.

Never installs trust anchors or modifies a running daemon. Output must be new.
"""
import argparse
import ipaddress
import os
from pathlib import Path
import re
import secrets
import subprocess
import tempfile


def run(*args):
    subprocess.run(["openssl", *map(str, args)], check=True, stdout=subprocess.DEVNULL)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["dev-ca", "csr"])
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--hostname", action="append", required=True, help="DNS name or IP; repeat for additional SANs")
    args = parser.parse_args()
    sans = []
    for host in args.hostname:
        try:
            sans.append("IP:" + str(ipaddress.ip_address(host)))
        except ValueError:
            if len(host) > 253 or not re.fullmatch(r"[A-Za-z0-9](?:[A-Za-z0-9.-]*[A-Za-z0-9])?", host):
                parser.error("hostname must be a DNS name or IP address")
            sans.append("DNS:" + host)
    os.umask(0o077)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    if args.output.exists():
        parser.error("output already exists; refusing to overwrite certificates or keys")
    with tempfile.TemporaryDirectory(prefix=".tls-", dir=args.output.parent) as staging:
        out = Path(staging)
        run("genpkey", "-algorithm", "EC", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-out", out / "server-key.pem")
        run("req", "-new", "-key", out / "server-key.pem", "-subj", "/CN=" + args.hostname[0],
            "-addext", "subjectAltName=" + ",".join(sans), "-out", out / "server.csr")
        if args.mode == "dev-ca":
            run("genpkey", "-algorithm", "EC", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-out", out / "ca-key.pem")
            run("req", "-new", "-x509", "-key", out / "ca-key.pem", "-days", "365", "-sha256",
                "-subj", "/CN=Amarcode Development CA", "-addext", "basicConstraints=critical,CA:TRUE,pathlen:0",
                "-addext", "keyUsage=critical,keyCertSign,cRLSign", "-out", out / "ca.pem")
            for name, usage in [("server", "serverAuth"), ("client", "clientAuth")]:
                if name == "client":
                    run("genpkey", "-algorithm", "EC", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-out", out / "client-key.pem")
                    run("req", "-new", "-key", out / "client-key.pem", "-subj", "/CN=Amarcode Development Client", "-out", out / "client.csr")
                extension = out / "extensions.cnf"
                extension.write_text("basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=" + usage + "\n" +
                    ("subjectAltName=" + ",".join(sans) + "\n" if name == "server" else ""))
                run("x509", "-req", "-in", out / (name + ".csr"), "-CA", out / "ca.pem", "-CAkey", out / "ca-key.pem",
                    "-set_serial", str(secrets.randbits(127) + 1), "-days", "30", "-sha256", "-extfile", extension,
                    "-out", out / (name + "-cert.pem"))
            (out / "extensions.cnf").unlink()
            (out / "index.txt").write_text("")
            (out / "crlnumber").write_text("01\n")
            (out / "ca.cnf").write_text("[ca]\ndefault_ca=issuer\n[issuer]\ndatabase=index.txt\ncertificate=ca.pem\nprivate_key=ca-key.pem\ncrlnumber=crlnumber\ndefault_md=sha256\ndefault_crl_days=1\n")
            subprocess.run(["openssl", "ca", "-gencrl", "-config", "ca.cnf", "-out", "clients.crl.pem", "-batch"], cwd=out, check=True, stdout=subprocess.DEVNULL)

            run("verify", "-CAfile", out / "ca.pem", "-purpose", "sslserver", "-verify_hostname", args.hostname[0], out / "server-cert.pem") if not sans[0].startswith("IP:") else run("verify", "-CAfile", out / "ca.pem", "-purpose", "sslserver", "-verify_ip", args.hostname[0], out / "server-cert.pem")
            run("verify", "-CAfile", out / "ca.pem", "-purpose", "sslclient", out / "client-cert.pem")
        # Atomic publication; failed commands leave no partial identity behind.
        os.rename(out, args.output)
    print(f"Created {args.mode} material in {args.output.resolve()}")
    if args.mode == "dev-ca":
        print("Development only. Keep ca-key.pem offline; do not deploy it or commit private keys.")
    else:
        print("Send server.csr to your company CA. Keep server-key.pem private.")


if __name__ == "__main__":
    main()
