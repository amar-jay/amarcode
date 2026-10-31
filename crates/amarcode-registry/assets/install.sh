#!/usr/bin/env bash
# Install or reuse the shared daemon, agent runtimes, and remote access connector.
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
import urllib.error
import urllib.parse
import urllib.request

PUBLIC_KEY = '5ef56cd7772e8c601ca9c5a15378b7088fc558e7edcde73770cbb116d9e255d2'
REGISTRY = 'https://updates.amarcode.amarjay.com'


def github_api(url):
    parsed = urllib.parse.urlsplit(url)
    return (parsed.scheme == 'https' and parsed.hostname == 'api.github.com'
            and parsed.port in (None, 443) and not parsed.username and not parsed.password)


class DownloadRedirectHandler(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, response, code, message, headers, new_url):
        if urllib.parse.urlsplit(new_url).scheme != 'https':
            raise ValueError('Download redirected away from HTTPS')
        redirected = super().redirect_request(request, response, code, message, headers, new_url)
        # GitHub release assets redirect to storage hosts. Credentials belong
        # only to the API, never to the registry, asset downloads, or redirects.
        if redirected and not github_api(new_url):
            redirected.remove_header('Authorization')
        return redirected


def download(url, destination, limit):
    if urllib.parse.urlsplit(url).scheme != 'https':
        raise ValueError('Downloads require HTTPS')
    headers = {'User-Agent': 'Amarcode-remote-installer'}
    if github_api(url):
        token = next((value.strip() for name in ('GH_TOKEN', 'GITHUB_TOKEN')
                      if (value := os.environ.get(name, '')).strip()), '')
        if token:
            if not all(33 <= ord(character) <= 126 for character in token):
                raise ValueError('Invalid GH_TOKEN or GITHUB_TOKEN value')
            headers['Authorization'] = 'Bearer ' + token
    request = urllib.request.Request(url, headers=headers)
    opener = urllib.request.build_opener(DownloadRedirectHandler())
    try:
        response = opener.open(request, timeout=60)
    except urllib.error.HTTPError as error:
        error.close()
        response_headers = error.headers or {}
        if github_api(error.url):
            if error.code == 429 or (error.code == 403 and (
                    response_headers.get('X-RateLimit-Remaining') == '0'
                    or response_headers.get('Retry-After')
                    or 'rate limit' in str(error.reason).lower())):
                wait = ''
                reset = response_headers.get('X-RateLimit-Reset', '')
                if reset.isdecimal():
                    from datetime import datetime, timezone
                    wait = ' The limit resets at ' + datetime.fromtimestamp(int(reset), timezone.utc).strftime('%Y-%m-%d %H:%M:%S UTC') + '.'
                raise ValueError('GitHub API rate limit exceeded while resolving an installation dependency.'
                                 ' Export GH_TOKEN or GITHUB_TOKEN with a GitHub token, or retry after the limit resets.' + wait) from None
            if error.code == 401:
                raise ValueError('GitHub rejected GH_TOKEN or GITHUB_TOKEN. Refresh the token or unset it and retry.') from None
        raise
    with response:
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
    if not {'authenticated-websocket', 'remote-cli', 'unified-instance'}.issubset(manifest.get('capabilities', [])):
        raise ValueError('This daemon release predates authenticated unified-instance CLI support; publish a new daemon release first')
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
        print('Resolving remote access connector...', flush=True)
        download(endpoint, metadata, 2 * 1024 * 1024)
        release = json.loads(metadata.read_bytes())
        version, url, size, checksum = select_cloudflared_asset(release, architecture, requested_version)
        destination = root / 'cloudflared' / version / checksum / 'cloudflared'
        if not destination.is_file() or destination.stat().st_size != size or hashlib.sha256(destination.read_bytes()).hexdigest() != checksum:
            binary = directory / 'cloudflared'
            print('Downloading remote access connector ' + version + '...', flush=True)
            download(url, binary, size)
            if binary.stat().st_size != size or hashlib.sha256(binary.read_bytes()).hexdigest() != checksum:
                raise ValueError('Downloaded cloudflared size/checksum verification failed')
            os.chmod(binary, 0o700)
            subprocess.run([str(binary), '--version'], check=True, capture_output=True, timeout=10)
            destination.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
            os.replace(binary, destination)
        os.chmod(destination, 0o700)
        print('Installed remote access connector ' + version, flush=True)
        return destination


