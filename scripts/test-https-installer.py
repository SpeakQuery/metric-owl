#!/usr/bin/env python3
"""Bounded, isolated installer regressions. Never changes the real web server."""
import importlib.util
import json
import os
from pathlib import Path
import pty
import select
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('https_installer', ROOT / 'scripts/configure-https.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class InstallerTests(unittest.TestCase):
    def test_real_terminal_prompts_with_piped_stdin_and_hidden_passwords(self):
        # No mocked prompt/getpass: exercise the same controlling-terminal behavior
        # as curl | sudo bash -> python while stdin is a pipe, not the TTY.
        script = '''
import importlib.util, sys
spec = importlib.util.spec_from_file_location('installer', sys.argv[1])
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)
m.os.environ.pop('GUARD_PUBLIC_IP', None)
m.discover_public_ip = lambda: ''
address = m.choose_public_ip()
terms = m.prompt('Accept the certificate authority terms? (yes/no)', 'no')
credentials = m.administrator_credentials()
assert address == '8.8.8.8' and terms == 'yes'
assert credentials['username'] == 'terminal-admin'
assert credentials['password'] == 'Synthetic-terminal-passphrase-814'
print('TERMINAL_FLOW_OK', flush=True)
'''
        read_end, write_end = os.pipe()
        pid, terminal = pty.fork()
        if pid == 0:
            os.close(write_end)
            os.dup2(read_end, 0)
            os.close(read_end)
            os.execv(sys.executable, [sys.executable, '-c', script, str(ROOT / 'scripts/configure-https.py')])
        os.close(read_end)
        os.close(write_end)
        transcript = bytearray()
        def expect(text):
            deadline = time.monotonic() + 8
            while text.encode() not in transcript:
                remaining = deadline - time.monotonic()
                self.assertGreater(remaining, 0, 'Timed out waiting for terminal prompt: ' + text)
                if not select.select([terminal], [], [], remaining)[0]:
                    continue
                try:
                    chunk = os.read(terminal, 8192)
                except OSError:
                    self.fail('Terminal closed before expected prompt: ' + text)
                self.assertTrue(chunk, 'Terminal ended before expected prompt: ' + text)
                transcript.extend(chunk)
        reaped = False
        try:
            for text, answer in [
                ('Enter this server’s public IPv4 address, then press Enter: ', '8.8.8.8\n'),
                ('Accept the certificate authority terms? (yes/no) [no]: ', 'yes\n'),
                ('Choose your Metric Owl administrator username [admin]: ', 'terminal-admin\n'),
                ('Create a password (at least 14 characters): ', 'Synthetic-terminal-passphrase-814\n'),
                ('Enter the password again: ', 'Synthetic-terminal-passphrase-814\n'),
            ]:
                expect(text)
                os.write(terminal, answer.encode())
            expect('TERMINAL_FLOW_OK')
            _, status = os.waitpid(pid, 0)
            reaped = True
            self.assertEqual(os.waitstatus_to_exitcode(status), 0)
            self.assertNotIn(b'Synthetic-terminal-passphrase-814', transcript)
        finally:
            os.close(terminal)
            if not reaped:
                try:
                    os.kill(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                os.waitpid(pid, 0)

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

    def test_public_ipv4_rejects_private_special_and_injection_inputs(self):
        self.assertEqual(module.public_ip('8.8.8.8'), '8.8.8.8')
        for value in ['127.0.0.1', '10.0.0.1', '192.168.1.1', '100.64.0.1', '169.254.1.1',
                      '0.0.0.0', '224.1.1.1', '255.255.255.255', '::1', '2001:4860:4860::8888',
                      '8.8.8.8:8443', 'https://8.8.8.8', '8.8.8.8;return 200;', '8.8.8.8\n', None]:
            with self.subTest(value=value), self.assertRaises(ValueError):
                module.public_ip(value)

    def test_public_origin_supports_exact_ip_port_and_preserves_old_hostname(self):
        self.assertEqual(module.public_origin('https://8.8.8.8:8443'), 'https://8.8.8.8:8443')
        self.assertEqual(module.public_origin('https://monitor.example.com'), 'https://monitor.example.com')
        for value in ['http://8.8.8.8:8443', 'https://8.8.8.8:8443/', 'https://8.8.8.8:0443',
                      'https://127.0.0.1:8443', 'https://8.8.8.8:65536', 'https://8.8.8.8:4877',
                      'https://user@8.8.8.8:8443', 'https://8.8.8.8:8443?x=1']:
            with self.subTest(value=value), self.assertRaises(ValueError):
                module.public_origin(value)

    def test_port_fallback_is_bounded_and_explicit_port_never_falls_back(self):
        def run(args, **kwargs):
            return subprocess.CompletedProcess(args, 0, 'occupied' if args[-1] != ':10443' else '', '')
        with patch.dict(module.os.environ, {}, clear=True), patch.object(module, 'run', side_effect=run) as runner:
            self.assertEqual(module.choose_panel_port(), 10443)
            self.assertEqual([c.args[0][-1] for c in runner.call_args_list], [':8443', ':9443', ':10443'])
        with patch.dict(module.os.environ, {'GUARD_PANEL_PORT': '8443'}, clear=True), patch.object(module, 'run', side_effect=run) as runner:
            with self.assertRaisesRegex(ValueError, 'already used'):
                module.choose_panel_port()
            self.assertEqual(runner.call_count, 1)

    def test_ip_discovery_uses_interfaces_and_ssh_target_without_external_lookup(self):
        interfaces = [{'addr_info': [{'local': '10.0.0.1'}, {'local': '8.8.8.8'}, {'local': '1.1.1.1'}]}]
        result = subprocess.CompletedProcess([], 0, json.dumps(interfaces), '')
        with patch.dict(module.os.environ, {'SSH_CONNECTION': '10.0.0.2 123 1.1.1.1 22'}, clear=True), patch.object(module, 'run', return_value=result) as runner:
            self.assertEqual(module.discover_public_ip(), '1.1.1.1')
            self.assertEqual(runner.call_args.args[0], ['ip', '-j', '-4', 'addr', 'show', 'scope', 'global'])
        with patch.dict(module.os.environ, {}, clear=True), patch.object(module, 'run', return_value=result):
            self.assertEqual(module.discover_public_ip(), '')

    def test_discovered_public_ip_is_announced_without_confirmation(self):
        with patch.dict(module.os.environ, {}, clear=True), \
                patch.object(module, 'discover_public_ip', return_value='8.8.8.8'), \
                patch.object(module, 'prompt') as prompt, patch('builtins.print') as output:
            self.assertEqual(module.choose_public_ip(), '8.8.8.8')
        prompt.assert_not_called()
        output.assert_called_once_with('Using public IPv4 address: 8.8.8.8', flush=True)

    def test_explicit_public_ip_overrides_discovery_and_invalid_value_never_falls_back(self):
        for value in ['1.1.1.1', '', '127.0.0.1', 'https://8.8.8.8']:
            with self.subTest(value=value), \
                    patch.dict(module.os.environ, {'GUARD_PUBLIC_IP': value}, clear=True), \
                    patch.object(module, 'discover_public_ip') as discover, \
                    patch.object(module, 'prompt') as prompt, patch('builtins.print'):
                if value == '1.1.1.1':
                    self.assertEqual(module.choose_public_ip(), value)
                else:
                    with self.assertRaises(ValueError):
                        module.choose_public_ip()
                discover.assert_not_called()
                prompt.assert_not_called()

    def test_ambiguous_public_ip_asks_for_address_with_enter_instruction(self):
        with patch.dict(module.os.environ, {}, clear=True), \
                patch.object(module, 'discover_public_ip', return_value=''), \
                patch.object(module, 'prompt', return_value='1.1.1.1') as prompt, \
                patch('builtins.print') as output:
            self.assertEqual(module.choose_public_ip(), '1.1.1.1')
        prompt.assert_called_once_with('Enter this server’s public IPv4 address, then press Enter')
        output.assert_called_once_with('Using public IPv4 address: 1.1.1.1', flush=True)

    def test_ip_host_collision_and_custom_regex_are_not_overwritten(self):
        self.assertTrue(module.host_collision('server_name 8.8.8.8;', '8.8.8.8'))
        self.assertFalse(module.host_collision('server_name example.com *.example.com;', '8.8.8.8'))
        with self.assertRaises(ValueError):
            module.host_collision('server_name ~^custom;', '8.8.8.8')

    def test_new_site_follows_existing_implicit_default(self):
        with tempfile.TemporaryDirectory() as directory:
            site = Path(directory) / 'zz-metricowl-panel.conf'
            (site.parent / 'existing.conf').write_text('server {}')
            with patch.object(module, 'SITES_CONF', site):
                self.assertEqual(module.select_conf('include /etc/nginx/sites-enabled/*;'), site)
                (site.parent / 'zzz-custom.conf').write_text('server {}')
                with self.assertRaises(ValueError):
                    module.select_conf('include /etc/nginx/sites-enabled/*;')
            with self.assertRaises(ValueError):
                module.select_conf('include /etc/nginx/conf.d/*.conf; listen 80;')

    def test_cloudpanel_conf_glob_ignores_backups_and_preserves_existing_site_order(self):
        with tempfile.TemporaryDirectory() as directory:
            site = Path(directory) / 'zz-metricowl-panel.conf'
            (site.parent / 'default.conf').write_text('server { listen 80 default_server; }')
            (site.parent / 'zzz-backup.conf.old').write_text('not an included configuration')
            with patch.object(module, 'SITES_CONF', site):
                for quote in ['', '"', "'"]:
                    config = f'http {{\n    include {quote}/etc/nginx/sites-enabled/*.conf{quote}; # CloudPanel sites\n}}'
                    with self.subTest(quote=quote):
                        self.assertEqual(module.select_conf(config), site)
                with self.assertRaisesRegex(ValueError, 'site ordering'):
                    module.select_conf('include /etc/nginx/sites-enabled/*;')
                later = site.parent / 'zzz-live.conf'
                later.write_text('server { listen 80; }')
                with self.assertRaisesRegex(ValueError, 'site ordering'):
                    module.select_conf('include /etc/nginx/sites-enabled/*.conf;')
                self.assertEqual(later.read_text(), 'server { listen 80; }')

    def test_sites_glob_rejects_comment_only_mismatched_quotes_and_other_layouts(self):
        for config in [
            '# include /etc/nginx/sites-enabled/*.conf;',
            'include "/etc/nginx/sites-enabled/*.conf\';',
            'include /etc/nginx/sites-enabled/*.conf.backup;',
            'include /custom/sites-enabled/*.conf;',
            'include /etc/nginx/sites-enabled/nested/*.conf;',
        ]:
            with self.subTest(config=config), self.assertRaisesRegex(ValueError, 'custom include'):
                module.select_conf(config)

    def test_both_sites_globs_check_every_included_file(self):
        with tempfile.TemporaryDirectory() as directory:
            site = Path(directory) / 'zz-metricowl-panel.conf'
            (site.parent / 'zzz-later').write_text('server { listen 80; }')
            with patch.object(module, 'SITES_CONF', site), self.assertRaisesRegex(ValueError, 'site ordering'):
                module.select_conf('include /etc/nginx/sites-enabled/*.conf;\ninclude /etc/nginx/sites-enabled/*;')

    def test_exact_ip_routes_keep_normal_web_ports_and_root_page_separate(self):
        config = module.tls_config('8.8.8.8', 9443)
        self.assertIn('listen 9443 ssl http2;', config)
        self.assertNotIn('listen 443', config)
        self.assertNotIn('default_server;', config)
        self.assertIn('if ($http_host != 8.8.8.8:9443)', config)
        self.assertIn('location = / { return 404; }', config)
        self.assertIn('location = /admin', config)
        self.assertIn('location ^~ /.well-known/acme-challenge/', config)
        self.assertIn('/etc/metric-owl/acme/live/metricowl-ip/fullchain.pem', config)
        self.assertNotIn('/etc/letsencrypt/', config)

    def test_proxy_includes_port_and_clears_untrusted_forwarded_headers(self):
        config = module.proxy_config('8.8.8.8:8443', 'a' * 64)
        self.assertIn('proxy_set_header Host 8.8.8.8:8443;', config)
        self.assertIn('proxy_set_header X-Forwarded-Proto https;', config)
        self.assertIn('proxy_set_header Forwarded "";', config)
        self.assertIn('proxy_buffering off;', config)
        with self.assertRaises(ValueError):
            module.proxy_config('8.8.8.8:8443', 'bad\nheader')

    def test_renewal_is_isolated_and_scheduled_twice_daily(self):
        service = module.renewal_service()
        self.assertIn('/opt/metric-owl/certbot/bin/certbot renew', service)
        self.assertIn('--config-dir /etc/metric-owl/acme', service)
        self.assertIn('--deploy-hook /etc/metric-owl/renew-panel', service)
        self.assertNotIn('/etc/letsencrypt', service)
        self.assertIn('OnCalendar=*-*-* 00,12:00:00', module.renewal_timer())
        self.assertIn('Persistent=true', module.renewal_timer())

    def test_ufw_preserves_existing_rules_and_tracks_only_its_additions(self):
        calls = []
        def run(args, **kwargs):
            calls.append(args)
            stdout = 'Status: active\n80/tcp ALLOW Anywhere\n443/tcp ALLOW Anywhere\nOpenSSH ALLOW Anywhere\n' if args == ['ufw', 'status'] else ''
            return subprocess.CompletedProcess(args, 0, stdout, '')
        with patch.object(module.shutil, 'which', return_value='/usr/sbin/ufw'), patch.object(module, 'run', side_effect=run):
            self.assertEqual(module.open_firewall(9443), [9443])
        self.assertIn(['ufw', 'allow', '9443/tcp', 'comment', module.FIREWALL_TAG], calls)
        self.assertFalse(any('disable' in c or 'reset' in c or 'flush' in c for c in calls))
        self.assertNotIn(['ufw', 'allow', '80/tcp', 'comment', module.FIREWALL_TAG], calls)

    def test_ufw_removal_requires_exact_owned_comment(self):
        output = "ufw allow 8443/tcp comment 'User rule'\nufw allow 9443/tcp comment 'Metric Owl installer'\n"
        with patch.object(module.shutil, 'which', return_value='/usr/sbin/ufw'), patch.object(module, 'run', return_value=subprocess.CompletedProcess([], 0, output, '')) as runner:
            module.close_firewall([8443, 9443])
        calls = [c.args[0] for c in runner.call_args_list]
        self.assertIn(['ufw', '--force', 'delete', 'allow', '9443/tcp'], calls)
        self.assertNotIn(['ufw', '--force', 'delete', 'allow', '8443/tcp'], calls)

    def test_existing_operator_configuration_is_preserved(self):
        with patch.object(module.os, 'geteuid', return_value=0), patch.object(module, 'existing_origin', return_value='https://monitor.example.com'), patch.object(module, 'verify_panel') as verifier, patch.object(module, 'run') as runner, patch.object(module, 'prompt') as prompt:
            module.configure()
        verifier.assert_called_once_with('monitor.example.com')
        runner.assert_not_called()
        prompt.assert_not_called()

    def test_marker_rejects_paths_symlinks_and_unknown_managers(self):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / 'marker'
            for value in ['http://8.8.8.8:8443', 'https://8.8.8.8:8443/admin']:
                marker.write_text(json.dumps({'public_origin': value, 'managed': 'installer'}))
                with self.assertRaises(ValueError):
                    module.existing_origin(marker)
            marker.unlink()
            marker.symlink_to(Path(directory) / 'target')
            (Path(directory) / 'target').write_text('{}')
            with self.assertRaises(ValueError):
                module.existing_origin(marker)

    def test_failed_certificate_rolls_back_only_owned_configuration_and_firewall(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            names = ['MARKER', 'CONF', 'SITES_CONF', 'PROXY', 'TOKEN', 'DROPIN', 'RENEWAL', 'RENEW_SERVICE', 'RENEW_TIMER', 'WEBROOT']
            paths = {name: root / name for name in names}
            existing_site = root / 'existing-site.conf'
            existing_site.write_text('server_name existing.example.com;')
            calls = []
            def run(args, **kwargs):
                calls.append(args)
                if args[0] == str(module.CERTBOT):
                    raise RuntimeError('synthetic failure')
                return subprocess.CompletedProcess(args, 0, '', '')
            with patch.multiple(module, **paths), patch.object(module.os, 'geteuid', return_value=0), patch.dict(module.os.environ, {'GUARD_PUBLIC_IP': '8.8.8.8', 'GUARD_HTTPS_AGREE_TOS': '1'}, clear=True), patch.object(module.shutil, 'which', return_value=None), patch.object(module, 'ensure_prerequisites'), patch.object(module, 'run', side_effect=run), patch.object(module, 'open_firewall', return_value=[8443]), patch.object(module, 'close_firewall') as closer:
                with self.assertRaisesRegex(RuntimeError, 'provider firewall'):
                    module.configure()
            self.assertEqual(existing_site.read_text(), 'server_name existing.example.com;')
            self.assertFalse(paths['SITES_CONF'].exists())
            self.assertFalse(paths['MARKER'].exists())
            closer.assert_called_once_with([8443])
            certificate_command = next(c for c in calls if c[0] == str(module.CERTBOT))
            self.assertIn('--ip-address', certificate_command)
            self.assertIn('shortlived', certificate_command)
            self.assertIn('--keep-until-expiring', certificate_command)
            self.assertNotIn('--staging', certificate_command)

    def test_other_webserver_is_left_running(self):
        with tempfile.TemporaryDirectory() as directory:
            paths = {name: Path(directory) / name for name in ['MARKER', 'CONF', 'SITES_CONF', 'PROXY', 'TOKEN', 'DROPIN', 'RENEWAL', 'RENEW_SERVICE', 'RENEW_TIMER']}
            def run(args, **kwargs):
                return subprocess.CompletedProcess(args, 0, 'LISTEN users:(("apache2",pid=123))' if args[-1] == ':80' else '', '')
            with patch.multiple(module, **paths), patch.object(module.os, 'geteuid', return_value=0), patch.dict(module.os.environ, {'GUARD_PUBLIC_IP': '8.8.8.8'}, clear=True), patch.object(module, 'run', side_effect=run) as runner, patch.object(module, 'ensure_prerequisites') as prerequisites:
                with self.assertRaisesRegex(ValueError, 'Another web server'):
                    module.configure()
            prerequisites.assert_not_called()
            self.assertFalse(any('systemctl' in call.args[0] for call in runner.call_args_list))

    def test_automatic_ip_does_not_automatically_accept_certificate_terms(self):
        with tempfile.TemporaryDirectory() as directory:
            paths = {name: Path(directory) / name for name in ['MARKER', 'CONF', 'SITES_CONF', 'PROXY', 'TOKEN', 'DROPIN', 'RENEWAL', 'RENEW_SERVICE', 'RENEW_TIMER']}
            with patch.multiple(module, **paths), patch.object(module.os, 'geteuid', return_value=0), \
                    patch.dict(module.os.environ, {}, clear=True), \
                    patch.object(module, 'discover_public_ip', return_value='8.8.8.8'), \
                    patch.object(module.shutil, 'which', return_value=None), \
                    patch.object(module, 'run', return_value=subprocess.CompletedProcess([], 0, '', '')), \
                    patch.object(module, 'prompt', return_value='no') as prompt, \
                    patch.object(module, 'ensure_prerequisites') as prerequisites:
                with self.assertRaisesRegex(ValueError, 'terms were not accepted'):
                    module.configure()
            prompt.assert_called_once_with('Accept the certificate authority terms? (yes/no)', 'no')
            prerequisites.assert_not_called()
            self.assertFalse(any(path.exists() for path in paths.values()))

    def test_ip_port_summary_prints_direct_url(self):
        with tempfile.TemporaryDirectory() as directory:
            temp = Path(directory)
            curl = temp / 'curl'
            curl.write_text('#!/bin/sh\nprintf \'%s\\n\' \'{"configured":true,"mfa_required":false,"process_id":12345}\'\n')
            curl.chmod(0o755)
            result = subprocess.run(['bash', str(ROOT / 'scripts/show-setup.sh'), str(temp), 'ssh', '4877', '12345'], capture_output=True, text=True, check=True, env={**os.environ, 'PATH':str(temp)+os.pathsep+os.environ['PATH'], 'GUARD_PUBLIC_ORIGIN':'https://8.8.8.8:9443'})
            self.assertIn('Admin panel: https://8.8.8.8:9443/admin', result.stdout)
            self.assertNotIn('ssh -N', result.stdout)
            self.assertNotIn('Setup key:', result.stdout)

    def test_fresh_ip_setup_records_owned_paths_and_uninstall_preserves_certificate(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            names = ['MARKER', 'CONF', 'SITES_CONF', 'PROXY', 'TOKEN', 'DROPIN', 'RENEWAL', 'RENEW_SERVICE', 'RENEW_TIMER', 'WEBROOT', 'ACME']
            paths = {name: root / name for name in names}
            certificate = paths['ACME'] / 'live/metricowl-ip/fullchain.pem'
            certificate.parent.mkdir(parents=True)
            certificate.write_text('synthetic certificate preserved across reinstall')
            calls = []
            def run(args, **kwargs):
                calls.append(args)
                return subprocess.CompletedProcess(args, 0, '', '')
            with patch.multiple(module, **paths), patch.object(module.os, 'geteuid', return_value=0), patch.dict(module.os.environ, {'GUARD_PUBLIC_IP': '8.8.8.8', 'GUARD_HTTPS_AGREE_TOS': '1'}, clear=True), patch.object(module.shutil, 'which', return_value=None), patch.object(module, 'ensure_prerequisites'), patch.object(module, 'run', side_effect=run), patch.object(module, 'open_firewall', return_value=[8443]), patch.object(module, 'close_firewall') as closer, patch.object(module, 'verify_panel') as verify:
                module.configure()
                data = json.loads(paths['MARKER'].read_text())
                self.assertEqual(data['public_origin'], 'https://8.8.8.8:8443')
                self.assertEqual(data['firewall_ports'], [8443])
                self.assertEqual(paths['TOKEN'].stat().st_mode & 0o777, 0o600)
                self.assertIn('Environment=GUARD_PUBLIC_ORIGIN=https://8.8.8.8:8443', paths['DROPIN'].read_text())
                verify.assert_called_once_with('8.8.8.8:8443')
                self.assertIn(['systemctl', 'start', paths['RENEW_SERVICE'].name], calls)
                module.uninstall_https()
                closer.assert_called_once_with([8443])
            for name in ['MARKER', 'SITES_CONF', 'PROXY', 'TOKEN', 'DROPIN', 'RENEWAL', 'RENEW_SERVICE', 'RENEW_TIMER']:
                self.assertFalse(paths[name].exists())
            self.assertTrue(certificate.exists())

    def test_explicit_ufw_deny_is_not_silently_overridden(self):
        output = 'Status: active\n80/tcp ALLOW Anywhere\n8443/tcp DENY Anywhere\n'
        with patch.object(module.shutil, 'which', return_value='/usr/sbin/ufw'), patch.object(module, 'run', return_value=subprocess.CompletedProcess([], 0, output, '')) as runner:
            with self.assertRaisesRegex(ValueError, 'explicitly blocks'):
                module.open_firewall(8443)
        self.assertFalse(any('allow' in call.args[0] or 'delete' in call.args[0] for call in runner.call_args_list))

    def test_rollback_restart_failure_still_cleans_firewall_and_restores_nginx(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            names = ['MARKER', 'CONF', 'SITES_CONF', 'PROXY', 'TOKEN', 'DROPIN', 'RENEWAL', 'RENEW_SERVICE', 'RENEW_TIMER', 'WEBROOT']
            paths = {name: root / name for name in names}
            calls = []
            def run(args, **kwargs):
                calls.append(args)
                if args == ['systemctl', 'restart', 'server-guard']:
                    raise RuntimeError('synthetic private command failure')
                return subprocess.CompletedProcess(args, 0, '', '')
            with patch.multiple(module, **paths), patch.object(module.os, 'geteuid', return_value=0), patch.dict(module.os.environ, {'GUARD_PUBLIC_IP': '8.8.8.8', 'GUARD_HTTPS_AGREE_TOS': '1'}, clear=True), patch.object(module.shutil, 'which', return_value='/usr/sbin/nginx'), patch.object(module, 'select_conf', return_value=paths['SITES_CONF']), patch.object(module, 'ensure_prerequisites'), patch.object(module, 'run', side_effect=run), patch.object(module, 'open_firewall', return_value=[80, 8443]), patch.object(module, 'close_firewall') as closer:
                with self.assertRaisesRegex(RuntimeError, 'cleanup was incomplete'):
                    module.configure()
                recovery = module.recovery_path()
                data = json.loads(recovery.read_text())
                self.assertEqual(recovery.stat().st_mode & 0o777, 0o600)
            closer.assert_called_once_with([80, 8443])
            self.assertGreaterEqual(calls.count(['systemctl', 'reload', 'nginx']), 2)
            self.assertEqual(data['ownership']['firewall_ports'], [80, 8443])
            self.assertEqual(data['cleanup_failures'][0]['step'], 'restart private application')
            self.assertNotIn('synthetic private command failure', json.dumps(data))
            self.assertFalse(paths['MARKER'].exists())

    def test_uninstall_firewall_failure_keeps_ownership_and_is_retryable(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths = {name: root / name for name in ['MARKER', 'CONF', 'SITES_CONF', 'PROXY', 'TOKEN', 'DROPIN', 'RENEWAL', 'RENEW_SERVICE', 'RENEW_TIMER']}
            for path in paths.values():
                path.write_text('synthetic owned file')
            data = {'public_origin': 'https://8.8.8.8:8443', 'managed': 'installer', 'schema': 2,
                    'nginx_conf': str(paths['SITES_CONF']), 'firewall_ports': [8443], 'port': 8443}
            paths['MARKER'].write_text(json.dumps(data))
            with patch.multiple(module, **paths), patch.object(module.os, 'geteuid', return_value=0), patch.object(module, 'run', return_value=subprocess.CompletedProcess([], 0, '', '')), patch.object(module, 'close_firewall', side_effect=RuntimeError('synthetic firewall failure')):
                with self.assertRaisesRegex(RuntimeError, 'ownership marker was retained'):
                    module.uninstall_https()
                self.assertEqual(json.loads(paths['MARKER'].read_text()), data)
                self.assertEqual(json.loads(module.recovery_path().read_text())['ownership']['firewall_ports'], [8443])
                self.assertFalse(paths['SITES_CONF'].exists())
            with patch.multiple(module, **paths), patch.object(module.os, 'geteuid', return_value=0), patch.object(module, 'run', return_value=subprocess.CompletedProcess([], 0, '', '')), patch.object(module, 'close_firewall') as closer:
                module.uninstall_https()
                closer.assert_called_once_with([8443])
                self.assertFalse(paths['MARKER'].exists())
                self.assertFalse(module.recovery_path().exists())

    def test_uninstall_restore_failure_retains_private_recovery_backup(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths = {name: root / name for name in ['MARKER', 'CONF', 'SITES_CONF', 'PROXY', 'TOKEN', 'DROPIN', 'RENEWAL', 'RENEW_SERVICE', 'RENEW_TIMER']}
            paths['SITES_CONF'].write_text('original owned nginx config')
            data = {'public_origin': 'https://8.8.8.8:8443', 'managed': 'installer', 'schema': 2,
                    'nginx_conf': str(paths['SITES_CONF']), 'firewall_ports': [8443], 'port': 8443}
            paths['MARKER'].write_text(json.dumps(data))
            original_atomic = module.atomic
            def atomic(path, text, mode=0o644):
                if path == paths['SITES_CONF']:
                    raise OSError('synthetic disk failure')
                original_atomic(path, text, mode)
            with patch.multiple(module, **paths), patch.object(module.os, 'geteuid', return_value=0), patch.object(module, 'run', side_effect=RuntimeError('synthetic nginx error')), patch.object(module, 'atomic', side_effect=atomic), patch.object(module, 'close_firewall') as closer:
                with self.assertRaisesRegex(RuntimeError, 'restoration could not be completed'):
                    module.uninstall_https()
                recovery = module.recovery_path()
                self.assertEqual(json.loads(recovery.read_text())['nginx_conf_backup'], 'original owned nginx config')
                self.assertEqual(recovery.stat().st_mode & 0o777, 0o600)
                self.assertTrue(paths['MARKER'].exists())
                closer.assert_not_called()

    def test_firewall_cleanup_failure_does_not_skip_other_owned_ports(self):
        calls = []
        output = "ufw allow 80/tcp comment 'Metric Owl installer'\nufw allow 9443/tcp comment 'Metric Owl installer'\n"
        def run(args, **kwargs):
            calls.append(args)
            if args == ['ufw', '--force', 'delete', 'allow', '9443/tcp']:
                raise RuntimeError('synthetic UFW failure')
            return subprocess.CompletedProcess(args, 0, output, '')
        with patch.object(module.shutil, 'which', return_value='/usr/sbin/ufw'), patch.object(module, 'run', side_effect=run):
            with self.assertRaisesRegex(RuntimeError, '9443'):
                module.close_firewall([80, 9443])
        self.assertIn(['ufw', '--force', 'delete', 'allow', '80/tcp'], calls)


if __name__ == '__main__':
    unittest.main()
