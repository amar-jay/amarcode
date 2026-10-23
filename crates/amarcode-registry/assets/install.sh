#!/usr/bin/env bash
# Install the signed remote daemon and cloudflared without administrator privileges.
set -euo pipefail
command -v python3 >/dev/null 2>&1 || { echo 'python3 is required' >&2; exit 1; }
exec python3 - "$@" <<'PY'
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import secrets
import shutil
import subprocess
import sys
import tempfile
import urllib.parse
import urllib.request

PUBLIC_KEY = '5ef56cd7772e8c601ca9c5a15378b7088fc558e7edcde73770cbb116d9e255d2'
REGISTRY = 'https://updates.amarcode.amarjay.com'


def download(url, destination, limit):
    if urllib.parse.urlsplit(url).scheme != 'https':
        raise ValueError('Downloads require HTTPS')
    request = urllib.request.Request(url, headers={'User-Agent': 'Amarcode-remote-installer'})
    with urllib.request.urlopen(request, timeout=60) as response:
        if urllib.parse.urlsplit(response.url).scheme != 'https':
            raise ValueError('Download redirected away from HTTPS')
        with open(destination, 'wb') as output:
            total = 0
            while True:
                chunk = response.read(min(65536, limit + 1 - total))
                if not chunk:
                    break
                total += len(chunk)
                if total > limit:
                    raise ValueError('Download exceeds its size limit')
                output.write(chunk)


def verify_manifest(manifest, signature, directory):
    signature_bytes = base64.b64decode(signature.read_text().strip(), validate=True)
    if len(signature_bytes) != 64:
        raise ValueError('Invalid manifest signature')
    key = directory / 'public.der'
    key.write_bytes(bytes.fromhex('302a300506032b6570032100' + PUBLIC_KEY))
    raw_signature = directory / 'signature.bin'
    raw_signature.write_bytes(signature_bytes)
    result = subprocess.run(['openssl', 'pkeyutl', '-verify', '-pubin', '-keyform', 'DER',
                             '-inkey', str(key), '-rawin', '-in', str(manifest),
                             '-sigfile', str(raw_signature)], capture_output=True)
    if result.returncode:
        raise ValueError('Manifest signature verification failed (OpenSSL with Ed25519 support is required)')
    return json.loads(manifest.read_bytes())


def select_artifact(manifest, target, manifest_url):
    if not {'authenticated-websocket', 'remote-cli'}.issubset(manifest.get('capabilities', [])):
        raise ValueError('This daemon release predates authenticated remote CLI support; publish a new daemon release first')
    version = manifest['version']
    if not isinstance(version, str) or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._+-]{0,127}', version):
        raise ValueError('Invalid release version')
    artifact = manifest['artifacts'].get(target)
    if not artifact or artifact['target'] != target:
        raise ValueError('No published daemon for ' + target)
    size = artifact['size']
    if type(size) is not int or not 0 < size <= 512 * 1024 * 1024:
        raise ValueError('Invalid artifact size')
    if not re.fullmatch(r'[a-fA-F0-9]{64}', artifact['sha256']):
        raise ValueError('Invalid artifact checksum')
    url = urllib.parse.urljoin(manifest_url, artifact['url'])
    parsed = urllib.parse.urlsplit(url)
    if parsed.scheme != 'https' or parsed.netloc != urllib.parse.urlsplit(manifest_url).netloc or parsed.username or parsed.password:
        raise ValueError('Artifact must be hosted on the signed release registry')
    return version, artifact, url


def select_cloudflared_asset(release, architecture, requested_version=None):
    version = release['tag_name']
    if not isinstance(version, str) or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._+-]{0,127}', version):
        raise ValueError('Invalid cloudflared release version')
    if requested_version and version != requested_version:
        raise ValueError('Cloudflared release does not match the requested version')
    suffix = {'x86_64': 'amd64', 'aarch64': 'arm64'}[architecture]
    name = 'cloudflared-linux-' + suffix
    asset = next((asset for asset in release['assets'] if asset.get('name') == name), None)
    if not asset:
        raise ValueError('No official cloudflared binary for ' + architecture)
    url = 'https://github.com/cloudflare/cloudflared/releases/download/' + version + '/' + name
    if asset.get('browser_download_url') != url:
        raise ValueError('Cloudflared binary must come from the official Cloudflare release')
    size, digest = asset.get('size'), asset.get('digest', '')
    if type(size) is not int or not 0 < size <= 256 * 1024 * 1024:
        raise ValueError('Invalid cloudflared artifact size')
    if not isinstance(digest, str) or not re.fullmatch(r'sha256:[a-fA-F0-9]{64}', digest):
        raise ValueError('Cloudflared release has no valid SHA-256 digest')
    return version, url, size, digest.split(':', 1)[1].lower()