def runtime_layout(tool, architecture):
    if tool == 'bun':
        target = 'bun-linux-x64-baseline' if architecture == 'x86_64' else 'bun-linux-aarch64'
        return 'oven-sh/bun', target + '.zip', { 'bun': target + '/bun' }
    target = 'uv-' + architecture + '-unknown-linux-gnu'
    return 'astral-sh/uv', target + '.tar.gz', { 'uv': target + '/uv', 'uvx': target + '/uvx' }


def install_runtime(root, tool, architecture, requested_version=None):
    repo, asset_name, members = runtime_layout(tool, architecture)
    endpoint = 'https://api.github.com/repos/' + repo + '/releases/'
    endpoint += 'tags/' + requested_version if requested_version else 'latest'
    with tempfile.TemporaryDirectory(dir=root) as temporary:
        directory = Path(temporary)
        metadata = directory / 'release.json'
        print('Resolving official ' + tool + ' release...', flush=True)
        download(endpoint, metadata, 2 * 1024 * 1024)
        release = json.loads(metadata.read_bytes())
        version = release['tag_name']
        if not isinstance(version, str) or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._+-]{0,127}', version):
            raise ValueError('Invalid ' + tool + ' release version')
        if requested_version and version != requested_version:
            raise ValueError(tool + ' release does not match requested version')
        asset = next((item for item in release['assets'] if item.get('name') == asset_name), None)
        if not asset:
            raise ValueError('No official ' + tool + ' binary for ' + architecture)
        url = 'https://github.com/' + repo + '/releases/download/' + version + '/' + asset_name
        size, digest = asset.get('size'), asset.get('digest', '')
        if asset.get('browser_download_url') != url:
            raise ValueError(tool + ' must come from its official release repository')
        if type(size) is not int or not 0 < size <= 256 * 1024 * 1024:
            raise ValueError('Invalid ' + tool + ' archive size')
        if not isinstance(digest, str) or not re.fullmatch(r'sha256:[a-fA-F0-9]{64}', digest):
            raise ValueError(tool + ' release has no valid SHA-256 digest')
        checksum = digest.split(':', 1)[1].lower()
        destination = root / 'runtimes' / tool / version / checksum
        names = list(members) + (['bunx'] if tool == 'bun' else [])
        receipt = destination / 'installed.json'
        try:
            hashes = json.loads(receipt.read_text())
            cached = all(re.fullmatch(r'[a-f0-9]{64}', hashes.get(name, '')) and
                         hashlib.sha256((destination / 'bin' / name).read_bytes()).hexdigest() == hashes[name]
                         for name in names)
        except (OSError, ValueError, TypeError, AttributeError):
            cached = False
        if not cached:
            archive = directory / asset_name
            print('Downloading ' + tool + ' ' + version + '...', flush=True)
            download(url, archive, size)
            if archive.stat().st_size != size or hashlib.sha256(archive.read_bytes()).hexdigest() != checksum:
                raise ValueError('Downloaded ' + tool + ' size/checksum verification failed')
            staged = directory / 'bin'
            staged.mkdir(mode=0o700)
            # Read only explicitly expected regular files; never extract archive paths.
            def copy_member(source, name, member_size):
                if not 0 < member_size <= 256 * 1024 * 1024:
                    raise ValueError('Invalid ' + tool + ' executable size')
                target = staged / name
                with target.open('wb') as output:
                    shutil.copyfileobj(source, output)
                os.chmod(target, 0o700)
            if tool == 'bun':
                import zipfile
                with zipfile.ZipFile(archive) as package:
                    for name, member in members.items():
                        entry = package.getinfo(member)
                        if entry.is_dir() or (entry.external_attr >> 16) & 0o170000 == 0o120000:
                            raise ValueError('Runtime archive entry must be a regular file')
                        with package.open(entry) as source:
                            copy_member(source, name, entry.file_size)
                (staged / 'bunx').symlink_to('bun')
            else:
                import tarfile
                with tarfile.open(archive, 'r:gz') as package:
                    for name, member in members.items():
                        entry = package.getmember(member)
                        if not entry.isfile():
                            raise ValueError('Runtime archive entry must be a regular file')
                        with package.extractfile(entry) as source:
                            copy_member(source, name, entry.size)
            environment = dict(os.environ, PATH=str(staged) + os.pathsep + os.environ.get('PATH', ''))
            for name in names:
                subprocess.run([str(staged / name), '--version'], check=True,
                               capture_output=True, timeout=10, env=environment)
            destination.mkdir(parents=True, exist_ok=True, mode=0o700)
            final_bin = destination / 'bin'
            final_bin.mkdir(exist_ok=True, mode=0o700)
            for name in names:
                os.replace(staged / name, final_bin / name)
            hashes = {name: hashlib.sha256((final_bin / name).read_bytes()).hexdigest() for name in names}
            private_write(receipt, json.dumps(hashes))
        print('Installed ' + tool + ' ' + version, flush=True)
        return destination / 'bin'


