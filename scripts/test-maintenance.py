#!/usr/bin/env python3
"""Maintenance safety fixtures: every path/service is disposable or faked."""
from contextlib import redirect_stdout
import importlib.util
import io
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('maintenance', ROOT / 'scripts/manage-installation.py')
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class MaintenanceTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.commands = []
        self.packaged = False
        self.installation = m.Installation(self.root, self.run_command)
        self.write('/usr/local/bin/server-guard', 'owned binary')
        self.write(m.UNITS[0], 'owned service')
        for name in m.DOC_NAMES:
            self.write(m.DOCS + '/' + name, 'owned helper or notice: ' + name)
        self.write(m.DATA + '/master.key', 'private-vault-key')
        database = self.root / m.DATA.lstrip('/') / 'guard.sqlite'
        with sqlite3.connect(database) as db:
            for table in ('kv', 'aggregates', 'incidents', 'sessions', 'mcp_clients', 'audit'):
                db.execute('CREATE TABLE ' + table + '(value TEXT)')
            db.execute("INSERT INTO kv VALUES('private account and integration data')")
        self.write('/srv/unrelated/important.txt', 'keep unrelated')
        with redirect_stdout(io.StringIO()):
            self.installation.record()

    def write(self, logical, content):
        path = self.root / logical.lstrip('/')
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        return path

    def run_command(self, args, allowed=(0,)):
        self.commands.append(args)
        if args[:2] == ['dpkg-query', '-S']:
            return subprocess.CompletedProcess(args, 0 if self.packaged else 1, '', '')
        if args[:3] == ['systemctl', 'is-active', '--quiet']:
            return subprocess.CompletedProcess(args, 3, '', '')
        if args == ['nginx', '-T']:
            return subprocess.CompletedProcess(args, 0, '# configuration file /etc/nginx/nginx.conf:\nhttp {}\n', '')
        if args[:1] == ['python3']:
            self.assertEqual(args[-1], '--uninstall')
            for logical in m.HTTPS:
                path = self.root / logical.lstrip('/')
                path.unlink(missing_ok=True)
        return subprocess.CompletedProcess(args, 0, '', '')

    def add_https(self):
        for logical in m.HTTPS[1:]:
            self.write(logical, 'owned HTTPS asset')
        self.write(m.HTTPS[0], json.dumps({'managed': 'installer', 'schema': 2, 'nginx_conf': m.HTTPS[4],
                                         'public_origin': 'https://8.8.8.8:8443', 'port': 8443, 'firewall_ports': [8443]}))
        for logical in m.TLS_TREES:
            self.write(logical + '/owned-file', 'private certificate or installer log')
        self.write(m.LOGS[0], 'private panel log')
        self.write(m.LOGS[0] + '.1', 'private rotated panel log')
        with redirect_stdout(io.StringIO()):
            self.installation.record(https=True)

    def test_default_retains_database_certificates_and_helpers_then_explicit_purge_removes_owned_only(self):
        self.add_https()
        with redirect_stdout(io.StringIO()):
            self.installation.uninstall()
        self.assertFalse(self.installation.path('/usr/local/bin/server-guard').exists())
        self.assertTrue(self.installation.path(m.DATA + '/guard.sqlite').exists())
        self.assertTrue(self.installation.path(m.TLS_TREES[0], 'directory').exists())
        self.assertTrue(self.installation.path(m.DOCS + '/uninstall-linux.sh').exists())
        with redirect_stdout(io.StringIO()):
            self.installation.uninstall(purge=True)
        self.assertFalse(self.installation.path(m.DATA, 'directory').exists())
        self.assertFalse(self.installation.path(m.MANIFEST).exists())
        self.assertFalse(self.installation.path(m.DOCS, 'directory').exists())
        self.assertFalse(self.installation.path(m.TLS_TREES[0], 'directory').exists())
        self.assertFalse(self.installation.path(m.LOGS[0]).exists())
        self.assertEqual((self.root / 'srv/unrelated/important.txt').read_text(), 'keep unrelated')
        self.assertFalse(any(args[0] in ('apt', 'apt-get', 'userdel', 'journalctl') for args in self.commands))

    def test_dry_run_json_has_no_service_or_file_mutations(self):
        output = io.StringIO()
        with redirect_stdout(output):
            self.installation.uninstall(purge=True, dry_run=True)
        plan = json.loads(output.getvalue())
        self.assertEqual(plan['mode'], 'purge')
        self.assertIn(m.DATA, plan['remove_trees'])
        self.assertIn('/usr/local/bin/server-guard', plan['remove_files'])
        self.assertEqual(self.commands, [])
        self.assertTrue(self.installation.path(m.DATA + '/guard.sqlite').exists())

    def test_unknown_data_file_and_modified_unit_refuse_before_commands(self):
        extra = self.write(m.DATA + '/my-backup.tar', 'never remove')
        with self.assertRaisesRegex(ValueError, 'Unrecognized'):
            self.installation.uninstall(purge=True)
        self.assertEqual(extra.read_text(), 'never remove')
        extra.unlink()
        self.write(m.UNITS[0], 'operator changed service')
        with self.assertRaisesRegex(ValueError, 'Modified'):
            self.installation.uninstall(purge=True)
        self.assertEqual(self.commands, [])

    def test_linked_data_or_owned_path_and_hostile_manifest_refuse(self):
        target = self.installation.path('/usr/local/bin/server-guard')
        target.unlink()
        target.symlink_to(self.root / 'srv/unrelated/important.txt')
        with self.assertRaisesRegex(ValueError, 'Symbolic'):
            self.installation.uninstall(purge=True)
        target.unlink()
        self.write('/usr/local/bin/server-guard', 'owned binary')
        data = self.installation.read()
        data['trees'].append('/srv/unrelated')
        self.installation.write(data)
        with self.assertRaisesRegex(ValueError, 'Unrecognized'):
            self.installation.uninstall(purge=True)
        self.assertEqual(self.commands, [])

    def test_shared_certificate_and_unrecorded_tls_refuse_before_commands(self):
        self.add_https()
        self.write('/etc/nginx/conf.d/customer.conf', 'ssl_certificate /etc/metric-owl/acme/live/metricowl-ip/fullchain.pem;')
        with self.assertRaisesRegex(ValueError, 'Another configuration'):
            self.installation.uninstall(purge=True)
        self.assertEqual(self.commands, [])

    def test_shared_certificate_directory_link_or_external_include_fail_closed(self):
        self.add_https()
        external = self.write('/srv/customer-vhosts/customer.conf', 'ssl_certificate /etc/metric-owl/acme/live/metricowl-ip/fullchain.pem;')
        self.write('/etc/nginx/nginx.conf', 'include /etc/nginx/customer-vhosts/*.conf;')
        link = self.root / 'etc/nginx/customer-vhosts'
        link.symlink_to(external.parent, target_is_directory=True)
        with self.assertRaisesRegex(ValueError, 'directory link'):
            self.installation.uninstall(purge=True)
        link.unlink()
        self.write('/etc/nginx/nginx.conf', 'include /srv/customer-vhosts/*.conf;')
        with self.assertRaisesRegex(ValueError, 'External configuration include'):
            self.installation.uninstall(purge=True)
        self.assertEqual(self.commands, [])

    def test_missing_manifest_and_package_manager_ownership_fail_closed(self):
        self.packaged = True
        with self.assertRaisesRegex(ValueError, 'Package-managed'):
            self.installation.uninstall(purge=True)
        self.assertEqual(len(self.commands), 1)
        self.installation.path(m.MANIFEST).unlink()
        self.commands.clear()
        with self.assertRaisesRegex(ValueError, 'manifest is missing'):
            self.installation.uninstall(purge=True)
        self.assertEqual(self.commands, [])

    def test_root_recovery_wrapper_restores_only_previously_active_service_on_cancel(self):
        fakebin = self.root / 'fakebin'
        fakebin.mkdir()
        log = self.root / 'wrapper-commands.log'
        for name, text in {
            'id': '#!/bin/bash\necho 0\n',
            'uname': '#!/bin/bash\necho Linux\n',
            'systemctl': '#!/bin/bash\nprintf "%s\\n" "$*" >> "$MAINTENANCE_TEST_LOG"\nif [[ "$1" == is-active ]]; then exit "$MAINTENANCE_TEST_ACTIVE"; fi\n',
        }.items():
            script = fakebin / name
            script.write_text(text)
            script.chmod(0o755)
        binary = self.installation.path('/usr/local/bin/server-guard')
        binary.write_text('#!/bin/bash\nexit 130\n')
        binary.chmod(0o755)
        wrapper = self.root / 'recover-fixture.sh'
        script = (ROOT / 'scripts/recover-admin.sh').read_text()
        for path in ('/usr/local/bin/server-guard', '/var/lib/server-guard'):
            script = script.replace(path, str(self.root / path.lstrip('/')))
        wrapper.write_text(script)
        for active in (0, 3):
            log.unlink(missing_ok=True)
            result = subprocess.run(['bash', str(wrapper)], capture_output=True, text=True, env={
                **os.environ, 'PATH': str(fakebin) + os.pathsep + os.environ['PATH'],
                'MAINTENANCE_TEST_LOG': str(log), 'MAINTENANCE_TEST_ACTIVE': str(active)})
            self.assertEqual(result.returncode, 130)
            commands = log.read_text().splitlines()
            self.assertIn('stop server-guard', commands)
            self.assertEqual('start server-guard' in commands, active == 0)

    def test_internal_certificate_links_are_safe_but_escape_links_and_shared_unit_aliases_are_distinguished(self):
        self.add_https()
        certificate = self.write(m.TLS_TREES[0] + '/archive/cert1.pem', 'private certificate')
        live = self.root / m.TLS_TREES[0].lstrip('/') / 'live'
        live.mkdir()
        (live / 'cert.pem').symlink_to('../archive/cert1.pem')
        unit_alias = self.root / 'etc/systemd/system/timers.target.wants/metricowl-certificate-renew.timer'
        unit_alias.parent.mkdir(parents=True)
        unit_alias.symlink_to(self.installation.path('/etc/systemd/system/metricowl-certificate-renew.timer'))
        self.installation.plan(purge=True)
        (live / 'cert.pem').unlink()
        (live / 'cert.pem').symlink_to(self.root / 'srv/unrelated/important.txt')
        with self.assertRaisesRegex(ValueError, 'symbolic link'):
            self.installation.uninstall(purge=True)
        self.assertTrue(all(args == ['nginx', '-T'] for args in self.commands))
        self.assertTrue(certificate.exists())


if __name__ == '__main__':
    unittest.main()