def install_cloudflared(root, architecture, requested_version=None):
    endpoint = 'https://api.github.com/repos/cloudflare/cloudflared/releases/'
    endpoint += 'tags/' + requested_version if requested_version else 'latest'
    with tempfile.TemporaryDirectory(dir=root) as temporary:
        directory = Path(temporary)
        metadata = directory / 'cloudflared-release.json'
        print('Resolving official cloudflared release...', flush=True)
        download(endpoint, metadata, 2 * 1024 * 1024)
        release = json.loads(metadata.read_bytes())
        version, url, size, checksum = select_cloudflared_asset(release, architecture, requested_version)
        destination = root / 'cloudflared' / version / checksum / 'cloudflared'
        if not destination.is_file() or destination.stat().st_size != size or hashlib.sha256(destination.read_bytes()).hexdigest() != checksum:
            binary = directory / 'cloudflared'
            print('Downloading cloudflared ' + version + '...', flush=True)
            download(url, binary, size)
            if binary.stat().st_size != size or hashlib.sha256(binary.read_bytes()).hexdigest() != checksum:
                raise ValueError('Downloaded cloudflared size/checksum verification failed')
            os.chmod(binary, 0o700)
            subprocess.run([str(binary), '--version'], check=True, capture_output=True, timeout=10)
            destination.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
            os.replace(binary, destination)
        os.chmod(destination, 0o700)
        print('Installed cloudflared ' + version, flush=True)
        return destination


def private_write(path, contents):
    # Atomic replacement and mode from creation, including on reinstall.
    descriptor, name = tempfile.mkstemp(dir=path.parent)
    try:
        with os.fdopen(descriptor, 'w') as output:
            output.write(contents)
            output.flush()
            os.fsync(output.fileno())
        os.replace(name, path)
    finally:
        if os.path.exists(name):
            os.unlink(name)


def install_command(bin_dir, executable, root):
    bin_dir.mkdir(parents=True, exist_ok=True)
    command = bin_dir / 'amarcode-daemon'
    if os.path.lexists(command):
        if not command.is_symlink() or not command.resolve().is_relative_to((root / 'releases').resolve()):
            raise ValueError(str(command) + ' already exists outside this installation; choose another --bin-dir')
    temporary = bin_dir / ('.amarcode-daemon-' + secrets.token_hex(8))
    try:
        temporary.symlink_to(executable)
        os.replace(temporary, command)
    finally:
        if temporary.is_symlink():
            temporary.unlink()
    return command


