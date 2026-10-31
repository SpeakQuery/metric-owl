#!/usr/bin/env python3
"""Fixed-path Linux ownership ledger. Tests use a disposable root, never real services."""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import sqlite3
import stat
import subprocess
import sys
import uuid

MANIFEST = '/etc/metric-owl/install-manifest.json'
DATA = '/var/lib/server-guard'
DOCS = '/usr/share/doc/server-guard'
UNITS = ('/etc/systemd/system/server-guard.service', '/usr/lib/systemd/system/server-guard.service')
DOC_NAMES = ('LICENSE', 'copyright', 'BINARY-LICENSE.txt', 'THIRD-PARTY-NOTICES.txt', 'release-metadata.json',
             'uninstall-linux.sh', 'manage-installation.py', 'recover-admin.sh',
             'show-setup.sh', 'connection-hints.sh', 'configure-https.py')
BASE = ('/usr/local/bin/server-guard', *UNITS, *(DOCS + '/' + name for name in DOC_NAMES))
HTTPS = ('/etc/metric-owl/panel-access.json', '/etc/metric-owl/panel-proxy-token', '/etc/metric-owl/renew-panel',
         '/etc/nginx/conf.d/metricowl-panel.conf', '/etc/nginx/sites-enabled/zz-metricowl-panel.conf',
         '/etc/nginx/snippets/metricowl-panel-proxy.conf',
         '/etc/systemd/system/server-guard.service.d/metricowl-https.conf',
         '/etc/systemd/system/metricowl-certificate-renew.service', '/etc/systemd/system/metricowl-certificate-renew.timer')
TLS_TREES = ('/etc/metric-owl/acme', '/var/lib/metric-owl-acme', '/var/log/metric-owl-acme',
             '/opt/metric-owl/certbot', '/var/log/metric-owl-installer', '/var/www/metricowl-panel-acme')
LOGS = ('/var/log/nginx/metricowl-panel-access.log', '/var/log/nginx/metricowl-panel-error.log')


