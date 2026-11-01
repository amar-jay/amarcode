"""Live CLI test using a private Xvfb display; never touches the desktop display."""
import fcntl
import os
from pathlib import Path
import pty
import secrets
import select
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import termios
import time

binary = sys.argv[1]
required = ["Xvfb", "xauth", "xdpyinfo", "xmessage", "xwininfo"]
if any(shutil.which(tool) is None for tool in required):
    print("SKIP: X11 integration needs Xvfb, xauth, xdpyinfo, xmessage, and xwininfo")
    sys.exit(0)

children = []
def start(arguments, **options):
    child = subprocess.Popen(arguments, **options)
    children.append(child)
    return child

def wait_until(condition, message, seconds=10):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if condition():
            return
        time.sleep(0.03)
    raise AssertionError(message)

def available_display():
    for _ in range(100):
        number = 2000 + secrets.randbelow(10000)
        if Path(f"/tmp/.X{number}-lock").exists() or Path(f"/tmp/.X11-unix/X{number}").exists():
            continue
        with socket.socket() as probe:
            try:
                probe.bind(("127.0.0.1", 6000 + number))
                return number
            except OSError:
                continue
    raise AssertionError("No available test display")

def connected(address):
    try:
        with socket.create_connection(address, timeout=0.1):
            return True
    except OSError:
        return False