def main():
    parser = argparse.ArgumentParser(description='Install the Amarcode remote daemon and cloudflared (Linux)')
    parser.add_argument('--version', help='Install a specific signed release; default: latest')
    parser.add_argument('--cloudflared-version', help='Install a specific official cloudflared release; default: latest')
    parser.add_argument('--bin-dir', type=Path, default=Path.home() / '.local/bin', help='Command installation directory; default: ~/.local/bin')
    parser.add_argument('--start', '--tunnel', dest='start', action='store_true', help='Start amarcode-daemon --remote after installation')
    parser.add_argument('--no-service', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args()
    if platform.system() != 'Linux':
        parser.error('The remote installer currently supports Linux hosts')
    architecture = {'x86_64': 'x86_64', 'amd64': 'x86_64', 'aarch64': 'aarch64', 'arm64': 'aarch64'}.get(platform.machine().lower())
    if architecture is None:
        parser.error('Unsupported host architecture')
    target = architecture + '-unknown-linux-gnu'
    if not shutil.which('openssl'):
        parser.error('openssl is required to verify the signed release')
    if args.version and not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._+-]{0,127}', args.version):
        parser.error('Invalid version')
    if args.cloudflared_version and not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._+-]{0,127}', args.cloudflared_version):
        parser.error('Invalid cloudflared version')
    manifest_url = REGISTRY + ('/v1/daemon/' + args.version + '/manifest.json' if args.version else '/v1/daemon/latest.json')
    root = Path(os.environ.get('XDG_DATA_HOME', str(Path.home() / '.local/share'))) / 'amarcode-remote'
    if not root.is_absolute() or any(c in str(root) for c in '\n\r\0'):
        parser.error('XDG_DATA_HOME must be an absolute path without control characters')
    bin_dir = args.bin_dir.expanduser().absolute()
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    os.chmod(root, 0o700)
    with tempfile.TemporaryDirectory(dir=root) as temporary:
        directory = Path(temporary)
        manifest_path, signature_path = directory / 'manifest.json', directory / 'manifest.sig'
        download(manifest_url, manifest_path, 1024 * 1024)
        download(manifest_url + '.sig', signature_path, 4096)
        manifest = verify_manifest(manifest_path, signature_path, directory)
        version, artifact, url = select_artifact(manifest, target, manifest_url)
        if args.version and version != args.version:
            raise ValueError('Signed manifest does not match requested version')
        binary = directory / 'amarcode-daemon'
        download(url, binary, artifact['size'])
        if binary.stat().st_size != artifact['size'] or hashlib.sha256(binary.read_bytes()).hexdigest().lower() != artifact['sha256'].lower():
            raise ValueError('Downloaded daemon size/checksum verification failed')
        os.chmod(binary, 0o700)
        probe = subprocess.run([str(binary), '--help'], check=True, capture_output=True, timeout=10)
        if b'--transport' not in probe.stdout or b'--remote' not in probe.stdout:
            raise ValueError('Downloaded binary does not support the daemon CLI')
        release = root / 'releases' / version / artifact['sha256']
        release.mkdir(parents=True, exist_ok=True, mode=0o700)
        executable = release / 'amarcode-daemon'
        os.replace(binary, executable)
        shutil.copyfile(manifest_path, release / 'manifest.json')
        shutil.copyfile(signature_path, release / 'manifest.json.sig')
    cloudflared = install_cloudflared(root, architecture, args.cloudflared_version)
    token_path = root / 'access-token'
    first_install = not token_path.exists()
    if first_install:
        private_write(token_path, secrets.token_hex(32) + '\n')
    os.chmod(token_path, 0o600)
    token = token_path.read_text().strip()
    if not re.fullmatch(r'[A-Za-z0-9._~-]{32,512}', token):
        raise ValueError('Existing access-token is invalid; fix it before restarting')
    private_write(root / 'remote.json', json.dumps({'cloudflared': str(cloudflared)}) + '\n')
    command = install_command(bin_dir, executable, root)
    import shlex
    quote = lambda value: shlex.quote(str(value))
    # Existing helper paths continue to use the same native command and lifecycle.
    for name, arguments in [('run.sh', '--transport websocket'), ('tunnel.sh', '--transport websocket --remote')]:
        launcher = root / name
        private_write(launcher, '#!/bin/sh\nset -eu\n' +
                      'export AMARCODE_REMOTE_ROOT=' + quote(root) + '\n' +
                      'export AMARCODE_APPDIR=' + quote(root / 'data') + '\n' +
                      'export AMARCODE_WS_TOKEN_FILE=' + quote(token_path) + '\n' +
                      'exec ' + quote(command) + ' ' + arguments + '\n')
        os.chmod(launcher, 0o700)
    print('\nAmarcode remote setup complete. No service or tunnel has been started.')
    print('Daemon version: ' + version)
    print('Command: ' + str(command))
    print('Cloudflared: ' + str(cloudflared))
    print('Data directory: ' + str(root / 'data'))
    print('Access token file: ' + str(token_path))
    if first_install:
        print('Access token (save privately): ' + token)
    else:
        print('Your existing access token and chat data have been preserved.')
    if str(bin_dir) not in os.get_exec_path():
        print('Add the command to this shell before continuing:')
        print('  export PATH=' + quote(bin_dir) + ':"$PATH"')
    print('\nStart whenever needed:')
    print('  amarcode-daemon --transport websocket --remote')
    print('  (or simply: amarcode-daemon --remote)')
    print('The command prints a wss://<tunnel-host>/rpc URL. Enter it with the access token in Amarcode → Settings → Connection.')
    print('Project paths refer to this host. Ctrl+C stops both processes; quick tunnel URLs change each time.')
    if args.start:
        os.environ['AMARCODE_REMOTE_ROOT'] = str(root)
        sys.stdout.flush()
        os.execv(str(command), [str(command), '--transport', 'websocket', '--remote'])


if __name__ == '__main__':
    try:
        main()
    except (Exception, KeyboardInterrupt) as error:
        print('Installation failed: ' + str(error), file=sys.stderr)
        sys.exit(1)
PY
