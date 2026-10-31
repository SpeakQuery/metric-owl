#!/usr/bin/env python3
"""Isolated trusted-HTTPS-proxy contract test. No owner data, mail or AI calls.

The TCP hop intentionally models Nginx -> loopback HTTP; production TLS itself
must be validated separately by the deployment smoke test.
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
PASSWORD = 'isolated-public-access-password-2026'


def totp(secret):
    digest = hmac.new(base64.b32decode(secret), struct.pack('>Q', int(time.time()) // 30), hashlib.sha1).digest()
    offset = digest[-1] & 15
    return str((struct.unpack('>I', digest[offset:offset + 4])[0] & 0x7fffffff) % 1000000).zfill(6)


with tempfile.TemporaryDirectory(prefix='metric-owl-public-access-') as temp:
    root = Path(temp)
    data = root / 'data'
    proxy_token = secrets.token_hex(32)
    token_file = root / 'proxy-token'
    token_file.write_text(proxy_token)
    token_file.chmod(0o600)
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        port = listener.getsockname()[1]
    env = {**os.environ, 'GUARD_PUBLIC_ORIGIN': ORIGIN, 'GUARD_PROXY_TOKEN_FILE': str(token_file)}
    log = (root / 'agent.log').open('w')
    agent = subprocess.Popen([str(BIN), '--data-dir', str(data), 'serve', '--port', str(port)], env=env, stdout=subprocess.DEVNULL, stderr=log)
    checks = []

    def request(path, body=None, *, public=True, headers=None, cookie=None, csrf=None):
        h = {'Host': 'panel.example.invalid' if public else f'127.0.0.1:{port}', 'Origin': ORIGIN if public else f'http://127.0.0.1:{port}'}
        if public:
            h.update({'X-Guard-Proxy-Token': proxy_token, 'X-Forwarded-Proto': 'https'})
        if cookie:
            h['Cookie'] = cookie
        if csrf:
            h['X-CSRF-Token'] = csrf
        for name, value in (headers or {}).items():
            if value is None:
                h.pop(name, None)
            else:
                h[name] = value
        h['Content-Type'] = 'application/json'
        connection = http.client.HTTPConnection('127.0.0.1', port, timeout=8)
        try:
            connection.request('POST' if body is not None else 'GET', path, json.dumps(body) if body is not None else None, h)
            response = connection.getresponse()
            content = response.read()
            return response.status, dict(response.getheaders()), json.loads(content) if content else None
        finally:
            connection.close()

    try:
        deadline = time.monotonic() + 20
        while True:
            try:
                status, _, value = request('/api/v1/auth/status')
                assert status == 200 and value['configured'] is False
                break
            except (OSError, AssertionError):
                if time.monotonic() > deadline:
                    raise AssertionError('Public access test process did not start')
                time.sleep(.1)
        for headers in [
            {'X-Guard-Proxy-Token': None}, {'X-Guard-Proxy-Token': 'wrong'},
            {'X-Forwarded-Proto': None}, {'X-Forwarded-Proto': 'http'},
            {'Host': 'attacker.example'}, {'Origin': 'https://attacker.example'},
        ]:
            status, response_headers, _ = request('/api/v1/auth/status', headers=headers)
            assert status == 403 and response_headers.get('cache-control') == 'no-store'
        assert request('/api/v1/auth/login', {}, headers={'Origin': None})[0] == 403
        assert request('/api/v1/current')[0] == 401
        checks.append('exact Host, HTTPS proxy proof, same Origin and no-store errors')

        setup = (data / 'setup-key').read_text()
        begin = {'key': setup, 'username': 'admin', 'password': PASSWORD}
        for invalid in ('', 'wrong'):
            assert request('/api/v1/auth/setup/begin', {**begin, 'key': invalid})[0] == 400
        with sqlite3.connect(data / 'guard.sqlite') as db:
            original = db.execute("SELECT value FROM kv WHERE key='bootstrap'").fetchone()[0]
            expired = {**json.loads(original), 'expires': 0}
            db.execute("UPDATE kv SET value=? WHERE key='bootstrap'", (json.dumps(expired),))
        assert request('/api/v1/auth/setup/begin', begin)[0] == 400
        with sqlite3.connect(data / 'guard.sqlite') as db:
            db.execute("UPDATE kv SET value=? WHERE key='bootstrap'", (original,))
        status, _, challenge = request('/api/v1/auth/setup/begin', begin)
        assert status == 200 and challenge['uri'].startswith('otpauth://totp/')
        status, _, recovery = request('/api/v1/auth/setup/finish', {'key': setup, 'code': totp(challenge['secret'])})
        assert status == 200 and len(recovery['recovery_codes']) == 10
        assert not (data / 'setup-key').exists()
        assert request('/api/v1/auth/setup/begin', begin)[0] == 400
        checks.append('public setup rejects missing/wrong/expired keys; key consumed after MFA; repeat setup rejected')

        credentials = {'username': 'admin', 'password': PASSWORD}
        assert request('/api/v1/auth/login', {**credentials, 'code': 'invalid'})[0] == 400
        status, headers, login = request('/api/v1/auth/login', {**credentials, 'code': recovery['recovery_codes'][0]})
        assert status == 200
        set_cookie = headers['set-cookie']
        assert set_cookie.startswith('__Host-guard_session=') and '; Secure' in set_cookie
        assert '; HttpOnly' in set_cookie and '; SameSite=Strict' in set_cookie and '; Path=/' in set_cookie and 'Domain=' not in set_cookie
        cookie = set_cookie.split(';', 1)[0]
        csrf = login['session']['csrf']
        assert request('/api/v1/auth/status', cookie=cookie)[2]['session'] is not None
        assert request('/api/v1/host', cookie=cookie)[0] == 200
        assert request('/api/v1/host', cookie=cookie.replace('__Host-', ''))[0] == 401
        assert request('/api/v1/host', cookie=cookie, public=False)[0] == 401
        assert request('/api/v1/auth/logout', {}, cookie=cookie)[0] == 403
        assert request('/api/v1/auth/logout', {}, cookie=cookie, csrf=csrf, headers={'Origin': 'https://attacker.example'})[0] == 403
        checks.append('MFA required; Secure host-only session cookie; separate local/public cookie names; CSRF enforced')

        connection = http.client.HTTPConnection('127.0.0.1', port, timeout=8)
        try:
            connection.request('GET', '/api/v1/live', headers={'Host':'panel.example.invalid', 'Origin':ORIGIN, 'X-Forwarded-Proto':'https', 'X-Guard-Proxy-Token':proxy_token, 'Cookie':cookie})
            stream = connection.getresponse()
            assert stream.status == 200 and stream.getheader('content-type').startswith('text/event-stream')
            observed = b''
            while len(observed) < 100000:
                line = stream.readline()
                if not line:
                    break
                observed += line
                if b'event: sample' in observed and b'data:' in observed:
                    break
            assert b'event: sample' in observed and b'data:' in observed
        finally:
            connection.close()
        checks.append('authenticated live SSE works through trusted proxy mode')

        status, logout_headers, _ = request('/api/v1/auth/logout', {}, cookie=cookie, csrf=csrf)
        assert status == 200
        assert logout_headers['set-cookie'].startswith('__Host-guard_session=;') and 'Max-Age=0' in logout_headers['set-cookie'] and '; Secure' in logout_headers['set-cookie']
        assert request('/api/v1/host', cookie=cookie)[0] == 401
        status, local_headers, local_login = request('/api/v1/auth/login', {**credentials, 'code': recovery['recovery_codes'][1]}, public=False)
        assert status == 200
        assert local_headers['set-cookie'].startswith('guard_session=') and '; Secure' not in local_headers['set-cookie']
        local_cookie = local_headers['set-cookie'].split(';', 1)[0]
        assert request('/api/v1/host', cookie=local_cookie, public=False)[0] == 200
        assert request('/api/v1/host', cookie=local_cookie)[0] == 401
        assert request('/api/v1/auth/logout', {}, cookie=local_cookie, csrf=local_login['session']['csrf'], public=False)[0] == 200
        checks.append('logout revokes session; local/SSH login remains functional')
        print(json.dumps({'result': 'passed', 'checks': checks, 'owner_data_touched': False, 'external_requests': False, 'tls_tested': False, 'note': 'Trusted reverse-proxy backend contract. Actual HTTPS termination needs deployment validation.'}))
    finally:
        agent.terminate()
        try:
            agent.wait(timeout=10)
        except subprocess.TimeoutExpired:
            agent.kill()
            agent.wait(timeout=5)
        log.close()