try:
    with tempfile.TemporaryDirectory(prefix="amarcode-x11-") as temporary:
        root = Path(temporary)
        number = available_display()
        display = f":{number}"
        authority = root / "local-authority"
        authority.touch(mode=0o600)
        real_cookie = secrets.token_bytes(16)
        subprocess.run(["xauth", "-f", str(authority), "source", "-"],
            input=f"add {display} MIT-MAGIC-COOKIE-1 {real_cookie.hex()}" + chr(10),
            text=True, check=True, capture_output=True)
        xvfb = start(["Xvfb", display, "-screen", "0", "640x480x24",
            "-auth", str(authority), "-nolisten", "tcp"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        wait_until(lambda: Path(f"/tmp/.X11-unix/X{number}").exists(), "Xvfb did not start")
        local_env = dict(os.environ, DISPLAY=display, XAUTHORITY=str(authority))
        subprocess.run(["xdpyinfo"], env=local_env, check=True,
            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)

        daemon_root = root / "daemon"
        daemon_root.mkdir()
        token = daemon_root / "access-token"
        token.write_text(secrets.token_hex(32))
        token.chmod(0o600)
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
        env = dict(os.environ)
        for key in ["AMARCODE_APPDIR", "AMARCODE_STORE_PATH", "AMARCODE_WS_TOKEN_FILE",
                    "AMARCODE_WS_TLS_CERT", "AMARCODE_WS_TLS_KEY", "AMARCODE_WS_ALLOW_ORIGIN"]:
            env.pop(key, None)
        env.update(AMARCODE_REMOTE_ROOT=str(daemon_root), AMARCODE_ACP_REGISTRY_SOURCE="",
            AMARCODE_DAEMON_ADDR=f"127.0.0.1:{port}", HOME=str(root), SHELL="/bin/sh",
            DISPLAY=":54321", XAUTHORITY=str(root / "wrong-daemon-authority"))
        daemon = start([binary, "--remote", "--no-tunnel"], env=env,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        wait_until(lambda: connected(("127.0.0.1", port)), "Daemon did not start")
        url = f"ws://127.0.0.1:{port}/rpc"

        for mode in ["disabled", "headless", "unavailable"]:
            client_env = local_env.copy()
            flags = ["--no-x11"] if mode == "disabled" else []
            if mode == "headless":
                client_env.pop("DISPLAY", None)
            elif mode == "unavailable":
                client_env["DISPLAY"] = ":54322"
            master, slave = pty.openpty()
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 100, 0, 0))
            client = start([binary, "ssh", *flags, url, "--token-file", str(token)],
                env=client_env, stdin=slave, stdout=slave, stderr=slave, start_new_session=True)
            os.close(slave)
            info = root / (mode + "-env")
            os.write(master, (f'printf "%s|%s" "$DISPLAY" "$XAUTHORITY" > {info}; exit 0' + chr(10)).encode())
            output = bytearray()
            deadline = time.monotonic() + 15
            while client.poll() is None and time.monotonic() < deadline:
                if select.select([master], [], [], 0.1)[0]:
                    try:
                        output.extend(os.read(master, 65536))
                    except OSError:
                        break
            client.wait(timeout=5)
            os.close(master)
            assert client.returncode == 0, output.decode(errors="replace")
            assert info.read_text() == "|", mode + " unexpectedly enabled X11"

        for abrupt in [False, True]:
            master, slave = pty.openpty()
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 100, 0, 0))
            flags = ["--x11"] if abrupt else []
            client = start([binary, "ssh", *flags, url, "--token-file", str(token)],
                env=local_env, stdin=slave, stdout=slave, stderr=slave, start_new_session=True)
            os.close(slave)
            info = root / ("disconnect-env" if abrupt else "exit-env")
            result = root / "xdpyinfo"
            escaped_newline = chr(92) + "n"
            command = (
                f'printf "%s{escaped_newline}%s{escaped_newline}" "$DISPLAY" "$XAUTHORITY" > {info}; '
                f'xdpyinfo > {result}; first=$?; '
                f'xdpyinfo > {root / "second"} & p1=$!; '
                f'xdpyinfo > {root / "third"} & p2=$!; wait "$p1"; wait "$p2"; '
                'xmessage -title "Amarcode X11 forwarding test" -timeout 20 -buttons OK:0 "Forwarded window" & xjob=$!; '
                f'printf "X11_RESULT_%s_READY{escaped_newline}" "$first"' + chr(10)
            )
            os.write(master, command.encode())
            output = bytearray()
            deadline = time.monotonic() + 12
            while b"X11_RESULT_0_READY" not in output and time.monotonic() < deadline:
                if select.select([master], [], [], 0.1)[0]:
                    try:
                        output.extend(os.read(master, 65536))
                    except OSError:
                        break
            assert b"X11_RESULT_0_READY" in output, output.decode(errors="replace")
            assert "vendor string:" in result.read_text()
            remote_display, remote_authority = info.read_text().splitlines()
            assert remote_display.startswith("127.0.0.1:")
            remote_authority = Path(remote_authority)
            assert remote_authority != authority and remote_authority.exists()
            assert remote_authority.stat().st_mode & 0o077 == 0
            assert real_cookie not in remote_authority.read_bytes()
            forward_port = 6000 + int(remote_display.split(":")[1].split(".")[0])
            def window_visible():
                result = subprocess.run(["xwininfo", "-root", "-tree"], env=local_env,
                    check=True, capture_output=True, text=True)
                return "Amarcode X11 forwarding test" in result.stdout
            wait_until(window_visible, "Forwarded application window did not appear on local X server")
            if abrupt:
                client.kill()
            else:
                os.write(master, ('kill "$xjob"; wait "$xjob" 2>/dev/null; exit 0' + chr(10)).encode())
            deadline = time.monotonic() + 8
            while client.poll() is None and time.monotonic() < deadline:
                if select.select([master], [], [], 0.1)[0]:
                    try:
                        output.extend(os.read(master, 65536))
                    except OSError:
                        break
            client.wait(timeout=5)
            if not abrupt:
                assert client.returncode == 0, output.decode(errors="replace")
            os.close(master)
            wait_until(lambda: not remote_authority.exists(), "Remote authority file leaked")
            wait_until(lambda: not connected(("127.0.0.1", forward_port)), "X11 listener leaked")
            wait_until(lambda: not window_visible(), "Forwarded window survived disconnect")
            assert daemon.poll() is None, "Closing X11 stopped the daemon"
        print("PASS: default and explicit X11 forwarding, opt-out, headless and unavailable display fallback, real window, concurrent connections and disconnect cleanup")
finally:
    for child in reversed(children):
        if child.poll() is None:
            child.terminate()
        try:
            child.wait(timeout=3)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait(timeout=3)