def command(args, allowed=(0,)):
    result = subprocess.run(args, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    if result.returncode not in allowed:
        raise ValueError(Path(args[0]).name + ' failed; its private output was not printed.')
    return result


class Installation:
    def __init__(self, root=Path('/'), runner=command):
        self.root, self.runner = root, runner

    def path(self, logical, kind='file'):
        value = PurePosixPath(logical)
        if not value.is_absolute() or '..' in value.parts or str(value) != logical:
            raise ValueError('Unrecognized installation path.')
        path = self.root.joinpath(*value.parts[1:])
        for parent in [path, *path.parents]:
            if parent.is_symlink():
                raise ValueError('Symbolic application path requires review: ' + logical)
            if parent == self.root:
                break
        if path.exists():
            info = path.stat()
            valid = stat.S_ISDIR(info.st_mode) if kind == 'directory' else stat.S_ISREG(info.st_mode) and info.st_nlink == 1
            if not valid:
                raise ValueError('Linked or unexpected application asset type: ' + logical)
        return path

    def digest(self, logical):
        hasher = hashlib.sha256()
        with self.path(logical).open('rb') as source:
            for chunk in iter(lambda: source.read(1024 * 1024), b''):
                hasher.update(chunk)
        return hasher.hexdigest()

    def read(self):
        path = self.path(MANIFEST)
        if not path.exists():
            raise ValueError('Ownership manifest is missing. Reinstall a verified current release or review the installation; no blind purge is available.')
        info = path.stat()
        if info.st_uid != os.geteuid() or info.st_mode & 0o022 or info.st_size > 1024 * 1024:
            raise ValueError('Ownership manifest must be bounded, root-owned and not writable by others.')
        data = json.loads(path.read_text())
        if (data.get('application') != 'metric-owl' or data.get('schema') != 1
                or not isinstance(data.get('files'), dict) or not isinstance(data.get('trees'), list)):
            raise ValueError('Unknown ownership manifest.')
        for logical, digest in data['files'].items():
            if (logical not in (*BASE, *HTTPS)
                    and not logical.startswith(DOCS + '/third-party-licenses/')):
                raise ValueError('Unrecognized manifest file path.')
            self.path(logical)
            if not isinstance(digest, str) or not re.fullmatch('[a-f0-9]{64}', digest):
                raise ValueError('Invalid manifest digest.')
        if any(path not in (DATA, *TLS_TREES) for path in data['trees']):
            raise ValueError('Unrecognized manifest directory.')
        return data

    def write(self, data):
        directory = self.path('/etc/metric-owl', 'directory')
        directory.mkdir(mode=0o700, parents=True, exist_ok=True)
        if directory.stat().st_uid != os.geteuid() or directory.stat().st_mode & 0o022:
            raise ValueError('Ownership directory must belong to root and not be writable by others.')
        path = self.path(MANIFEST)
        temporary = path.with_name('.install-manifest-' + uuid.uuid4().hex)
        try:
            with temporary.open('x') as output:
                os.chmod(temporary, 0o600)
                json.dump(data, output, sort_keys=True, indent=2)
                output.write('\n')
                output.flush()
                os.fsync(output.fileno())
            temporary.replace(path)
        finally:
            temporary.unlink(missing_ok=True)

    def check_data(self):
        directory = self.path(DATA, 'directory')
        if not directory.exists():
            return
        fixed = {'guard.sqlite', 'guard.sqlite-wal', 'guard.sqlite-shm', 'master.key', 'guard.sock',
                 'setup-key', 'connection-hints.json', 'mcp-keys', 'job-reports'}
        for path in directory.iterdir():
            if path.name not in fixed or path.is_symlink():
                raise ValueError('Unrecognized or linked data entry; preserved: ' + path.name)
            if path.name in ('mcp-keys', 'job-reports'):
                self.path(DATA + '/' + path.name, 'directory')
                suffix = '.key' if path.name == 'mcp-keys' else '.json'
                for item in path.iterdir():
                    self.path(DATA + '/' + path.name + '/' + item.name)
                    try:
                        valid = item.name.endswith(suffix) and str(uuid.UUID(item.name[:-len(suffix)])) + suffix == item.name
                    except ValueError:
                        valid = False
                    if not valid:
                        raise ValueError('Unknown data filename requires review.')
            elif path.name == 'guard.sock':
                if not stat.S_ISSOCK(path.lstat().st_mode):
                    raise ValueError('Unexpected socket type.')
            else:
                self.path(DATA + '/' + path.name)
        database = directory / 'guard.sqlite'
        if database.exists():
            # READ_ONLY SQLite can still create WAL/SHM. Validate as the file's
            # owner, including dry runs; never leave root sidecars for the agent.
            program = """import pathlib,sqlite3,sys
p=pathlib.Path(sys.argv[1]);db=sqlite3.connect(p.as_uri()+'?mode=ro',uri=True)
try:
 tables={row[0] for row in db.execute(\"SELECT name FROM sqlite_master WHERE type='table'\")}
 if not {'kv','aggregates','incidents','sessions','mcp_clients','audit'} <= tables: sys.exit(1)
finally: db.close()
"""
            info = database.stat()
            if (directory.stat().st_uid != info.st_uid or directory.stat().st_mode & 0o022
                    or info.st_mode & 0o022):
                raise ValueError('Database/directory ownership or write permissions require review.')
            identity = {'user': info.st_uid, 'group': info.st_gid, 'extra_groups': []} if os.geteuid() == 0 and info.st_uid != 0 else {}
            checked = subprocess.run([sys.executable, '-I', '-c', program, str(database)],
                                     stdout=subprocess.PIPE, stderr=subprocess.PIPE, **identity)
            if checked.returncode != 0:
                raise ValueError('Not a recognized/readable Metric Owl database; data was preserved.')

    def record(self, https=False):
        data = self.read() if self.path(MANIFEST).exists() else {
            'application': 'metric-owl', 'schema': 1, 'installation_id': str(uuid.uuid4()),
            'files': {}, 'trees': [DATA]}
        self.check_data()
        for logical in BASE:
            if self.path(logical).exists():
                data['files'][logical] = self.digest(logical)
        licenses = self.path(DOCS + '/third-party-licenses', 'directory')
        if licenses.exists():
            for path in licenses.rglob('*'):
                if not path.is_dir() or path.is_symlink():
                    logical = '/' + str(path.relative_to(self.root))
                    data['files'][logical] = self.digest(logical)
        if https or self.path(HTTPS[0]).exists():
            marker = json.loads(self.path(HTTPS[0]).read_text())
            if (marker.get('managed') != 'installer' or marker.get('schema') != 2
                    or marker.get('nginx_conf') not in HTTPS[3:5]):
                if https:
                    raise ValueError('Operator-managed or legacy HTTPS cannot be adopted automatically.')
                # App upgrades may preserve operator-owned HTTPS without adopting
                # it. Any subsequent removal still refuses that unknown ownership.
            else:
                for logical in HTTPS:
                    if self.path(logical).exists():
                        data['files'][logical] = self.digest(logical)
                for logical in TLS_TREES:
                    if self.path(logical, 'directory').exists() and logical not in data['trees']:
                        data['trees'].append(logical)
                data['https_managed'] = True
        self.write(data)
        print('Recorded fixed-path Metric Owl ownership manifest.')

    def check_tree(self, logical):
        directory = self.path(logical, 'directory')
        if not directory.exists():
            return
        if logical == DATA:
            self.check_data()
            return
        for path in [directory, *directory.rglob('*')]:
            info = path.lstat()
            if info.st_uid != os.geteuid() or not path.is_symlink() and info.st_mode & 0o022:
                raise ValueError('Unexpected private-asset owner: ' + logical)
            if path.is_symlink():
                destination = path.resolve()
                launcher = (logical == '/opt/metric-owl/certbot' and path.parent == directory / 'bin'
                            and re.fullmatch(r'python(?:3(?:\.\d+)?)?', path.name)
                            and re.fullmatch(r'/usr/bin/python3(?:\.\d+)?', str(destination)))
                if not destination.is_relative_to(directory.resolve()) and not launcher:
                    raise ValueError('Unexpected symbolic link prevents purge: ' + logical)
            elif not (stat.S_ISDIR(info.st_mode) or stat.S_ISREG(info.st_mode) and info.st_nlink == 1):
                raise ValueError('Unexpected linked/special private asset prevents purge.')

    def check_shared_tls(self, data):
        if not data.get('https_managed'):
            return
        owned = {self.path(logical).resolve() for logical in data['files']}
        total = count = 0
        roots = ('/etc/nginx', '/etc/apache2', '/etc/haproxy', '/etc/caddy', '/etc/systemd/system', '/etc/cron.d')
        for logical in roots:
            for path in self.path(logical, 'directory').rglob('*'):
                if path.is_symlink() and (path.is_dir() or not path.exists()):
                    raise ValueError('Uninspected configuration directory link requires operator review.')
                if not path.is_file() or path.resolve() in owned:
                    continue
                count += 1
                total += path.stat().st_size
                if count > 10000 or total > 32 * 1024 * 1024 or path.stat().st_size > 1024 * 1024:
                    raise ValueError('Shared-configuration scan requires operator review.')
                content = path.read_bytes()
                for include in re.finditer(rb'^\s*(?:include|Include|IncludeOptional|import)\s+[\"\']?([^;#\s\"\']+)', content, re.M):
                    target = include.group(1).decode('utf-8', errors='replace')
                    if '$' in target or '..' in PurePosixPath(target).parts:
                        raise ValueError('Dynamic/external configuration include requires operator review.')
                    if target.startswith('/') and not any(target.startswith(root + '/') for root in roots):
                        raise ValueError('External configuration include requires operator review.')
                if any(value.encode() in content for value in (*TLS_TREES, *HTTPS[1:])):
                    raise ValueError('Another configuration references these certificate assets; purge refused.')
        # Check Nginx's fully expanded active configuration too. This covers
        # include/prefix behavior that a directory walk alone cannot establish.
        active = self.runner(['nginx', '-T']).stdout
        headers = list(re.finditer(r'^# configuration file (.+):\s*$', active, re.M))
        if len(active) > 8 * 1024 * 1024 or not headers or len(headers) > 10000:
            raise ValueError('Active Nginx configuration could not be inspected within safe limits.')
        for index, header in enumerate(headers):
            if Path(header.group(1)).resolve() in owned:
                continue
            end = headers[index + 1].start() if index + 1 < len(headers) else len(active)
            section = active[header.end():end]
            if any(value in section for value in (*TLS_TREES, *HTTPS[1:])):
                raise ValueError('Another active Nginx configuration references these certificate assets; purge refused.')

    def plan(self, purge):
        data = self.read()
        if self.path('/etc/metric-owl/https-recovery.json').exists():
            raise ValueError('An unfinished HTTPS operation requires review.')
        for logical, expected in data['files'].items():
            if self.path(logical).exists() and self.digest(logical) != expected:
                raise ValueError('Modified installed file requires review: ' + logical)
        dropins = self.path('/etc/systemd/system/server-guard.service.d', 'directory')
        if dropins.exists() and any('/' + str(p.relative_to(self.root)) not in data['files'] for p in dropins.iterdir()):
            raise ValueError('Unknown service override requires review.')
        if self.path(HTTPS[0]).exists() and not data.get('https_managed'):
            raise ValueError('Unrecognized HTTPS ownership; nothing was removed.')
        self.check_shared_tls(data)
        if purge:
            for logical in TLS_TREES:
                if self.path(logical, 'directory').exists() and logical not in data['trees']:
                    raise ValueError('Unrecorded private TLS assets require review: ' + logical)
            for logical in data['trees']:
                self.check_tree(logical)
            for path in self.path(DOCS, 'directory').rglob('*'):
                if (not path.is_dir() or path.is_symlink()) and '/' + str(path.relative_to(self.root)) not in data['files']:
                    raise ValueError('Unrecognized documentation file requires review.')
        files = [path for path in data['files'] if purge or path in ('/usr/local/bin/server-guard', *UNITS)]
        if purge and data.get('https_managed'):
            for logical in LOGS:
                path = self.path(logical)
                files.append(logical)
                for rotated in path.parent.glob(path.name + '.*'):
                    if re.fullmatch(re.escape(path.name) + r'\.\d+(?:\.gz)?', rotated.name):
                        rotated_name = '/' + str(rotated.relative_to(self.root))
                        self.path(rotated_name)
                        files.append(rotated_name)
        return data, {'mode': 'purge' if purge else 'preserve-data', 'remove_files': sorted(files),
                      'remove_trees': sorted(data['trees']) if purge else [],
                      'preserve': ['Shared OS packages/services', 'Unrelated files/firewall rules', 'Shared system journal']}

    def uninstall(self, purge=False, dry_run=False):
        data, plan = self.plan(purge)
        if dry_run:
            print(json.dumps(plan, indent=2))
            return
        package = self.runner(['dpkg-query', '-S', '/usr/local/bin/server-guard'], allowed=(0, 1))
        if package.returncode == 0:
            raise ValueError('Package-managed installation: remove server-guard with apt first; do not delete package-owned files manually. Keep these helpers/manifest for a reviewed data purge afterwards.')
        if self.path(HTTPS[0]).exists():
            self.runner(['python3', str(self.path(DOCS + '/configure-https.py')), '--uninstall'])
        self.runner(['systemctl', 'disable', '--now', 'server-guard'], allowed=(0, 1, 5))
        self.runner(['systemctl', 'is-active', '--quiet', 'server-guard'], allowed=(3, 4))
        if purge:
            if not shutil.rmtree.avoids_symlink_attacks:
                raise ValueError('This Python cannot safely remove owned directories; data retained.')
            for logical in data['trees']:
                self.check_tree(logical)
            for logical in data['trees']:
                path = self.path(logical, 'directory')
                if path.exists():
                    shutil.rmtree(path)
        for logical in plan['remove_files']:
            self.path(logical).unlink(missing_ok=True)
        self.runner(['systemctl', 'daemon-reload'])
        self.runner(['systemctl', 'reset-failed', 'server-guard'], allowed=(0, 1, 5))
        if purge:
            self.path(MANIFEST).unlink()
            docs = self.path(DOCS, 'directory')
            candidates = sorted((p for p in docs.rglob('*') if p.is_dir()), key=lambda p: len(p.parts), reverse=True) if docs.exists() else []
            candidates.extend(self.path(p, 'directory') for p in (DOCS, '/etc/systemd/system/server-guard.service.d', '/opt/metric-owl', '/etc/metric-owl'))
            for path in candidates:
                if path.exists() and not any(path.iterdir()):
                    path.rmdir()
            print('Purged verified Metric Owl account/database, integration keys and owned application/TLS assets.')
        else:
            print('Application removed. Account/history/settings, certificates, manifest and helpers are preserved.')
        print('Shared OS dependencies/services, unrelated firewall rules/files and the shared journal were preserved.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('record', 'record-https', 'inspect', 'uninstall'))
    parser.add_argument('--purge', action='store_true')
    parser.add_argument('--dry-run', action='store_true')
    args = parser.parse_args()
    if os.geteuid() != 0 or sys.platform != 'linux':
        raise ValueError('Installation management requires root privileges on Linux.')
    installation = Installation()
    if args.operation in ('record', 'record-https'):
        if args.purge or args.dry_run:
            raise ValueError('Removal flags do not apply to ownership recording.')
        installation.record(args.operation == 'record-https')
    else:
        installation.uninstall(args.purge, args.dry_run or args.operation == 'inspect')


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, sqlite3.Error) as error:
        print('Metric Owl ownership/removal check stopped: ' + str(error), file=sys.stderr)
        raise SystemExit(1) from None
