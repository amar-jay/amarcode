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

SOURCE = Path(__file__).resolve().parents[1] / 'crates/amarcode-registry/assets/install.sh'


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
        self.binary = b'#!/bin/sh\necho "Amarcode run --transport --remote access enable tunnel"\n'
        self.manifest = {'version': '1.2.3', 'protocolVersion': 12, 'capabilities': ['authenticated-websocket', 'remote-cli', 'unified-instance'], 'artifacts': {
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

    def test_current_path_aliases_work_without_sourcing_and_follow_releases(self):
        home = self.root / 'home'
        home.mkdir()
        aliases = home / 'bin'
        aliases.mkdir()
        root, legacy = home / 'managed', home / 'legacy'
        bin_dir = root / 'daemon/bin'
        executable = root / 'daemon/v1/amarcode-daemon'
        executable.parent.mkdir(parents=True)
        executable.write_text('#!/bin/sh\nprintf "%s\n" "$@"\n')
        executable.chmod(0o700)
        with patch.dict(os.environ, {'PATH': str(aliases) + ':/usr/bin:/bin'}):
            directory = self.module.current_shell_alias_dir(home, root, legacy)
            self.assertEqual(directory, aliases)
            managed = self.module.install_command(bin_dir, executable, root)
            short = self.module.install_command(bin_dir, managed, root, name='amarcoded')
            self.module.install_command(directory, managed, root)
            self.module.install_command(directory, short, root, name='amarcoded')
            self.assertEqual(self.module.start_command(short), 'amarcoded --remote')
            result = subprocess.run(['amarcoded', '--remote'], check=True, capture_output=True)
            self.assertEqual(result.stdout, b'--remote\n')
            # Stable aliases follow the managed pointer, not a particular release.
            next_release = root / 'daemon/v2/amarcode-daemon'
            next_release.parent.mkdir()
            next_release.write_text('#!/bin/sh\necho next-version\n')
            next_release.chmod(0o700)
            self.module.install_command(bin_dir, next_release, root)
            result = subprocess.run(['amarcoded', '--version'], check=True, capture_output=True)
            self.assertEqual(result.stdout, b'next-version\n')

    def test_auto_alias_skips_other_programs_system_and_relative_paths(self):
        home = self.root / 'home'
        home.mkdir()
        taken, usable = home / 'taken', home / 'usable'
        for directory in [taken, usable]:
            directory.mkdir()
        other = taken / 'amarcoded'
        other.write_text('unrelated application')
        root, legacy = home / 'managed', home / 'legacy'
        with patch.dict(os.environ, {'PATH': ':.:/usr/bin:' + str(taken) + ':' + str(usable)}):
            self.assertEqual(self.module.current_shell_alias_dir(home, root, legacy), usable)
        self.assertEqual(other.read_text(), 'unrelated application')
        with patch.dict(os.environ, {'PATH': '/usr/bin:.'}):
            self.assertIsNone(self.module.current_shell_alias_dir(home, root, legacy))
        with patch.dict(os.environ, {'PATH': str(usable)}), patch.object(self.module.os, 'access', return_value=False):
            self.assertIsNone(self.module.current_shell_alias_dir(home, root, legacy))

    def test_start_command_falls_back_to_a_shell_safe_path_and_preserves_foreign_command(self):
        import shlex
        home = self.root / "home with ' quote"
        root, legacy = home / 'managed', home / 'legacy'
        executable = root / 'daemon/v1/amarcode-daemon'
        executable.parent.mkdir(parents=True)
        executable.write_text('#!/bin/sh\nprintf "%s\n" "$@"\n')
        executable.chmod(0o700)
        command = self.module.install_command(root / 'daemon/bin', executable, root, name='amarcoded')
        with patch.dict(os.environ, {'PATH': '/usr/bin:/bin'}):
            text = self.module.start_command(command, True)
            self.assertEqual(shlex.split(text), [str(command), '--remote', '--no-tunnel'])
            result = subprocess.run(['sh', '-c', text], check=True, capture_output=True)
            self.assertEqual(result.stdout, b'--remote\n--no-tunnel\n')
        other = home / 'bin'
        other.mkdir()
        (other / 'amarcoded').write_text('#!/bin/sh\necho unrelated\n')
        (other / 'amarcoded').chmod(0o700)
        with patch.dict(os.environ, {'PATH': str(other) + ':/usr/bin:/bin'}):
            self.assertEqual(shlex.split(self.module.start_command(command))[0], str(command))

    def test_plain_ui_summary_is_short_and_verbose_is_opt_in(self):
        # Minimal Python installations do not need pip or Rich to install.
        for verbose in [False, True]:
            output = io.StringIO()
            with patch.dict(sys.modules, {'rich.console': None}), contextlib.redirect_stdout(output):
                self.module.ui = self.module.InstallerUI(verbose)
                for label in ['Daemon', 'Remote access', 'Agent tools']:
                    with self.module.ui.step(label):
                        self.module.ui.detail('Downloading a verified dependency')
                self.module.installation_summary('1.2.3', self.root / 'bin/amarcoded',
                    self.root, self.root / 'bun', self.root / 'uv', False, False)
            text = output.getvalue()
            self.assertNotIn('\x1b', text)
            self.assertIn('[3/3] Agent tools', text)
            self.assertIn('Start the daemon:', text)
            self.assertEqual('Data directory:' in text, verbose)
            self.assertEqual('Downloading a verified dependency' in text, verbose)
            if not verbose:
                self.assertLessEqual(len(text.splitlines()), 10)

    def test_rich_spinner_stops_on_success_and_failure(self):
        console_module = types.ModuleType('rich.console')
        status = []
        class Console:
            is_terminal = True
            def __init__(self, **options):
                self.options = options
            def print(self, message, **options):
                status.append(message)
            @contextlib.contextmanager
            def status(self, message, **options):
                status.append('start')
                try:
                    yield
                finally:
                    status.append('stop')
        console_module.Console = Console
        with patch.dict(sys.modules, {'rich.console': console_module}):
            ui = self.module.InstallerUI()
            self.assertFalse(ui.console.options['markup'])
            with ui.step('Daemon'):
                pass
            with self.assertRaisesRegex(ValueError, 'network failure'):
                with ui.step('Remote access'):
                    raise ValueError('network failure')
        self.assertEqual(status, ['start', 'stop', '[1/3] Daemon done',
                                  'start', 'stop', '[2/3] Remote access failed'])

    def test_persistent_path_is_idempotent_and_shell_safe(self):
        home = self.root / 'home'
        home.mkdir()
        profile = home / '.bash_profile'
        profile.write_text('# preserve existing configuration\n')
        bin_dir = self.root / "bin with ' quote [glob]"
        bin_dir.mkdir()
        command = bin_dir / 'amarcode-daemon'
        command.write_text('#!/bin/sh\nprintf "%s\\n" "$@"\n')
        command.chmod(0o700)
        with patch.dict(os.environ, {'ZDOTDIR': str(home), 'XDG_CONFIG_HOME': str(home / '.config')}):
            self.module.persist_command_path(home, bin_dir)
            first = profile.read_text()
            self.module.persist_command_path(home, bin_dir)
        self.assertEqual(first, profile.read_text())
        self.assertTrue(first.startswith('# preserve existing configuration\n'))
        result = subprocess.run(['sh', '-c', '. "$1"; amarcode-daemon "two words" --version',
                                 'sh', str(home / '.profile')],
                                env={'PATH': '/usr/bin:/bin'}, capture_output=True, check=True)
        self.assertEqual(result.stdout, b'two words\n--version\n')

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
        with self.assertRaisesRegex(ValueError, 'predates authenticated unified-instance CLI support'):
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
        self.assertIn(b'--ssh-only', output.stdout)
        self.assertIn(b'--start', output.stdout)

    def test_offline_migration_preserves_data_token_and_aliases_old_paths(self):
        legacy, root = self.root / 'amarcode-remote', self.root / 'com.amarcode.desktop'
        (legacy / 'data/attachments').mkdir(parents=True)
        root.mkdir()
        (legacy / 'data/workspace.sqlite3').write_bytes(b'fixture database')
        (legacy / 'data/attachments/file').write_text('attachment')
        (legacy / 'access-token').write_text('a' * 64)
        self.module.migrate_legacy(legacy, root)
        self.module.migrate_legacy(legacy, root)
        self.assertEqual((root / 'data/workspace.sqlite3').read_bytes(), b'fixture database')
        self.assertEqual((root / 'data/attachments/file').read_text(), 'attachment')
        self.assertEqual((root / 'access-token').read_text(), 'a' * 64)
        self.assertEqual((legacy / 'data').resolve(), (root / 'data').resolve())

    def test_migration_refuses_active_and_conflicting_databases(self):
        import fcntl
        legacy, root = self.root / 'legacy', self.root / 'shared'
        (legacy / 'data').mkdir(parents=True)
        root.mkdir()
        source = legacy / 'data/workspace.sqlite3'
        source.write_bytes(b'legacy')
        with (legacy / 'data/workspace.sqlite3.amarcode.lock').open('a+b') as owner:
            fcntl.flock(owner, fcntl.LOCK_EX | fcntl.LOCK_NB)
            with self.assertRaisesRegex(ValueError, 'Stop the legacy remote daemon'):
                self.module.migrate_legacy(legacy, root)
        (root / 'data').mkdir()
        (root / 'data/workspace.sqlite3').write_bytes(b'desktop')
        with self.assertRaisesRegex(ValueError, 'Both desktop and legacy remote data exist'):
            self.module.migrate_legacy(legacy, root)
        self.assertEqual(source.read_bytes(), b'legacy')
        self.assertEqual((root / 'data/workspace.sqlite3').read_bytes(), b'desktop')

    def test_ssh_only_reuses_desktop_release_without_daemon_or_connector_download(self):
        base = self.root / 'data'
        installed = base / 'com.amarcode.desktop'
        digest = hashlib.sha256(self.binary).hexdigest()
        executable = installed / 'daemon/bundled/9.0.0' / digest / 'amarcode-daemon'
        executable.parent.mkdir(parents=True)
        executable.write_bytes(self.binary)
        executable.chmod(0o700)
        record = installed / 'daemon/current-release.json'
        record.write_text(json.dumps({'bundled': {
            'version': '9.0.0', 'protocol_version': 13,
            'target': 'x86_64-unknown-linux-gnu', 'sha256': digest, 'size': len(self.binary),
        }}))
        original = record.read_bytes()
        tools = installed / 'runtimes/fixture/bin'
        tools.mkdir(parents=True)
        for name in ['bun', 'bunx', 'uv', 'uvx']:
            (tools / name).write_text('#!/bin/sh\nexit 0\n')
            (tools / name).chmod(0o700)
        environment = {'HOME': str(self.root / 'home'), 'XDG_DATA_HOME': str(base),
                       'XDG_CONFIG_HOME': str(self.root / 'config'), 'ZDOTDIR': str(self.root / 'zsh')}
        with patch.dict(os.environ, environment), patch.object(sys, 'argv', ['installer', '--ssh-only']), patch.object(self.module, 'download') as download, patch.object(self.module, 'install_runtime', return_value=tools), patch.object(self.module.platform, 'system', return_value='Linux'), patch.object(self.module.platform, 'machine', return_value='x86_64'), contextlib.redirect_stdout(io.StringIO()):
            self.module.main()
        download.assert_not_called()
        self.assertEqual(record.read_bytes(), original)
        self.assertEqual((installed / 'daemon/bin/amarcode-daemon').resolve(), executable)
        self.assertEqual((installed / 'daemon/bin/amarcoded').resolve(), executable)
        self.assertTrue((installed / 'run.sh').read_text().endswith(' --remote --no-tunnel\n'))
        settings = json.loads((installed / 'remote.json').read_text())
        self.assertEqual(settings['enabled'], True)
        self.assertNotIn('cloudflared', settings)

    def test_interrupted_migration_resumes_and_token_conflicts_are_preserved(self):
        legacy, root = self.root / 'legacy', self.root / 'shared'
        (legacy / 'data').mkdir(parents=True)
        root.mkdir()
        (legacy / 'data/workspace.sqlite3').write_bytes(b'legacy database')
        (legacy / 'access-token').write_text('a' * 64)
        (root / 'access-token').write_text('b' * 64)
        with self.assertRaisesRegex(ValueError, 'access tokens conflict'):
            self.module.migrate_legacy(legacy, root)
        self.assertEqual((root / 'access-token').read_text(), 'b' * 64)
        (root / 'access-token').unlink()
        pending = root / 'remote-migration-pending.json'
        pending.write_text(json.dumps({'source': str(legacy / 'data'), 'target': str(root / 'data')}))
        (legacy / 'data').rename(root / 'data')
        self.module.migrate_legacy(legacy, root)
        self.assertFalse(pending.exists())
        self.assertEqual((root / 'access-token').read_text(), 'a' * 64)
        self.assertEqual((legacy / 'data').resolve(), (root / 'data').resolve())
        self.assertEqual((root / 'data/workspace.sqlite3').read_bytes(), b'legacy database')

    def test_ssh_only_rejects_conflicting_public_access_options_before_installing(self):
        for options in [['--with-cloudflare'], ['--cloudflared-version', '2026.10.3'], ['--tunnel']]:
            with patch.object(sys, 'argv', ['installer', '--ssh-only'] + options), patch.object(self.module, 'download') as download, contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit) as error:
                    self.module.main()
                self.assertEqual(error.exception.code, 2)
                download.assert_not_called()

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
        with patch.dict(os.environ, {'HOME': str(self.root / 'home'), 'XDG_DATA_HOME': str(data), 'XDG_CONFIG_HOME': str(self.root / 'config'), 'ZDOTDIR': str(self.root / 'zsh')}), patch.object(sys, 'argv', ['installer', '--bin-dir', str(self.root / 'bin')]), patch.object(self.module, 'download', download), patch.object(self.module.platform, 'system', return_value='Linux'), patch.object(self.module.platform, 'machine', return_value='x86_64'), contextlib.redirect_stdout(captured):
            self.module.main()
            installed = data / 'com.amarcode.desktop'
            token = (installed / 'access-token').read_text()
            self.module.main()
            self.assertEqual(token, (installed / 'access-token').read_text())
            for options, mode in [(['--start'], '--remote'), (['--start', '--ssh-only'], '--remote --no-tunnel'), (['--start', '--no-tunnel'], '--remote --no-tunnel'), (['--tunnel'], '--remote')]:
                with patch.object(sys, 'argv', ['installer'] + options), patch.object(self.module.os, 'execv') as execute:
                    self.module.main()
                execute.assert_called_once_with(str(installed / 'daemon/bin/amarcode-daemon'),
                                                [str(installed / 'daemon/bin/amarcode-daemon')] + mode.split())
                self.assertEqual(token, (installed / 'access-token').read_text())
                self.assertTrue((installed / 'run.sh').read_text().endswith(' ' + mode + '\n'))
        self.assertNotIn(token.strip(), captured.getvalue())
        import shlex
        commands = [shlex.split(line.strip()) for line in captured.getvalue().splitlines()
                    if line.startswith('  ')]
        self.assertIn([str(installed / 'daemon/bin/amarcoded'), '--remote'], commands)
        self.assertIn('Start the daemon:', captured.getvalue())
        self.assertNotIn('Bun / bunx:', captured.getvalue())
        self.assertNotIn('For a managed service:', captured.getvalue())
        for name in ['access-token', 'remote.json', 'run.sh', 'tunnel.sh']:
            self.assertEqual((installed / name).stat().st_mode & 0o077, 0)
        launcher = installed / 'run.sh'
        subprocess.run(['sh', '-n', str(launcher)], check=True)
        output = subprocess.run([str(launcher)], check=True, capture_output=True)
        self.assertIn(b'Amarcode run', output.stdout)
        self.assertIn('AMARCODE_WS_TOKEN_FILE', launcher.read_text())
        self.assertIn(' --remote', launcher.read_text())
        self.assertNotIn('AMARCODE_REMOTE_ROOT', launcher.read_text())
        tunnel = installed / 'tunnel.sh'
        output = subprocess.run([str(tunnel)], check=True, capture_output=True)
        self.assertIn(b'Amarcode run --transport --remote', output.stdout)
        import shlex
        executable = Path(shlex.split(next(line for line in tunnel.read_text().splitlines() if line.startswith('exec ')))[1])
        self.assertEqual(executable, installed / 'daemon/bin/amarcode-daemon')
        self.assertEqual((self.root / 'bin/amarcode-daemon').readlink(), executable)
        self.assertEqual((self.root / 'bin/amarcoded').readlink(), installed / 'daemon/bin/amarcoded')
        self.assertEqual((installed / 'daemon/bin/amarcoded').resolve(), executable.resolve())
        record = json.loads((installed / 'daemon/current-release.json').read_text())
        self.assertEqual(json.loads(record['manifest'])['version'], '1.2.3')
        self.assertTrue(executable.is_symlink())
        self.assertIn(' tunnel', tunnel.read_text())
        setup = json.loads((installed / 'remote.json').read_text())
        self.assertTrue(Path(setup['cloudflared']).is_relative_to(installed / 'cloudflared'))
        self.assertEqual(len(setup['tool_bin_dirs']), 2)
        for tool in ['bun', 'bunx', 'uv', 'uvx']:
            runtime = installed / 'daemon/bin' / tool
            self.assertTrue(runtime.is_symlink())
            self.assertTrue(runtime.resolve().is_relative_to(installed / 'runtimes'))
            subprocess.run([str(runtime), '--version'], check=True, capture_output=True)
        self.assertIn('export PATH=', launcher.read_text())

    def test_download_authenticates_only_the_github_api(self):
        cases = [
            ('https://api.github.com/repos/cloudflare/cloudflared/releases/latest', 'primary-token'),
            ('https://api.github.com:443/repos/oven-sh/bun/releases/latest', 'primary-token'),
            ('https://github.com/cloudflare/cloudflared/releases/download/v1/asset', None),
            ('https://updates.amarcode.amarjay.com/v1/daemon/latest', None),
            ('https://api.github.com.evil.example/releases', None),
        ]
        for url, expected in cases:
            with self.subTest(url=url):
                response = io.BytesIO(b'fixture')
                response.url = url
                with patch.dict(os.environ, {'GH_TOKEN': 'primary-token', 'GITHUB_TOKEN': 'secondary-token'}), patch.object(self.module.urllib.request, 'build_opener') as build:
                    build.return_value.open.return_value = response
                    self.module.download(url, self.root / 'download', 100)
                    request = build.return_value.open.call_args.args[0]
                    self.assertEqual(request.get_header('Authorization'), 'Bearer ' + expected if expected else None)
                    self.assertEqual((self.root / 'download').read_bytes(), b'fixture')

    def test_download_uses_codespaces_token_when_gh_token_is_empty(self):
        for token, expected in [('codespaces-token', 'Bearer codespaces-token'), ('', None)]:
            response = io.BytesIO(b'fixture')
            response.url = 'https://api.github.com/repos/astral-sh/uv/releases/latest'
            with patch.dict(os.environ, {'GH_TOKEN': '', 'GITHUB_TOKEN': token}), patch.object(self.module.urllib.request, 'build_opener') as build:
                build.return_value.open.return_value = response
                self.module.download(response.url, self.root / 'download', 100)
                request = build.return_value.open.call_args.args[0]
                self.assertEqual(request.get_header('Authorization'), expected)

    def test_redirects_strip_credentials_and_reject_https_downgrades(self):
        request = self.module.urllib.request.Request('https://api.github.com/repos/example/release', headers={'Authorization': 'Bearer private-token'})
        handler = self.module.DownloadRedirectHandler()
        for url, expected in [('https://api.github.com/new-location', 'Bearer private-token'), ('https://release-assets.githubusercontent.com/asset', None), ('https://api.github.com.evil.example/asset', None)]:
            redirected = handler.redirect_request(request, None, 302, 'Found', {}, url)
            self.assertEqual(redirected.get_header('Authorization'), expected)
        with self.assertRaisesRegex(ValueError, 'HTTPS'):
            handler.redirect_request(request, None, 302, 'Found', {}, 'http://api.github.com/asset')

    def test_rate_limit_error_explains_authentication_and_reset_without_exposing_tokens(self):
        url = 'https://api.github.com/repos/cloudflare/cloudflared/releases/latest'
        for status, headers in [(403, {'X-RateLimit-Remaining': '0', 'X-RateLimit-Reset': '1700000000'}), (429, {}), (403, {'Retry-After': '60'})]:
            with patch.dict(os.environ, {'GH_TOKEN': 'private-token'}), patch.object(self.module.urllib.request, 'build_opener') as build:
                build.return_value.open.side_effect = self.module.urllib.error.HTTPError(url, status, 'Forbidden', headers, None)
                with self.assertRaisesRegex(ValueError, 'GitHub API rate limit exceeded') as error:
                    self.module.download(url, self.root / 'download', 100)
                self.assertIn('GH_TOKEN or GITHUB_TOKEN', str(error.exception))
                self.assertNotIn('private-token', str(error.exception))
                if headers.get('X-RateLimit-Reset'):
                    self.assertIn('2023-11-14 22:13:20 UTC', str(error.exception))

    def test_invalid_tokens_and_auth_failures_never_print_the_token(self):
        url = 'https://api.github.com/repos/example/release'
        with patch.dict(os.environ, {'GH_TOKEN': 'private\nheader-injection'}), patch.object(self.module.urllib.request, 'build_opener') as build:
            with self.assertRaisesRegex(ValueError, 'Invalid GH_TOKEN'):
                self.module.download(url, self.root / 'download', 100)
            build.assert_not_called()
        with patch.dict(os.environ, {'GH_TOKEN': 'private-token'}), patch.object(self.module.urllib.request, 'build_opener') as build:
            build.return_value.open.side_effect = self.module.urllib.error.HTTPError(url, 401, 'Unauthorized', {}, None)
            with self.assertRaisesRegex(ValueError, 'GitHub rejected') as error:
                self.module.download(url, self.root / 'download', 100)
            self.assertNotIn('private-token', str(error.exception))


if __name__ == '__main__':
    unittest.main()
