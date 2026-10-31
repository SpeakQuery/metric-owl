#!/usr/bin/env python3
"""Disposable installer -> password login -> optional MFA acceptance.
No production data, external requests, or credentials in argv/output.
"""
import base64
import hashlib
import hmac
import http.client
import json
import os
from pathlib import Path
import secrets
import socket
import sqlite3
import struct
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BIN = Path(os.environ.get('GUARD_TEST_BIN', ROOT / 'target/debug/server-guard'))
ORIGIN = 'https://panel.example.invalid'
USERNAME = 'installer_admin'
PASSWORD = 'isolated-installer-enrollment-2026'


def totp(secret):
    digest = hmac.new(base64.b32decode(secret), struct.pack('>Q', int(time.time()) // 30), hashlib.sha1).digest()
    offset = digest[-1] & 15
    return str((struct.unpack('>I', digest[offset:offset + 4])[0] & 0x7fffffff) % 1000000).zfill(6)


with tempfile.TemporaryDirectory(prefix='metric-owl-enrollment-') as temp:
    root = Path(temp)
    data = root / 'data'
    credentials = {'username': USERNAME, 'password': PASSWORD}
    def prepare(payload):
        result = subprocess.run([str(BIN), '--data-dir', str(data), 'prepare-admin'], input=json.dumps(payload), capture_output=True, text=True)
        assert PASSWORD not in result.stdout + result.stderr
        return result
    prepared = prepare(credentials)
    assert prepared.returncode == 0 and json.loads(prepared.stdout) == {'configured': True, 'mfa_required': False}
    assert prepare(credentials).returncode != 0
    assert prepare({**credentials, 'password': 'different-installer-password'}).returncode != 0
    assert prepare({**credentials, 'extra': 'x' * 5000}).returncode != 0
    with sqlite3.connect(data / 'guard.sqlite') as db:
        raw = db.execute("SELECT value FROM kv WHERE key='admin'").fetchone()[0]
        assert PASSWORD not in raw
        admin = json.loads(raw)
        assert admin['password'].startswith('$argon2id$') and admin['mfa_enabled'] is False
        assert admin['secret'] == '' and admin['recovery'] == []
        assert db.execute("SELECT COUNT(*) FROM kv WHERE key IN ('installer_pending','bootstrap','pending_admin')").fetchone()[0] == 0
    proxy_token = secrets.token_hex(32)
    token_file = root / 'proxy-token'
    token_file.write_text(proxy_token)
    token_file.chmod(0o600)
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        port = listener.getsockname()[1]
    env = {**os.environ, 'GUARD_PUBLIC_ORIGIN': ORIGIN, 'GUARD_PROXY_TOKEN_FILE': str(token_file)}
    log = (root / 'agent.log').open('w')
    def launch():
        return subprocess.Popen([str(BIN), '--data-dir', str(data), 'serve', '--port', str(port)], env=env, stdout=subprocess.DEVNULL, stderr=log)
    agent = launch()
    checks = ['bounded non-echoing CLI stdin; password stored as Argon2id; existing account cannot be overwritten']
    def request(path, body=None, cookie=None, csrf=None):
        h = {'Host':'panel.example.invalid', 'Origin':ORIGIN, 'X-Forwarded-Proto':'https', 'X-Guard-Proxy-Token':proxy_token, 'Content-Type':'application/json'}
        if cookie:
            h['Cookie'] = cookie
        if csrf:
            h['X-CSRF-Token'] = csrf
        connection = http.client.HTTPConnection('127.0.0.1', port, timeout=8)
        try:
            connection.request('POST' if body is not None else 'GET', '/api/v1/' + path, json.dumps(body) if body is not None else None, h)
            response = connection.getresponse()
            return response.status, dict(response.getheaders()), json.loads(response.read())
        finally:
            connection.close()
    def ready():
        deadline = time.monotonic() + 20
        while True:
            try:
                response = request('auth/status')
                if response[0] == 200:
                    return response[2]
            except OSError:
                pass
            if time.monotonic() > deadline:
                raise AssertionError('Enrollment test process did not start')
            time.sleep(.1)
    try:
        status = ready()
        assert status['configured'] is True and status['enrollment_ready'] is False and status['mfa_required'] is False
        assert not (data / 'setup-key').exists()
        assert request('current')[0] == 401
        assert request('auth/login', {**credentials, 'password':'wrong'})[0] == 400
        code, headers, login = request('auth/login', credentials)
        assert code == 200 and headers['set-cookie'].startswith('__Host-guard_session=') and '; Secure' in headers['set-cookie']
        cookie, csrf = headers['set-cookie'].split(';', 1)[0], login['session']['csrf']
        assert request('host', cookie=cookie)[0] == 200
        assert request('security', cookie=cookie)[2]['mfa_required'] is False
        assert request('auth/reauth', credentials, cookie, csrf)[0] == 200
        assert request('auth/mfa/begin', {}, cookie)[0] == 403
        code, _, challenge = request('auth/mfa/begin', {}, cookie, csrf)
        assert code == 200 and challenge['uri'].startswith('otpauth://totp/')
        checks.append('HTTPS password-only sign-in and reauthentication; secure cookie; no setup key or QR required')
        code, _, finished = request('auth/mfa/finish', {'code':totp(challenge['secret'])}, cookie, csrf)
        assert code == 200 and len(finished['recovery_codes']) == 10
        assert request('host', cookie=cookie)[0] == 401
        assert request('auth/status')[2]['mfa_required'] is True
        assert request('auth/login', credentials)[0] == 400
        code, headers, login = request('auth/login', {**credentials, 'code':finished['recovery_codes'][0]})
        assert code == 200
        cookie, csrf = headers['set-cookie'].split(';', 1)[0], login['session']['csrf']
        assert request('security', cookie=cookie)[2]['mfa_required'] is True
        with sqlite3.connect(data / 'guard.sqlite') as db:
            db.execute('UPDATE sessions SET step_up=0')
        assert request('auth/mfa/disable', {}, cookie, csrf)[0] == 400
        assert request('auth/reauth', credentials, cookie, csrf)[0] == 400
        assert request('auth/reauth', {**credentials, 'code':finished['recovery_codes'][1]}, cookie, csrf)[0] == 200
        assert request('auth/mfa/disable', {}, cookie, csrf)[0] == 200
        assert request('host', cookie=cookie)[0] == 401
        assert request('auth/status')[2]['mfa_required'] is False
        with sqlite3.connect(data / 'guard.sqlite') as db:
            after_disable = json.loads(db.execute("SELECT value FROM kv WHERE key='admin'").fetchone()[0])
            assert after_disable['mfa_enabled'] is False and after_disable['secret'] == '' and after_disable['recovery'] == []
        checks.append('optional QR enables MFA and revokes sessions; disabling needs current MFA reauthentication and clears secrets/sessions')
        code, _, _ = request('auth/login', credentials)
        assert code == 200
        assert prepare(credentials).returncode != 0
        agent.terminate()
        agent.wait(timeout=10)
        agent = launch()
        status = ready()
        assert status['configured'] is True and status['enrollment_ready'] is False and status['mfa_required'] is False
        assert not (data / 'setup-key').exists()
        with sqlite3.connect(data / 'guard.sqlite') as db:
            current = json.loads(db.execute("SELECT value FROM kv WHERE key='admin'").fetchone()[0])
            assert current == after_disable
        checks.append('restart preserves account and chosen MFA state; setup key is not recreated')
        print(json.dumps({'result':'passed', 'checks':checks, 'owner_data_touched':False, 'external_requests':False, 'typed_setup_key_required':False, 'default_mfa_required':False}))
    finally:
        agent.terminate()
        try:
            agent.wait(timeout=10)
        except subprocess.TimeoutExpired:
            agent.kill()
            agent.wait(timeout=5)
        log.close()
