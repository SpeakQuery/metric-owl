#!/usr/bin/env python3
"""Linux root regression using only disposable service-owned SQLite fixtures.

Does not stop/start a service, read the installed database, or reset an account.
Run after installing the test binary/service account on a fresh CI machine.
"""
import json
import importlib.util
import os
from pathlib import Path
import pwd
import subprocess
import sys
import tempfile
import pty
import select
import time


def main():
    if os.geteuid() != 0 or sys.platform != 'linux':
        raise SystemExit('This isolated ownership regression needs Linux root and the server-guard service account.')
    account = pwd.getpwnam('server-guard')
    assert account.pw_uid != 0
    binary = sys.argv[1] if len(sys.argv) == 2 else '/usr/local/bin/server-guard'
    assert Path(binary).is_absolute()
    with tempfile.TemporaryDirectory(prefix='metric-owl-recovery-fixture-') as temporary:
        directory = Path(temporary)
        os.chown(directory, account.pw_uid, account.pw_gid)
        setup = '''
import json,pathlib,sqlite3,sys
p=pathlib.Path(sys.argv[1]);(p/'master.key').write_bytes(b'x'*32)
db=sqlite3.connect(p/'guard.sqlite');db.execute('PRAGMA journal_mode=WAL')
db.execute('CREATE TABLE kv(key TEXT PRIMARY KEY,value TEXT NOT NULL)')
for name in ('aggregates','incidents','sessions','mcp_clients','audit'): db.execute('CREATE TABLE '+name+'(value TEXT)')
db.execute("INSERT INTO kv VALUES('admin',?)",(json.dumps({'username':'fixture-owner','password':'unused-hash','secret':'','last_step':0,'recovery':[],'mfa_enabled':False}),))
db.commit();db.execute('PRAGMA wal_checkpoint(TRUNCATE)');db.close()
'''
        subprocess.run(['runuser', '-u', 'server-guard', '--', sys.executable, '-c', setup, temporary], check=True)
        assert not (directory / 'guard.sqlite-wal').exists()
        assert not (directory / 'guard.sqlite-shm').exists()

        def check(expected):
            result = subprocess.run([binary, '--data-dir', temporary, 'recover-admin', '--show-username'],
                                    text=True, capture_output=True, check=True)
            assert result.stdout == 'Administrator username: ' + expected + '\n'
            for suffix in ('', '-wal', '-shm'):
                path = directory / ('guard.sqlite' + suffix)
                if path.exists():
                    assert path.stat().st_uid == account.pw_uid, 'Root-created SQLite sidecar'
            subprocess.run(['runuser', '-u', 'server-guard', '--', sys.executable, '-c',
                            "import sqlite3,sys;db=sqlite3.connect(sys.argv[1]);db.execute('SELECT * FROM kv').fetchall();db.close()",
                            str(directory / 'guard.sqlite')], check=True)

        check('fixture-owner')
        # The schema inspection used by dry-run/ownership recording has the
        # same SQLite-sidecar rule, but keeps its parent process root.
        spec = importlib.util.spec_from_file_location('maintenance', Path(__file__).with_name('manage-installation.py'))
        maintenance = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(maintenance)
        fixture_root = directory / 'filesystem'
        data_directory = fixture_root / 'var/lib/server-guard'
        data_directory.mkdir(parents=True)
        os.chown(data_directory, account.pw_uid, account.pw_gid)
        subprocess.run(['runuser', '-u', 'server-guard', '--', sys.executable, '-c', setup, str(data_directory)], check=True)
        maintenance.Installation(fixture_root).check_data()
        for path in data_directory.glob('guard.sqlite*'):
            assert path.stat().st_uid == account.pw_uid, 'Root schema check created a root-owned sidecar'
        # Keep a genuine live WAL with a newer account value. Immutable reads
        # would miss it, so this also prevents that tempting workaround.
        live = '''
import json,sqlite3,sys
db=sqlite3.connect(sys.argv[1]);db.execute('PRAGMA journal_mode=WAL')
raw=json.loads(db.execute("SELECT value FROM kv WHERE key='admin'").fetchone()[0]);raw['username']='fixture-newer'
db.execute("UPDATE kv SET value=? WHERE key='admin'",(json.dumps(raw),));db.commit()
print('ready',flush=True);sys.stdin.readline();db.close()
'''
        writer = subprocess.Popen(['runuser', '-u', 'server-guard', '--', sys.executable, '-c', live,
                                   str(directory / 'guard.sqlite')], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        try:
            assert writer.stdout.readline().strip() == 'ready'
            check('fixture-newer')
        finally:
            writer.communicate('\n', timeout=10)
        assert writer.returncode == 0
        # A real controlling PTY reaches the prompt only after the socket was
        # created as the service user. Ctrl-C cancels without a password reset.
        pid, terminal = pty.fork()
        if pid == 0:
            os.execv(binary, [binary, '--data-dir', temporary, 'recover-admin'])
        transcript = bytearray()
        reaped = False
        try:
            deadline = time.monotonic() + 10
            while b'New password' not in transcript:
                assert time.monotonic() < deadline, 'Recovery terminal did not become ready'
                if select.select([terminal], [], [], 0.2)[0]:
                    transcript.extend(os.read(terminal, 8192))
            assert (directory / 'guard.sock').stat().st_uid == account.pw_uid
            os.write(terminal, b'\x03')
            deadline = time.monotonic() + 10
            while True:
                finished, status = os.waitpid(pid, os.WNOHANG)
                if finished:
                    reaped = True
                    break
                assert time.monotonic() < deadline, 'Cancelled recovery did not exit'
                time.sleep(0.05)
            assert os.waitstatus_to_exitcode(status) != 0
        finally:
            os.close(terminal)
            if not reaped:
                os.kill(pid, 9)
                os.waitpid(pid, 0)
        check('fixture-newer')
    print(json.dumps({'recovery_owner_regression': 'passed', 'fixtures_only': True,
                      'checks': ['No root-created WAL/SHM', 'Service owner can reopen', 'Live WAL username is current']}))


if __name__ == '__main__':
    main()