def link_runtime(bin_dir, name, executable, root):
    command = bin_dir / name
    if os.path.lexists(command) and (not command.is_symlink() or not command.resolve().is_relative_to((root / 'runtimes').resolve())):
        # Keep unrelated user installations; the daemon uses its own managed PATH.
        return
    temporary = bin_dir / ('.' + name + '-' + secrets.token_hex(8))
    try:
        temporary.symlink_to(executable)
        os.replace(temporary, command)
    finally:
        if temporary.is_symlink():
            temporary.unlink()


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


def persist_command_path(home, bin_dir):
    """Append one owned block; preserve existing profile contents and PATH entries."""
    from shlex import quote
    if any(char in str(bin_dir) for char in '\r\n:'):
        raise ValueError('Command directory contains a newline or colon')
    begin = '# >>> Amarcode remote PATH >>>\n'
    end = '# <<< Amarcode remote PATH <<<\n'
    quoted = quote(str(bin_dir))
    block = (begin + 'case ":$PATH:" in\n  *:' + quoted + ':*) ;;\n' +
             '  *) export PATH="$PATH":' + quoted + ' ;;\nesac\n' + end)
    login = next((home / name for name in ['.bash_profile', '.bash_login']
                  if (home / name).exists()), home / '.profile')
    zdot = Path(os.environ.get('ZDOTDIR', str(home)))
    profiles = {home / '.profile', login, home / '.bashrc', zdot / '.zshrc'}
    config = Path(os.environ.get('XDG_CONFIG_HOME', str(home / '.config')))
    fish = config / 'fish/conf.d/amarcode-remote.fish'
    profiles.add(fish)
    for path in sorted(profiles):
        original = path.read_text() if path.exists() else ''
        updated = original
        if begin in updated:
            start = updated.index(begin)
            if end not in updated[start:]:
                raise ValueError('Incomplete Amarcode remote PATH block in ' + str(path))
            finish = updated.index(end, start) + len(end)
            updated = updated[:start] + updated[finish:]
        contents = (begin + 'if not contains -- ' + quoted + ' $PATH\n' +
                    '    set -gx PATH $PATH ' + quoted + '\nend\n' + end) if path == fish else block
        if updated and not updated.endswith('\n'):
            updated += '\n'
        updated += contents
        if updated != original:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(updated)


