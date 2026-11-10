#!/usr/bin/env python3
"""Refresh/revoke a development identity using the CA created by tls-certificates.py.
Production deployments should use their company's CA/CRL distribution service.
"""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ca-directory", required=True, type=Path)
    parser.add_argument("--revoke", type=Path, help="Certificate to revoke, relative to CA directory or absolute")
    parser.add_argument("--days", type=int, default=1)
    args = parser.parse_args()
    if args.days < 1 or args.days > 30:
        parser.error("CRL lifetime must be between 1 and 30 days")
    os.umask(0o077)
    root = args.ca_directory.resolve()
    with (root / ".ca.lock").open("a+b") as lock:
        if os.name == "nt":
            import msvcrt
            lock.write(b"0")
            lock.flush()
            lock.seek(0)
            msvcrt.locking(lock.fileno(), msvcrt.LK_LOCK, 1)
        else:
            import fcntl
            fcntl.flock(lock, fcntl.LOCK_EX)
        if args.revoke:
            certificate = args.revoke if args.revoke.is_absolute() else root / args.revoke
            subprocess.run(["openssl", "ca", "-config", "ca.cnf", "-revoke", str(certificate), "-batch"], cwd=root, check=True)
        descriptor, temporary = tempfile.mkstemp(prefix=".crl-", dir=root)
        os.close(descriptor)
        try:
            subprocess.run(["openssl", "ca", "-config", "ca.cnf", "-gencrl", "-crldays", str(args.days), "-out", temporary, "-batch"], cwd=root, check=True)
            with open(temporary, "rb") as signed:
                os.fsync(signed.fileno())
            os.replace(temporary, root / "clients.crl.pem")
        finally:
            if os.path.exists(temporary):
                os.unlink(temporary)
    print("Published signed development CRL; new TLS handshakes will use it automatically.")


if __name__ == "__main__":
    main()
