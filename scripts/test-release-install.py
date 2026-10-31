#!/usr/bin/env python3
"""Actual published-package lifecycle on a FRESH DISPOSABLE GitHub Ubuntu VM.

Installs a service, resets its synthetic account, and purges its database.
Never run on a customer/operator server. The guards reject an existing install.
No real credentials, external AI requests, SMTP messages or public TLS issuance.
"""
import http.cookiejar
import json
import os
from pathlib import Path
import re
import secrets
import subprocess
import time
import urllib.error
import urllib.request

import pexpect

assert os.environ.get('GITHUB_ACTIONS') == 'true', 'Disposable GitHub runner only'
assert os.environ.get('RUNNER_OS') == 'Linux', 'Linux runner only'
assert os.geteuid() != 0, 'Invoke as the runner account with passwordless sudo'
for path in ['/usr/local/bin/server-guard', '/var/lib/server-guard',
             '/etc/metric-owl', '/usr/share/doc/server-guard']:
    assert not Path(path).exists(), 'Refusing to touch an existing installation'
subprocess.run(['sudo', '-n', 'true'], check=True)
tag = os.environ['METRIC_OWL_TEST_TAG']
assert re.fullmatch(r'v\d+\.\d+\.\d+(?:-[A-Za-z0-9.]+)?', tag)
out = Path('artifacts/validation/published-install')
out.mkdir(parents=True, exist_ok=True)
username = 'ci-operator'
password = secrets.token_urlsafe(24)
replacement = secrets.token_urlsafe(24)
origin = 'http://127.0.0.1:4877'
jar = http.cookiejar.CookieJar()
client = urllib.request.build_opener(urllib.request.ProxyHandler({}), urllib.request.HTTPCookieProcessor(jar))
checks = []


def request(path, body=None):
    req = urllib.request.Request(origin + '/api/v1/' + path,
        data=json.dumps(body).encode() if body is not None else None,
        headers={'Origin': origin, 'Content-Type': 'application/json'})
    with client.open(req, timeout=8) as response:
        return json.load(response)


def ready():
    deadline = time.monotonic() + 25
    while time.monotonic() < deadline:
        try:
            return request('auth/status')
        except (OSError, ValueError):
            time.sleep(.2)
    raise AssertionError('Installed service did not become ready')


def install(existing=False):
    command = ('curl -fsSL https://raw.githubusercontent.com/SpeakQuery/metric-owl/'
               + tag + '/scripts/bootstrap-linux.sh | sudo -n bash')
    child = pexpect.spawn('bash', ['-c', command], encoding='utf8', timeout=240, echo=False)
    try:
        if not existing:
            child.expect('Choose your Metric Owl administrator username')
            child.sendline(username)
            child.expect('Create a password')
            child.sendline(password)
            child.expect('Enter the password again')
            child.sendline(password)
        child.expect('Enter 1 or 2, then press Enter')
        child.sendline('2')
        child.expect(pexpect.EOF)
        output = child.before
        child.close()
        assert child.exitstatus == 0, 'Published installer failed'
        assert password not in output and replacement not in output, 'Credential echoed'
        assert 'http://127.0.0.1:' in output and '/admin' in output
    finally:
        child.close(force=True)


def root_helper(*args):
    return subprocess.check_output(['sudo', '-n', 'bash',
        '/usr/share/doc/server-guard/recover-admin.sh', *args], text=True)


def purge(*args):
    return subprocess.check_output(['sudo', '-n', 'bash',
        '/usr/share/doc/server-guard/uninstall-linux.sh', *args], text=True)


install()
assert ready()['configured'] is True
assert root_helper('--show-username').strip() == 'Administrator username: ' + username
assert request('auth/login', {'username': username, 'password': password})['session']['csrf']
assert request('current')['memory_total'] > 0
assert request('ai/chat/status')['connected'] is False
checks.append('Anonymous version-pinned bootstrap; terminal account prompts; private SSH access; authenticated real metrics; no provider key required')

metadata = json.loads(subprocess.check_output(['sudo', '-n', 'cat',
    '/usr/share/doc/server-guard/release-metadata.json'], text=True))
assert metadata['version'] == tag and re.fullmatch('[a-f0-9]{40}', metadata['source_commit'])
assert metadata['libc'] == 'musl' and metadata['linkage'] == 'static'
subprocess.run(['sudo', '-n', '/usr/bin/python3', 'scripts/test-recovery-root.py',
                '/usr/local/bin/server-guard'], check=True)
checks.append('Root username lookup preserves service-owned SQLite WAL/SHM; live-WAL account changes remain visible')

child = pexpect.spawn('sudo', ['-n', 'bash', '/usr/share/doc/server-guard/recover-admin.sh'],
                      encoding='utf8', timeout=60, echo=False)
try:
    child.expect('New password')
    child.sendline(replacement)
    child.expect('Enter the new password again')
    child.sendline(replacement)
    child.expect(pexpect.EOF)
    output = child.before
    child.close()
    assert child.exitstatus == 0, 'Root recovery failed'
    assert replacement not in output, 'Recovery password echoed'
finally:
    child.close(force=True)
assert ready()['configured'] is True
try:
    request('current')
    raise AssertionError('Pre-recovery session survived')
except urllib.error.HTTPError as error:
    assert error.code == 401
assert request('auth/login', {'username': username, 'password': replacement})['session']['csrf']
checks.append('Root-only hidden-terminal reset; username lookup; old session revoked; wrapper restarted service; new password login')

plan = json.loads(purge('--purge', '--dry-run'))
assert plan['mode'] == 'purge' and '/var/lib/server-guard' in plan['remove_trees']
assert request('current')['memory_total'] > 0
sentinel = Path('/var/tmp/metric-owl-unrelated-sentinel-' + secrets.token_hex(8))
sentinel.write_text('unrelated application data\n')
try:
    purge()
    assert Path('/var/lib/server-guard').is_dir()
    assert not Path('/usr/local/bin/server-guard').exists()
    install(existing=True)
    assert ready()['configured'] is True
    jar.clear()
    assert request('auth/login', {'username': username, 'password': replacement})['session']['csrf']
    checks.append('Default removal retains data; reinstall preserves recovered administrator and existing history')
    purge('--purge')
    for path in ['/usr/local/bin/server-guard', '/var/lib/server-guard',
                 '/usr/share/doc/server-guard', '/etc/metric-owl/install-manifest.json']:
        assert not Path(path).exists(), 'Owned application path survived purge'
    assert sentinel.read_text() == 'unrelated application data\n'
    checks.append('Purge dry-run has no mutation; actual purge removes owned database/keys/files; unrelated sentinel untouched')
finally:
    sentinel.unlink(missing_ok=True)

receipt = {'result': 'passed', 'tag': tag, 'source_commit': metadata['source_commit'],
    'checks': checks, 'platform': 'fresh disposable GitHub Ubuntu 24.04 x86-64 VM',
    'access': 'private loopback HTTP; SSH instructions printed',
    'public_https_tested': False, 'customer_data_touched': False,
    'provider_requests': 0, 'real_email_sent': False}
(out / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
print(json.dumps(receipt, indent=2))
