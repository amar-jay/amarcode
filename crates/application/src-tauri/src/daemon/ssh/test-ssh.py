#!/usr/bin/env python3
"""Test-only OpenSSH stand-in: forward TCP bytes; never contact a real SSH host."""
import os
from pathlib import Path
import socket
import sys
import threading
import time

Path(__file__).with_name("pid").write_text(str(os.getpid()))
host = sys.argv[-1]
if host == "noisy":
    sys.stderr.write("é" * 32768 + "\nHost key verification failed\n")
    sys.exit(255)
if host == "reject":
    print("Host key verification failed", file=sys.stderr)
    sys.exit(255)
if host == "stall":
    time.sleep(60)
    sys.exit(1)

forward = sys.argv[sys.argv.index("-L") + 1].split(":")
listener = socket.socket()
listener.bind((forward[0], int(forward[1])))
listener.listen()


def copy(source, destination):
    try:
        while data := source.recv(65536):
            destination.sendall(data)
    except OSError:
        pass
    finally:
        try:
            destination.shutdown(socket.SHUT_WR)
        except OSError:
            pass


def relay(client):
    with client:
        try:
            remote = socket.create_connection((forward[2], int(forward[3])), timeout=2)
        except OSError:
            return
        with remote:
            remote.settimeout(None)
            upstream = threading.Thread(target=copy, args=(client, remote), daemon=True)
            upstream.start()
            copy(remote, client)
            upstream.join(timeout=2)


while True:
    client, _ = listener.accept()
    threading.Thread(target=relay, args=(client,), daemon=True).start()
