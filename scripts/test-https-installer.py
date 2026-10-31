#!/usr/bin/env python3
"""Bounded, isolated installer regressions. Never changes the real web server."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('https_installer', ROOT / 'scripts/configure-https.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class InstallerTests(unittest.TestCase):
    def test_admin_prepare_skips_existing_or_ready_account(self):
        for status in [{'configured': True}, {'configured': False, 'enrollment_ready': True}]:
            with self.subTest(status=status), patch.object(module.os, 'geteuid', return_value=0), \
                    patch.object(module, 'local_status', return_value=status), \
                    patch.object(module, 'administrator_credentials') as credentials, \
                    patch.object(module, 'run') as runner:
                module.prepare_administrator()
                credentials.assert_not_called()
                runner.assert_not_called()

    def test_admin_prepare_uses_only_stdin_and_restarts_service(self):
        synthetic = 'synthetic-test-password-123'
        with patch.object(module.os, 'geteuid', return_value=0), \
                patch.object(module, 'local_status', side_effect=[{'configured': False}, {'configured': True, 'mfa_required': False}]), \
                patch.object(module, 'administrator_credentials', return_value={'username': 'test-admin', 'password': synthetic}), \
                patch.object(module, 'run') as runner, \
                patch.object(module.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, b'{"configured":true}', b'')) as process:
            module.prepare_administrator()
        args, kwargs = process.call_args
        self.assertNotIn(synthetic, str(args))
        self.assertNotIn('env', kwargs)
        self.assertEqual(json.loads(kwargs['input']), {'username': 'test-admin', 'password': synthetic})
        self.assertEqual(args[0][:4], ['runuser', '-u', 'server-guard', '--'])
        self.assertEqual(runner.call_args_list[0].args[0], ['systemctl', 'stop', 'server-guard'])
        self.assertEqual(runner.call_args_list[-1].args[0], ['systemctl', 'start', 'server-guard'])

    def test_failed_admin_preparation_always_restarts_without_exposing_output(self):
        secret = 'synthetic-secret-must-not-print'
        with patch.object(module.os, 'geteuid', return_value=0), \
                patch.object(module, 'local_status', return_value={'configured': False}), \
                patch.object(module, 'administrator_credentials', return_value={'username': 'admin', 'password': secret}), \
                patch.object(module, 'run') as runner, \
                patch.object(module.subprocess, 'run', return_value=subprocess.CompletedProcess([], 1, secret.encode(), secret.encode())):
            with self.assertRaises(RuntimeError) as failure:
                module.prepare_administrator()
        self.assertNotIn(secret, str(failure.exception))
        self.assertEqual(runner.call_args_list[-1].args[0], ['systemctl', 'start', 'server-guard'])

    def test_prepared_https_summary_has_no_setup_key_or_tunnel_instructions(self):
        with tempfile.TemporaryDirectory() as directory:
            temp = Path(directory)
            curl = temp / 'curl'
            curl.write_text('#!/bin/sh\nprintf \'%s\\n\' \'{"configured":false,"enrollment_ready":true,"process_id":12345}\'\n')
            curl.chmod(0o755)
            result = subprocess.run(['bash', str(ROOT / 'scripts/show-setup.sh'), str(temp), 'ssh', '4877', '12345'],
                                    capture_output=True, text=True, check=True,
                                    env={**os.environ, 'PATH':str(temp)+os.pathsep+os.environ['PATH'],
                                         'GUARD_PUBLIC_ORIGIN':'https://monitor.example.com'})
            self.assertIn('Admin panel: https://monitor.example.com/admin', result.stdout)
            self.assertIn('No setup key is needed.', result.stdout)
            self.assertIn('Scan the QR code', result.stdout)
            self.assertNotIn('Setup key:', result.stdout)
            self.assertNotIn('ssh -N', result.stdout)
            self.assertNotIn('MCP', result.stdout)

    def test_configured_summary_respects_optional_and_existing_mfa(self):
        for required in [False, True]:
            with self.subTest(required=required), tempfile.TemporaryDirectory() as directory:
                temp = Path(directory)
                curl = temp / 'curl'
                status = json.dumps({'configured': True, 'enrollment_ready': False,
                                     'mfa_required': required, 'process_id': 12345})
                curl.write_text('#!/bin/sh\nprintf \'%s\\n\' \'' + status + '\'\n')
                curl.chmod(0o755)
                result = subprocess.run(['bash', str(ROOT / 'scripts/show-setup.sh'), str(temp), 'ssh', '4877', '12345'],
                                        capture_output=True, text=True, check=True,
                                        env={**os.environ, 'PATH':str(temp)+os.pathsep+os.environ['PATH'],
                                             'GUARD_PUBLIC_ORIGIN':'https://monitor.example.com'})
                self.assertIn('Sign in with your username and password.', result.stdout)
                self.assertNotIn('Setup key:', result.stdout)
                self.assertNotIn('Scan the QR code', result.stdout)
                self.assertNotIn('ssh -N', result.stdout)
                if required:
                    self.assertIn('existing two-factor protection remains enabled', result.stdout)
                else:
                    self.assertIn('optionally enable two-factor authentication', result.stdout)
                    self.assertNotIn('Enter your authenticator code', result.stdout)

    def test_hostname_input_cannot_inject_shell_or_nginx(self):
        self.assertEqual(module.hostname('Monitor.Example.com'), 'monitor.example.com')
        for value in ['127.0.0.1', 'https://monitor.example.com', 'x; return 200;#', 'a\nb.com',
                      'a..com', '-a.example.com', 'a.example.com:443', '*.example.com',
                      'x.localhost', 'x.test', '$hostname.com', 'a.example.com/']:
            with self.subTest(value=value), self.assertRaises(ValueError):
                module.hostname(value)

    def test_existing_site_conflicts_include_wildcards(self):
        for declaration in ['monitor.example.com', '*.example.com', '.example.com', 'monitor.*']:
            self.assertTrue(module.host_collision('server_name ' + declaration + ';', 'monitor.example.com'))
        self.assertFalse(module.host_collision('server_name example.com www.example.com;', 'monitor.example.com'))
        with self.assertRaises(ValueError):
            module.host_collision('server_name ~^monitor.*;', 'monitor.example.com')

    def test_other_web_server_is_rejected_even_when_nginx_is_installed(self):
        with tempfile.TemporaryDirectory() as directory:
            paths = {name: Path(directory) / name for name in ['MARKER', 'CONF', 'PROXY', 'TOKEN', 'DROPIN', 'RENEWAL']}
            with patch.multiple(module, **paths), patch.object(module, 'existing_origin', return_value=None), \
                    patch.object(module, 'Path', side_effect=lambda value: Path(directory) / 'certificates' if value == '/etc/letsencrypt/live' else Path(value)), \
                    patch.object(module.os, 'geteuid', return_value=0), \
                    patch.dict(module.os.environ, {'GUARD_HTTPS_DOMAIN': 'monitor.example.com'}), \
                    patch.object(module.socket, 'getaddrinfo', return_value=[(0, 0, 0, '', ('8.8.8.8', 443))]), \
                    patch.object(module.shutil, 'which', return_value='/usr/sbin/nginx'), \
                    patch.object(module, 'run', return_value=subprocess.CompletedProcess([], 0, 'LISTEN 0 511 *:80 *:* users:(("apache2",pid=123,fd=4))\n', '')) as runner, \
                    patch.object(module, 'atomic') as writer:
                with self.assertRaisesRegex(ValueError, 'Another web server'):
                    module.configure()
            self.assertEqual(runner.call_count, 1)
            writer.assert_not_called()

    def test_existing_operator_configuration_is_not_modified(self):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / 'panel-access.json'
            marker.write_text(json.dumps({'public_origin': 'https://monitor.example.com', 'managed': 'operator'}))
            original = marker.read_bytes()
            with patch.object(module.os, 'geteuid', return_value=0), \
                    patch.object(module, 'existing_origin', return_value=module.existing_origin(marker)), \
                    patch.object(module, 'verify_panel') as verifier, \
                    patch.object(module, 'run') as runner, patch.object(module, 'prompt') as prompt:
                module.configure()
            runner.assert_not_called()
            prompt.assert_not_called()
            verifier.assert_called_once_with('monitor.example.com')
            self.assertEqual(marker.read_bytes(), original)

    def test_marker_rejects_paths_and_symlinks(self):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / 'marker'
            for value in ['http://example.com', 'https://example.com/admin', 'https://evil.com:443',
                          'https://example.com\nX-Header: something']:
                marker.write_text(json.dumps({'public_origin': value, 'managed': 'operator'}))
                with self.assertRaises(ValueError):
                    module.existing_origin(marker)
            marker.unlink()
            marker.symlink_to(Path(directory) / 'target')
            (Path(directory) / 'target').write_text('{}')
            with self.assertRaises(ValueError):
                module.existing_origin(marker)

    def test_proxy_uses_fixed_authority_and_strips_untrusted_forwarding(self):
        text = module.proxy_config('monitor.example.com', 'a' * 64)
        self.assertIn('proxy_set_header Host monitor.example.com;', text)
        self.assertIn('proxy_set_header X-Forwarded-Proto https;', text)
        self.assertIn('proxy_set_header Forwarded "";', text)
        self.assertIn('proxy_set_header X-Forwarded-For $remote_addr;', text)
        self.assertNotIn('$http_x_forwarded', text)
        self.assertIn('proxy_buffering off;', text)
        with self.assertRaises(ValueError):
            module.proxy_config('monitor.example.com', 'bad\nheader')

    def test_tls_routes_are_bounded_and_auth_is_rate_limited(self):
        text = module.tls_config('monitor.example.com')
        self.assertIn('ssl_protocols TLSv1.2 TLSv1.3;', text)
        self.assertIn('location ^~ /admin/', text)
        self.assertIn('location /api/v1/', text)
        self.assertIn('limit_req zone=metricowl_panel_login burst=5 nodelay;', text)
        self.assertIn('location / { return 404; }', text)
        self.assertIn('client_max_body_size 64k;', text)

    def test_atomic_private_credential_permissions(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'private/token'
            module.atomic(path, 'synthetic-test-value', 0o600)
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            self.assertEqual(path.read_text(), 'synthetic-test-value')
            self.assertEqual(len(list(path.parent.iterdir())), 1)

    def test_failure_rolls_back_only_new_files_and_preserves_existing_site(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            owned = {name: root / name for name in ['MARKER', 'CONF', 'PROXY', 'TOKEN', 'DROPIN', 'RENEWAL', 'WEBROOT']}
            existing = root / 'existing-site.conf'
            existing.write_text('server_name existing.example.com;')
            commands = []

            def run(args, **kwargs):
                commands.append(args)
                if args[:2] in [['systemctl', 'is-active'], ['systemctl', 'is-enabled']]:
                    return subprocess.CompletedProcess(args, 3, '', '')
                if args[:2] == ['nginx', '-T']:
                    return subprocess.CompletedProcess(args, 0, 'include /etc/nginx/conf.d/*.conf;\nserver_name existing.example.com;', '')
                return subprocess.CompletedProcess(args, 0, '', '')

            with patch.multiple(module, **owned), patch.object(module, 'existing_origin', return_value=None), \
                    patch.object(module, 'Path', side_effect=lambda value: root / 'certificates' if value == '/etc/letsencrypt/live' else Path(value)), \
                    patch.object(module.os, 'geteuid', return_value=0), \
                    patch.dict(module.os.environ, {'GUARD_HTTPS_DOMAIN': 'monitor.example.com', 'GUARD_HTTPS_EMAIL': '', 'GUARD_HTTPS_AGREE_TOS': '1'}), \
                    patch.object(module.socket, 'getaddrinfo', return_value=[(0, 0, 0, '', ('8.8.8.8', 443))]), \
                    patch.object(module.shutil, 'which', return_value='/test/bin'), patch.object(module, 'run', side_effect=run), \
                    patch.object(module, 'verify_panel', side_effect=RuntimeError('synthetic TLS failure')):
                with self.assertRaisesRegex(RuntimeError, 'synthetic TLS failure'):
                    module.configure()
            for name in ['MARKER', 'CONF', 'PROXY', 'TOKEN', 'DROPIN', 'RENEWAL']:
                self.assertFalse(owned[name].exists(), name)
            self.assertEqual(owned['WEBROOT'].stat().st_mode & 0o777, 0o755)
            self.assertEqual((owned['WEBROOT'] / '.well-known/acme-challenge').stat().st_mode & 0o777, 0o755)
            self.assertEqual(existing.read_text(), 'server_name existing.example.com;')
            self.assertEqual(commands.count(['systemctl', 'restart', 'server-guard']), 2)
            self.assertIn(['systemctl', 'stop', 'nginx'], commands)
            self.assertIn(['systemctl', 'disable', 'nginx'], commands)


if __name__ == '__main__':
    unittest.main()
