"""Exercise the shipped installer with signed fixtures and no host/service changes."""
import base64
import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import types
import unittest
from unittest.mock import patch

SOURCE = Path(__file__).resolve().parents[1] / 'crates/landing/public/install.sh'


class RemoteInstallerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.module = types.ModuleType('installer')
        source = SOURCE.read_text().split("<<'PY'\n", 1)[1].rsplit('\nPY', 1)[0]
        exec(compile(source, str(SOURCE), 'exec'), self.module.__dict__)
        self.key = self.root / 'signing.pem'
        subprocess.run(['openssl', 'genpkey', '-algorithm', 'ED25519', '-out', str(self.key)], check=True, capture_output=True)
        public = subprocess.run(['openssl', 'pkey', '-in', str(self.key), '-pubout', '-outform', 'DER'], check=True, capture_output=True).stdout
        self.module.PUBLIC_KEY = public[-32:].hex()
        self.binary = b'#!/bin/sh\necho "Amarcode run --transport --remote"\n'
        self.manifest = {'version': '1.2.3', 'protocolVersion': 12, 'capabilities': ['authenticated-websocket', 'remote-cli'], 'artifacts': {
            'x86_64-unknown-linux-gnu': {'target': 'x86_64-unknown-linux-gnu', 'size': len(self.binary),
                                      'sha256': hashlib.sha256(self.binary).hexdigest(),
                                      'url': '/v1/daemon/1.2.3/x86_64-unknown-linux-gnu'}}}

        self.cloudflared = b'#!/bin/sh\necho "cloudflared version fixture"\n'
        self.cloudflared_release = {'tag_name': '2026.10.3', 'assets': [
            {'name': 'cloudflared-linux-' + suffix,
             'size': len(self.cloudflared),
             'digest': 'sha256:' + hashlib.sha256(self.cloudflared).hexdigest(),
             'browser_download_url': 'https://github.com/cloudflare/cloudflared/releases/download/2026.10.3/cloudflared-linux-' + suffix}
            for suffix in ['amd64', 'arm64']
        ]}

        self.runtime_downloads = {}
        for tool, repo, version in [('bun', 'oven-sh/bun', 'bun-v1.4.4'), ('uv', 'astral-sh/uv', '0.13.5')]:
            assets = []
            for architecture in ['x86_64', 'aarch64']:
                _, asset_name, members = self.module.runtime_layout(tool, architecture)
                payload = io.BytesIO()
                if tool == 'bun':
                    import zipfile
                    with zipfile.ZipFile(payload, 'w') as package:
                        for name, member in members.items():
                            package.writestr(member, b'#!/bin/sh\necho bun-fixture\n')
                        package.writestr('../../outside', 'must not be extracted')
                else:
                    import tarfile
                    with tarfile.open(fileobj=payload, mode='w:gz') as package:
                        for name, member in members.items():
                            body = b'#!/bin/sh\necho uv-fixture\n'
                            entry = tarfile.TarInfo(member)
                            entry.size = len(body)
                            package.addfile(entry, io.BytesIO(body))
                archive = payload.getvalue()
                url = 'https://github.com/' + repo + '/releases/download/' + version + '/' + asset_name
                assets.append({'name': asset_name, 'size': len(archive), 'digest': 'sha256:' + hashlib.sha256(archive).hexdigest(), 'browser_download_url': url})
                self.runtime_downloads[url] = archive
            metadata = json.dumps({'tag_name': version, 'assets': assets}).encode()
            self.runtime_downloads['https://api.github.com/repos/' + repo + '/releases/latest'] = metadata
            self.runtime_downloads['https://api.github.com/repos/' + repo + '/releases/tags/' + version] = metadata

    def tearDown(self):
        self.temporary.cleanup()

    def signed(self):
        path = self.root / 'manifest.json'
        path.write_text(json.dumps(self.manifest))
        signature = subprocess.run(['openssl', 'pkeyutl', '-sign', '-inkey', str(self.key), '-rawin', '-in', str(path)], check=True, capture_output=True).stdout
        signed = self.root / 'manifest.sig'
        signed.write_text(base64.b64encode(signature).decode())
        return path, signed

    def test_signature_and_tampering(self):
        manifest, signature = self.signed()
        self.assertEqual(self.module.verify_manifest(manifest, signature, self.root)['version'], '1.2.3')
        manifest.write_text(manifest.read_text().replace('1.2.3', '9.9.9'))
        with self.assertRaisesRegex(ValueError, 'signature verification failed'):
            self.module.verify_manifest(manifest, signature, self.root)

    def test_artifact_origin_target_and_size_validation(self):
        target = 'x86_64-unknown-linux-gnu'
        url = self.module.REGISTRY + '/v1/daemon/latest.json'
        self.module.select_artifact(self.manifest, target, url)
        for field, value in [('url', 'https://evil.example/binary'), ('size', 0), ('target', 'wrong'), ('sha256', 'invalid')]:
            artifact = self.manifest['artifacts'][target]
            original = artifact[field]
            artifact[field] = value
            with self.assertRaises(ValueError):
                self.module.select_artifact(self.manifest, target, url)
            artifact[field] = original

    def test_legacy_release_is_rejected_before_any_binary_runs(self):
        self.manifest.pop('capabilities')
        with self.assertRaisesRegex(ValueError, 'predates authenticated remote CLI support'):
            self.module.select_artifact(self.manifest, 'x86_64-unknown-linux-gnu', self.module.REGISTRY + '/v1/daemon/latest.json')

    def test_cloudflared_asset_selection_checks_architecture_origin_version_and_digest(self):
        for architecture, suffix in [('x86_64', 'amd64'), ('aarch64', 'arm64')]:
            version, url, size, digest = self.module.select_cloudflared_asset(self.cloudflared_release, architecture)
            self.assertEqual(version, '2026.10.3')
            self.assertTrue(url.endswith('cloudflared-linux-' + suffix))
            self.assertEqual(size, len(self.cloudflared))
            self.assertEqual(digest, hashlib.sha256(self.cloudflared).hexdigest())
        with self.assertRaisesRegex(ValueError, 'requested version'):
            self.module.select_cloudflared_asset(self.cloudflared_release, 'x86_64', '2026.1.1')
        asset = self.cloudflared_release['assets'][0]
        for field, value in [('browser_download_url', 'https://evil.example/cloudflared'), ('size', 0), ('digest', ''), ('digest', 'sha256:bad')]:
            original = asset[field]
            asset[field] = value
            with self.assertRaises(ValueError):
                self.module.select_cloudflared_asset(self.cloudflared_release, 'x86_64')
            asset[field] = original

    def test_cloudflared_download_is_checked_before_execution_and_cached(self):
        downloads = []
        def download(url, destination, limit):
            downloads.append(url)
            Path(destination).write_bytes(json.dumps(self.cloudflared_release).encode() if 'api.github.com' in url else self.cloudflared)
        with patch.object(self.module, 'download', download), contextlib.redirect_stdout(io.StringIO()):
            binary = self.module.install_cloudflared(self.root, 'x86_64', '2026.10.3')
            self.assertEqual(binary.read_bytes(), self.cloudflared)
            self.module.install_cloudflared(self.root, 'x86_64', '2026.10.3')
            self.assertEqual(sum('releases/download/' in url for url in downloads), 1)
            self.assertTrue(any('releases/tags/2026.10.3' in url for url in downloads))
            self.assertEqual(binary.stat().st_mode & 0o077, 0)
            # Corrupt the cached artifact and return corrupt replacement bytes.
            binary.write_bytes(b'bad')
            def corrupt_download(url, destination, limit):
                Path(destination).write_bytes(json.dumps(self.cloudflared_release).encode() if 'api.github.com' in url else b'bad')
            with patch.object(self.module, 'download', corrupt_download), self.assertRaisesRegex(ValueError, 'checksum verification failed'):
                self.module.install_cloudflared(self.root, 'x86_64')

    def test_runtime_archives_install_companions_cache_and_repair_for_both_architectures(self):
        for architecture in ['x86_64', 'aarch64']:
            downloads = []
            def download(url, destination, limit):
                downloads.append(url)
                Path(destination).write_bytes(self.runtime_downloads[url])
            with patch.object(self.module, 'download', download), contextlib.redirect_stdout(io.StringIO()):
                for tool, version in [('bun', 'bun-v1.4.4'), ('uv', '0.13.5')]:
                    bin_dir = self.module.install_runtime(self.root, tool, architecture, version)
                    names = ['bun', 'bunx'] if tool == 'bun' else ['uv', 'uvx']
                    for name in names:
                        self.assertTrue((bin_dir / name).is_file())
                        self.assertEqual((bin_dir / name).stat().st_mode & 0o077, 0)
                    self.module.install_runtime(self.root, tool, architecture, version)
                    self.assertEqual(sum('/releases/download/' in url and tool in url for url in downloads), 1)
                    (bin_dir / tool).write_bytes(b'corrupt')
                    self.module.install_runtime(self.root, tool, architecture, version)
                    self.assertNotEqual((bin_dir / tool).read_bytes(), b'corrupt')
            self.assertFalse((self.root / 'outside').exists())

    def test_runtime_download_checksum_and_origin_are_required_before_execution(self):
        endpoint = 'https://api.github.com/repos/oven-sh/bun/releases/latest'
        release = json.loads(self.runtime_downloads[endpoint])
        def download(url, destination, limit):
            Path(destination).write_bytes(json.dumps(release).encode() if url == endpoint else b'corrupt')
        with patch.object(self.module, 'download', download), contextlib.redirect_stdout(io.StringIO()), patch.object(self.module.subprocess, 'run') as execute:
            with self.assertRaisesRegex(ValueError, 'checksum verification failed'):
                self.module.install_runtime(self.root, 'bun', 'x86_64')
            execute.assert_not_called()
            release['assets'][0]['browser_download_url'] = 'https://evil.example/bun.zip'
            with self.assertRaisesRegex(ValueError, 'official release repository'):
                self.module.install_runtime(self.root, 'bun', 'x86_64')
            execute.assert_not_called()

    def test_runtime_links_preserve_unrelated_user_installations(self):
        original = self.root / 'bun'
        original.write_text('user-owned bun')
        self.module.link_runtime(self.root, 'bun', self.root / 'new-bun', self.root)
        self.assertEqual(original.read_text(), 'user-owned bun')

    def test_existing_commands_outside_installation_are_preserved(self):
        binary = self.root / 'amarcode-daemon'
        binary.write_text('existing command')
        with self.assertRaisesRegex(ValueError, 'already exists outside this installation'):
            self.module.install_command(self.root, self.root / 'new-daemon', self.root)
        self.assertEqual(binary.read_text(), 'existing command')

    def test_bash_pipe_accepts_arguments_without_consuming_script_as_user_input(self):
        subprocess.run(['bash', '-n', str(SOURCE)], check=True)
        output = subprocess.run(['bash', '-s', '--', '--help'], input=SOURCE.read_bytes(), capture_output=True, check=True)
        self.assertIn(b'--cloudflared-version', output.stdout)
        self.assertIn(b'--tunnel', output.stdout)

    def test_install_and_reinstall_preserve_token_and_write_private_launchers(self):
        manifest, signature = self.signed()
        def download(url, destination, limit):
            if url in self.runtime_downloads:
                payload = self.runtime_downloads[url]
            elif url.startswith('https://api.github.com/repos/cloudflare/cloudflared/releases/'):
                payload = json.dumps(self.cloudflared_release).encode()
            elif url.startswith('https://github.com/cloudflare/cloudflared/releases/download/'):
                payload = self.cloudflared
            else:
                payload = signature.read_bytes() if url.endswith('.sig') else manifest.read_bytes() if url.endswith('.json') else self.binary
            Path(destination).write_bytes(payload)
        captured = io.StringIO()
        data = self.root / "data with ' spaces"
        with patch.dict(os.environ, {'XDG_DATA_HOME': str(data)}), patch.object(sys, 'argv', ['installer', '--bin-dir', str(self.root / 'bin')]), patch.object(self.module, 'download', download), patch.object(self.module.platform, 'system', return_value='Linux'), patch.object(self.module.platform, 'machine', return_value='x86_64'), contextlib.redirect_stdout(captured):
            self.module.main()
            installed = data / 'amarcode-remote'
            token = (installed / 'access-token').read_text()
            self.module.main()
            self.assertEqual(token, (installed / 'access-token').read_text())
        self.assertEqual(captured.getvalue().count('Access token (save privately):'), 1)
        self.assertIn('amarcode-daemon --remote', captured.getvalue())
        self.assertIn('No service or tunnel has been started', captured.getvalue())
        for name in ['access-token', 'remote.json', 'run.sh', 'tunnel.sh']:
            self.assertEqual((installed / name).stat().st_mode & 0o077, 0)
        launcher = installed / 'run.sh'
        subprocess.run(['sh', '-n', str(launcher)], check=True)
        output = subprocess.run([str(launcher)], check=True, capture_output=True)
        self.assertIn(b'Amarcode run', output.stdout)
        self.assertIn('AMARCODE_WS_TOKEN_FILE', launcher.read_text())
        self.assertIn('--transport websocket', launcher.read_text())
        tunnel = installed / 'tunnel.sh'
        output = subprocess.run([str(tunnel)], check=True, capture_output=True)
        self.assertIn(b'Amarcode run --transport --remote', output.stdout)
        import shlex
        executable = Path(shlex.split(next(line for line in tunnel.read_text().splitlines() if line.startswith('exec ')))[1])
        self.assertEqual(executable, self.root / 'bin/amarcode-daemon')
        self.assertTrue(executable.is_symlink())
        self.assertIn('--remote', tunnel.read_text())
        setup = json.loads((installed / 'remote.json').read_text())
        self.assertTrue(Path(setup['cloudflared']).is_relative_to(installed / 'cloudflared'))
        self.assertEqual(len(setup['tool_bin_dirs']), 2)
        for tool in ['bun', 'bunx', 'uv', 'uvx']:
            runtime = self.root / 'bin' / tool
            self.assertTrue(runtime.is_symlink())
            self.assertTrue(runtime.resolve().is_relative_to(installed / 'runtimes'))
            subprocess.run([str(runtime), '--version'], check=True, capture_output=True)
        self.assertIn('export PATH=', launcher.read_text())


if __name__ == '__main__':
    unittest.main()