def install_command(bin_dir, executable, root, legacy=None, name='amarcode-daemon'):
    bin_dir.mkdir(parents=True, exist_ok=True)
    command = bin_dir / name
    if os.path.lexists(command):
        resolved = command.resolve()
        owned = resolved.is_relative_to((root / 'daemon').resolve())
        if legacy is not None:
            owned = owned or resolved.is_relative_to((legacy / 'releases').resolve())
        if not command.is_symlink() or not owned:
            raise ValueError(str(command) + ' already exists outside this installation; choose another --bin-dir')
    temporary = bin_dir / ('.amarcode-daemon-' + secrets.token_hex(8))
    try:
        temporary.symlink_to(executable)
        os.replace(temporary, command)
    finally:
        if temporary.is_symlink():
            temporary.unlink()
    return command


def migrate_legacy(legacy, root):
    """Move an offline legacy instance once; old launchers then alias the same DB."""
    import fcntl
    source, target = legacy / 'data', root / 'data'
    pending = root / 'remote-migration-pending.json'
    old_token, new_token = legacy / 'access-token', root / 'access-token'
    if pending.exists():
        saved = json.loads(pending.read_text())
        if saved != {'source': str(source), 'target': str(target)}:
            raise ValueError('Unexpected pending remote migration')
        if old_token.exists() and not new_token.exists():
            private_write(new_token, old_token.read_text())
        if not source.exists() and target.is_dir():
            source.symlink_to(target, target_is_directory=True)
        if source.is_symlink() and source.resolve() == target.resolve():
            pending.unlink()
    if not source.exists() or (source.is_symlink() and source.resolve() == target.resolve()):
        return
    if source.is_symlink() or not source.is_dir():
        raise ValueError('Legacy data directory is redirected; migrate it manually')
    if not any(source.iterdir()):
        return
    if target.exists() and any(target.iterdir()):
        raise ValueError('Both desktop and legacy remote data exist. Stop both daemons and choose which instance to keep; automatic database merging is not supported. Both directories were preserved.')
    if old_token.exists() and new_token.exists() and old_token.read_bytes() != new_token.read_bytes():
        raise ValueError('Desktop and legacy remote access tokens conflict; both were preserved')
    lock = source / 'workspace.sqlite3.amarcode.lock'
    with lock.open('a+b') as ownership:
        try:
            fcntl.flock(ownership, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise ValueError('Stop the legacy remote daemon before migrating its database')
        if old_token.exists() and not new_token.exists():
            private_write(new_token, old_token.read_text())
        settings = legacy / 'remote.json'
        if settings.exists() and not (root / 'remote.json').exists():
            private_write(root / 'remote.json', json.dumps(json.loads(settings.read_text())))
        private_write(pending, json.dumps({'source': str(source), 'target': str(target)}))
        if target.exists():
            target.rmdir()
        source.rename(target)
        source.symlink_to(target, target_is_directory=True)
        pending.unlink()
    print('Migrated legacy remote data; old data paths now alias the same instance.')


def installed_release(root, temporary, target):
    """Verify and reuse the same current release consumed by desktop bootstrap."""
    record = root / 'daemon/current-release.json'
    if record.exists():
        saved = json.loads(record.read_text())
    elif (root / 'daemon/current-manifest.json').exists():
        saved = {'manifest': (root / 'daemon/current-manifest.json').read_text(),
                 'signature': (root / 'daemon/current-manifest.json.sig').read_text(),
                 'source_url': REGISTRY + '/v1/daemon/latest.json'}
    else:
        return None
    if 'bundled' in saved:
        release = saved['bundled']
        if release['target'] != target:
            raise ValueError('Installed daemon target is incompatible')
        parts = [release['version'], release['sha256']]
        if release.get('instance'):
            parts.append(release['instance'])
        if any(not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._+-]{0,127}', part) for part in parts):
            raise ValueError('Invalid installed bundle path')
        executable = root / 'daemon/bundled'
        for part in parts:
            executable /= part
        executable /= 'amarcode-daemon'
        artifact = release
    else:
        manifest_path, signature_path = temporary / 'current.json', temporary / 'current.sig'
        manifest_path.write_text(saved['manifest'])
        signature_path.write_text(saved['signature'])
        manifest = verify_manifest(manifest_path, signature_path, temporary)
        source = urllib.parse.urlsplit(saved['source_url'])
        if source.scheme != 'https' or source.netloc != urllib.parse.urlsplit(REGISTRY).netloc:
            raise ValueError('Installed daemon registry origin is invalid')
        version, artifact, _ = select_artifact(manifest, target, saved['source_url'])
        executable = root / 'daemon' / version / target / 'amarcode-daemon'
        release = {'version': version, 'protocol_version': manifest.get('protocolVersion')}
    if not executable.is_file() or executable.stat().st_size != artifact['size'] or hashlib.sha256(executable.read_bytes()).hexdigest().lower() != artifact['sha256'].lower():
        raise ValueError('Installed daemon verification failed; repair it before configuring remote access')
    probe = subprocess.run([str(executable), 'access', '--help'], check=True, capture_output=True, timeout=10)
    if b'enable' not in probe.stdout:
        raise ValueError('Upgrade the installed daemon before configuring unified remote access')
    return executable, release['version'], release.get('protocol_version')


def main():
    parser = argparse.ArgumentParser(description='Install Amarcode with secure remote access and agent runtimes (Linux)')
    parser.add_argument('--version', help='Initial signed release; default: latest. Existing releases use the managed updater')
    access = parser.add_mutually_exclusive_group()
    access.add_argument('--no-tunnel', '--ssh-only', dest='ssh_only', action='store_true', help='Set up authenticated loopback-only access without downloading a public connection helper')
    access.add_argument('--with-cloudflare', action='store_true', help=argparse.SUPPRESS)
    parser.add_argument('--cloudflared-version', help='Install a specific official cloudflared release; default: latest')
    parser.add_argument('--bun-version', help='Official Bun release tag; default: latest')
    parser.add_argument('--uv-version', help='Official uv release tag; default: latest')
    parser.add_argument('--bin-dir', type=Path, help='Optional additional command alias directory; the managed command always lives in the instance daemon/bin directory')
    parser.add_argument('--start', action='store_true', help='Start the daemon after installation and print its remote connection URL (loopback-only with --no-tunnel)')
    parser.add_argument('--tunnel', action='store_true', help=argparse.SUPPRESS)
    parser.add_argument('--no-service', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.ssh_only and (args.cloudflared_version or args.tunnel):
        parser.error('--ssh-only cannot be combined with --cloudflared-version or --tunnel')
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
    for label, value in [('Bun', args.bun_version), ('uv', args.uv_version)]:
        if value and not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._+-]{0,127}', value):
            parser.error('Invalid ' + label + ' version')
    manifest_url = REGISTRY + ('/v1/daemon/' + args.version + '/manifest.json' if args.version else '/v1/daemon/latest.json')
    data_home = Path(os.environ.get('XDG_DATA_HOME', str(Path.home() / '.local/share')))
    legacy = data_home / 'amarcode-remote'
    root = data_home / 'com.amarcode.desktop'
    if not root.is_absolute() or any(c in str(root) for c in '\n\r\0'):
        parser.error('XDG_DATA_HOME must be an absolute path without control characters')
    for name, expected in [('AMARCODE_APPDIR', root / 'data'),
                           ('AMARCODE_STORE_PATH', root / 'data/workspace.sqlite3'),
                           ('AMARCODE_REMOTE_ROOT', root),
                           ('AMARCODE_WS_TOKEN_FILE', root / 'access-token')]:
        if os.environ.get(name) and Path(os.environ[name]).expanduser().absolute() != expected:
            parser.error('Unset ' + name + ' before configuring the standard instance; use explicit manual setup for isolated instances')
    bin_dir = root / 'daemon/bin'
    alias_dir = args.bin_dir.expanduser().absolute() if args.bin_dir else None
    root.mkdir(parents=True, exist_ok=True, mode=0o700)
    os.chmod(root, 0o700)
    # Serialize with desktop bootstrap and update supervision. Do not switch a
    # running service behind its back: reuse its verified current release.
    import fcntl
    release_root = root / 'daemon'
    release_root.mkdir(exist_ok=True, mode=0o700)
    with (release_root / 'request.lock').open('a+b') as request_lock, (release_root / 'supervisor.lock').open('a+b') as supervisor_lock, tempfile.TemporaryDirectory(dir=root) as temporary:
        for lock in [request_lock, supervisor_lock]:
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                raise ValueError('A daemon install or update is active; retry when it finishes')
        if (release_root / 'bootstrap-activation-pending-v1').exists():
            raise ValueError('Finish or repair the interrupted desktop installation first')
        latest = release_root / 'updates/latest'
        if latest.exists():
            import uuid
            identifier = str(uuid.UUID(latest.read_text().strip()))
            job = json.loads((release_root / 'updates' / identifier / 'job.json').read_text())
            if not job['job']['finished']:
                raise ValueError('A daemon update is pending; finish it before running the installer')
        # Native request/supervisor locks protect the current release throughout.
        directory = Path(temporary)
        current = installed_release(root, directory, target)
        if current:
            executable, version, protocol = current
            if args.version and args.version != version:
                raise ValueError('Use the managed daemon update flow to change an existing release')
            print('Reusing managed daemon ' + version)
        else:
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
            probe = subprocess.run([str(binary), 'access', '--help'], check=True, capture_output=True, timeout=10)
            if b'enable' not in probe.stdout:
                raise ValueError('Downloaded binary does not support unified instance access')
            release = release_root / version / target
            release.mkdir(parents=True, exist_ok=True, mode=0o700)
            executable = release / 'amarcode-daemon'
            if executable.exists():
                if executable.stat().st_size != artifact['size'] or hashlib.sha256(executable.read_bytes()).hexdigest().lower() != artifact['sha256'].lower():
                    raise ValueError('Immutable release is damaged; repair it before installing')
            else:
                os.replace(binary, executable)
            private_write(release_root / 'current-manifest.json', manifest_path.read_text())
            private_write(release_root / 'current-manifest.json.sig', signature_path.read_text())
            private_write(release_root / 'current-release.json', json.dumps({
                'manifest': manifest_path.read_text(), 'signature': signature_path.read_text(),
                'source_url': manifest_url,
            }))
        migrate_legacy(legacy, root)
        command = install_command(bin_dir, executable, root)
        short_command = install_command(bin_dir, command, root, name='amarcoded')
        if alias_dir and alias_dir != bin_dir:
            install_command(alias_dir, command, root, legacy)
            install_command(alias_dir, short_command, root, legacy, name='amarcoded')
        old_alias = Path.home() / '.local/bin/amarcode-daemon'
        if old_alias.is_symlink() and old_alias.resolve().is_relative_to((legacy / 'releases').resolve()):
            install_command(old_alias.parent, command, root, legacy)
    cloudflared = install_cloudflared(root, architecture, args.cloudflared_version) if not args.ssh_only else None
    bun_bin = install_runtime(root, 'bun', architecture, args.bun_version)
    uv_bin = install_runtime(root, 'uv', architecture, args.uv_version)
    tool_bin_dirs = [str(bun_bin), str(uv_bin)]
    token_path = root / 'access-token'
    first_install = not token_path.exists()
    if first_install:
        private_write(token_path, secrets.token_hex(32) + '\n')
    os.chmod(token_path, 0o600)
    token = token_path.read_text().strip()
    if not re.fullmatch(r'[A-Za-z0-9._~-]{32,512}', token):
        raise ValueError('Existing access-token is invalid; fix it before restarting')
    settings_path = root / 'remote.json'
    settings = json.loads(settings_path.read_text()) if settings_path.exists() else {}
    settings.update({'enabled': True, 'tool_bin_dirs': tool_bin_dirs})
    if cloudflared is not None:
        settings['cloudflared'] = str(cloudflared)
    private_write(settings_path, json.dumps(settings) + '\n')
    persist_command_path(Path.home(), bin_dir)
    for name, directory in [('bun', bun_bin), ('bunx', bun_bin), ('uv', uv_bin), ('uvx', uv_bin)]:
        link_runtime(bin_dir, name, directory / name, root)
    import shlex
    quote = lambda value: shlex.quote(str(value))
    # Existing helper paths continue to use the same native command and lifecycle.
    for name, arguments in [('run.sh', '--remote --no-tunnel' if args.ssh_only else '--remote'), ('tunnel.sh', 'tunnel')]:
        launcher = root / name
        private_write(launcher, '#!/bin/sh\nset -eu\n' +
                      'export PATH=' + quote(os.pathsep.join(tool_bin_dirs)) + ':"$PATH"\n' +
                      'export AMARCODE_APPDIR=' + quote(root / 'data') + '\n' +
                      'export AMARCODE_WS_TOKEN_FILE=' + quote(token_path) + '\n' +
                      'exec ' + quote(command) + ' ' + arguments + '\n')
        os.chmod(launcher, 0o700)
    print('\nAmarcode remote setup complete. No service or tunnel has been started.')
    print('Daemon version: ' + version)
    print('Command: ' + str(command))
    print('Remote access: ' + ('ready' if cloudflared else 'loopback only'))
    print('Bun / bunx: ' + str(bun_bin))
    print('uv / uvx: ' + str(uv_bin))
    print('Data directory: ' + str(root / 'data'))
    print('Access token file: ' + str(token_path))
    if first_install:
        print('Access token (save privately): ' + token)
    else:
        print('Your existing access token and chat data have been preserved.')
    if str(bin_dir) not in os.get_exec_path():
        print('PATH saved for new Bash, Zsh, and Fish sessions. For this shell, run:')
        print('  export PATH=' + quote(bin_dir) + ':"$PATH"')
    print('\nStart whenever needed:')
    print('  amarcoded --remote --no-tunnel' if args.ssh_only else '  amarcode-daemon --remote')
    if not args.ssh_only:
        print('Copy the connection URL and access token it prints into Amarcode connection settings.')
    print('Local and remote connections share the same daemon and data.')
    print('If already running, restart the existing service to apply access settings: amarcode-daemon restart')
    print('For a managed service: amarcode-daemon bootstrap install.')
    print('Without systemd (Codespaces/containers), run the foreground command above inside tmux.')
    print('See https://amarcode.amarjay.com/docs/remote/setup/#codespaces-and-containers-without-systemd')
    if not args.ssh_only:
        print('If the daemon is already running, attach remote access with: amarcode-daemon tunnel')
    if args.start or args.tunnel:
        os.environ.pop('AMARCODE_REMOTE_ROOT', None)
        os.environ['AMARCODE_APPDIR'] = str(root / 'data')
        os.environ['AMARCODE_WS_TOKEN_FILE'] = str(token_path)
        os.environ['PATH'] = os.pathsep.join(tool_bin_dirs + [os.environ.get('PATH', '')])
        sys.stdout.flush()
        os.execv(str(command), [str(command)] + (['--remote', '--no-tunnel'] if args.ssh_only else ['--remote']))


if __name__ == '__main__':
    try:
        main()
    except (Exception, KeyboardInterrupt) as error:
        print('Installation failed: ' + str(error), file=sys.stderr)
        sys.exit(1)
PY
