#!/usr/bin/env python3
"""Configure a new, dedicated HTTPS hostname without editing existing websites.

Invoked by bootstrap-linux.sh. Existing operator-managed HTTPS is preserved.
No shell interpolation, credential output, firewall changes, or public app port.
"""
import ipaddress
import getpass
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request
import warnings

MARKER = Path('/etc/metric-owl/panel-access.json')
CONF = Path('/etc/nginx/conf.d/metricowl-panel.conf')
PROXY = Path('/etc/nginx/snippets/metricowl-panel-proxy.conf')
TOKEN = Path('/etc/metric-owl/panel-proxy-token')
DROPIN = Path('/etc/systemd/system/server-guard.service.d/metricowl-https.conf')
RENEWAL = Path('/etc/letsencrypt/renewal-hooks/deploy/metricowl-panel-reload')
WEBROOT = Path('/var/www/metricowl-panel-acme')
TOS = 'https://letsencrypt.org/repository/'


def local_status():
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with opener.open('http://127.0.0.1:4877/api/v1/auth/status', timeout=5) as response:
        data = json.loads(response.read(4096))
    if not isinstance(data.get('configured'), bool):
        raise ValueError('Could not verify the local administrator state.')
    return data


def administrator_credentials():
    username = prompt('Choose your Metric Owl administrator username', 'admin')
    if not re.fullmatch(r'[A-Za-z0-9._@-]{3,64}', username):
        raise ValueError('Use 3–64 letters, digits, dots, underscores, @ or hyphens for your username.')
    try:
        with open('/dev/tty', 'r+') as terminal:
            with warnings.catch_warnings():
                warnings.simplefilter('error', getpass.GetPassWarning)
                password = getpass.getpass('Create a password (at least 14 characters): ', stream=terminal)
                confirmation = getpass.getpass('Enter the password again: ', stream=terminal)
    except (OSError, EOFError, getpass.GetPassWarning):
        raise ValueError('Administrator setup needs your SSH terminal. No password was saved.') from None
    if password != confirmation:
        raise ValueError('Passwords did not match. Run the installer again; no account was overwritten.')
    if not 14 <= len(password.encode()) <= 256:
        raise ValueError('Use a password of 14–256 bytes. A long passphrase works well.')
    return {'username': username, 'password': password}


def prepare_administrator():
    if os.geteuid() != 0:
        raise ValueError('Administrator preparation requires root privileges.')
    status = local_status()
    if status['configured']:
        print('Your existing administrator account is unchanged.')
        return
    if status.get('enrollment_ready') is True:
        print('Administrator details are already prepared. Finish authenticator setup in your browser.')
        return
    credentials = administrator_credentials()
    encoded = json.dumps(credentials).encode()
    credentials.clear()
    # The app has an exclusive IPC listener. Do not write authentication state
    # beside its live in-memory state. Never pass a password in argv or the environment.
    try:
        run(['systemctl', 'stop', 'server-guard'])
        result = subprocess.run([
            'runuser', '-u', 'server-guard', '--', '/usr/local/bin/server-guard',
            '--data-dir', '/var/lib/server-guard', 'prepare-admin'], input=encoded,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        encoded = b''
        if result.returncode != 0:
            raise RuntimeError('Administrator details could not be prepared. '
                               'Existing authentication data was not reset. Run setup again or review the service locally.')
        if json.loads(result.stdout).get('configured') is not True:
            raise RuntimeError('Administrator preparation did not return the expected confirmation.')
    finally:
        encoded = b''
        run(['systemctl', 'start', 'server-guard'])
    for _ in range(20):
        try:
            status = local_status()
            if status['configured']:
                print('Administrator account created. Your password was not displayed or logged.')
                return
        except (OSError, ValueError, urllib.error.URLError):
            pass
        time.sleep(0.5)
    raise RuntimeError('Administrator preparation completed, but service readiness could not be confirmed.')


def hostname(value):
    value = value.strip().lower()
    if (len(value) > 253 or '.' not in value or
            any(not re.fullmatch(r'[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?', label)
                for label in value.split('.'))):
        raise ValueError('Enter a hostname such as monitor.example.com, without https://, a port or a path.')
    try:
        ipaddress.ip_address(value)
    except ValueError:
        if value.endswith(('.localhost', '.local', '.test', '.invalid', '.example')):
            raise ValueError('Use a publicly registered hostname whose DNS points to this server.') from None
        return value
    raise ValueError('Use a domain hostname for a trusted HTTPS certificate, not a bare IP address.')


def public_origin(value):
    if not isinstance(value, str) or not value.startswith('https://'):
        raise ValueError('Invalid HTTPS access marker; review it before changing the installation.')
    name = hostname(value[8:])
    if value != 'https://' + name:
        raise ValueError('Invalid HTTPS access marker; expected a canonical origin without a path.')
    return value


def existing_origin(path=MARKER):
    if path.is_symlink():
        raise ValueError('Unsafe HTTPS access marker.')
    if not path.exists():
        return None
    if not path.is_file() or path.stat().st_size > 4096:
        raise ValueError('Unsafe HTTPS access marker.')
    data = json.loads(path.read_text())
    if data.get('managed') not in ('operator', 'installer'):
        raise ValueError('Unknown HTTPS manager; existing configuration was left unchanged.')
    return public_origin(data.get('public_origin'))


def prompt(text, default=''):
    try:
        with open('/dev/tty', 'r+') as tty:
            tty.write(text + (f' [{default}]' if default else '') + ': ')
            tty.flush()
            line = tty.readline()
    except OSError:
        raise ValueError('HTTPS setup needs an interactive terminal. Set GUARD_HTTPS_DOMAIN, '
                         'GUARD_HTTPS_AGREE_TOS=1 and optionally GUARD_HTTPS_EMAIL for unattended setup; '
                         'or GUARD_CONFIGURE_HTTPS=0 for deliberate SSH-only access.') from None
    if not line:
        raise ValueError('No answer received. HTTPS setup was cancelled.')
    return line.strip() or default


def run(args, *, allowed=(0,)):
    result = subprocess.run(args, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.returncode not in allowed:
        # nginx -T and errors may include a private proxy header: never echo their output.
        raise RuntimeError(f'{Path(args[0]).name} did not complete successfully. '
                           'Review that service locally; private configuration was not printed.')
    return result


def host_collision(config, domain):
    for declaration in re.findall(r'\bserver_name\s+([^;]+);', re.sub(r'#.*', '', config)):
        for name in declaration.split():
            name = name.strip('"\'').lower()
            if name == domain:
                return True
            if name.startswith('.') and (domain == name[1:] or domain.endswith(name)):
                return True
            if name.startswith('*.') and domain.endswith(name[1:]):
                return True
            if name.endswith('.*') and domain.startswith(name[:-1]):
                return True
            # nginx regex matching can be complex. Do not rewrite an ambiguous installation.
            if name.startswith('~'):
                raise ValueError('Existing Nginx regex hostnames require an operator-reviewed HTTPS setup. '
                                 'No existing site was changed.')
    return False


def atomic(path, text, mode=0o644):
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + '.metricowl-' + secrets.token_hex(5))
    fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, mode)
    try:
        with os.fdopen(fd, 'w') as file:
            file.write(text)
            file.flush()
            os.fsync(file.fileno())
        os.replace(temporary, path)
        os.chmod(path, mode)
    finally:
        if temporary.exists():
            temporary.unlink()


def challenge_config(domain):
    return f'''# Managed by Metric Owl. A dedicated hostname, never an existing virtual host.
server {{
    listen 80;
    listen [::]:80;
    server_name {domain};
    if ($host != {domain}) {{ return 444; }}
    server_tokens off;
    location ^~ /.well-known/acme-challenge/ {{
        root {WEBROOT};
        default_type text/plain;
        try_files $uri =404;
    }}
    location / {{ return 404; }}
}}
'''


def proxy_config(domain, token):
    if not re.fullmatch(r'[a-f0-9]{64}', token):
        raise ValueError('Invalid generated proxy credential.')
    return f'''# Root-readable credential; never copy this file into a repository.
proxy_pass http://127.0.0.1:4877;
proxy_http_version 1.1;
proxy_set_header Host {domain};
proxy_set_header X-Forwarded-Proto https;
proxy_set_header X-Guard-Proxy-Token "{token}";
proxy_set_header X-Real-IP $remote_addr;
proxy_set_header X-Forwarded-For $remote_addr;
proxy_set_header X-Forwarded-Host "";
proxy_set_header Forwarded "";
proxy_set_header Connection "";
proxy_buffering off;
proxy_cache off;
proxy_connect_timeout 5s;
proxy_read_timeout 75s;
proxy_send_timeout 15s;
proxy_hide_header X-Powered-By;
'''


def tls_config(domain):
    challenge = challenge_config(domain).replace('location / { return 404; }',
                                                 f'location / {{ return 301 https://{domain}$request_uri; }}')
    return f'''limit_req_zone $binary_remote_addr zone=metricowl_panel_login:1m rate=6r/m;
limit_conn_zone $binary_remote_addr zone=metricowl_panel_connections:1m;
{challenge}
server {{
    listen 443 ssl http2;
    listen [::]:443 ssl http2;
    server_name {domain};
    if ($host != {domain}) {{ return 444; }}
    ssl_certificate /etc/letsencrypt/live/{domain}/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/{domain}/privkey.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_session_tickets off;
    server_tokens off;
    client_max_body_size 64k;
    client_body_timeout 15s;
    limit_conn metricowl_panel_connections 32;
    limit_req_status 429;
    access_log /var/log/nginx/metricowl-panel-access.log;
    error_log /var/log/nginx/metricowl-panel-error.log warn;
    add_header Strict-Transport-Security "max-age=31536000" always;
    add_header X-Content-Type-Options nosniff always;
    add_header X-Frame-Options DENY always;
    add_header Referrer-Policy no-referrer always;
    location = / {{ return 302 /admin; }}
    location = /admin {{ include {PROXY}; }}
    location ^~ /admin/ {{ include {PROXY}; }}
    location ~ ^/api/v1/auth/(login|setup/begin|setup/enroll|setup/finish|reauth)$ {{
        limit_req zone=metricowl_panel_login burst=5 nodelay;
        include {PROXY};
    }}
    location /api/v1/ {{ include {PROXY}; }}
    location / {{ return 404; }}
}}
'''


def verify_panel(domain):
    # Verify DNS, certificate and application together through the public hostname.
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    for _ in range(20):
        try:
            with opener.open('https://' + domain + '/api/v1/auth/status', timeout=3) as response:
                data = json.loads(response.read(4096))
                if response.status == 200 and isinstance(data.get('configured'), bool):
                    return
        except (OSError, ValueError, urllib.error.URLError):
            pass
        time.sleep(0.5)
    raise RuntimeError('The HTTPS certificate and panel could not be verified. '
                       'Check DNS and inbound ports 80/443. The application remains private.')


def configure():
    if os.geteuid() != 0:
        raise ValueError('HTTPS installation requires root privileges.')
    origin = existing_origin()
    if origin:
        verify_panel(origin[8:])
        print('Preserved existing HTTPS configuration: ' + origin + '/admin')
        return
    # An existing drop-in can mean an operator setup without a marker. Never replace it.
    for path in (CONF, PROXY, TOKEN, DROPIN, RENEWAL):
        if path.exists() or path.is_symlink():
            raise ValueError('An existing Metric Owl HTTPS file needs operator review: ' + str(path) +
                             '. Nothing was overwritten.')
    domain = hostname(os.environ.get('GUARD_HTTPS_DOMAIN') or prompt(
        'Panel hostname (for example monitor.example.com; point its DNS to this server first)'))
    try:
        addresses = {item[4][0] for item in socket.getaddrinfo(domain, 443, type=socket.SOCK_STREAM)}
    except socket.gaierror:
        raise ValueError('The panel hostname does not resolve. Point its DNS to this server and retry.') from None
    if not addresses or any(not ipaddress.ip_address(address).is_global for address in addresses):
        raise ValueError('The panel hostname must resolve only to public addresses for certificate validation.')
    certificate = Path('/etc/letsencrypt/live') / domain
    if certificate.exists() or certificate.is_symlink():
        raise ValueError('A certificate already exists for this hostname. Use an operator-reviewed proxy '
                         'configuration; the installer will not change an existing certificate.')
    # An installed nginx executable does not mean nginx owns the web ports.
    listeners = run(['ss', '-H', '-ltnp', '( sport = :80 or sport = :443 )']).stdout.strip()
    if listeners and any('"nginx"' not in line for line in listeners.splitlines()):
        raise ValueError('Another web server uses ports 80/443. It was left unchanged. '
                         'An operator must add an HTTPS proxy to that web server.')
    nginx_was_active = run(['systemctl', 'is-active', '--quiet', 'nginx'], allowed=(0, 3, 4)).returncode == 0
    nginx_was_enabled = run(['systemctl', 'is-enabled', '--quiet', 'nginx'], allowed=(0, 1, 3, 4)).returncode == 0
    # Discover conflicts before package installation or server reconfiguration.
    if shutil.which('nginx'):
        existing = run(['nginx', '-T']).stdout
        if host_collision(existing, domain):
            raise ValueError('This hostname already belongs to an existing Nginx website. '
                             'Choose a dedicated hostname or ask an operator to add the panel safely.')
        if not re.search(r'include\s+/etc/nginx/conf\.d/\*\.conf\s*;', existing):
            raise ValueError('Nginx does not include the standard conf.d directory. Existing configuration '
                             'was left unchanged; an operator must configure HTTPS.')
    email = os.environ.get('GUARD_HTTPS_EMAIL')
    if email is None:
        email = prompt('Certificate email (optional; press Enter to skip)')
    if email and not re.fullmatch(r'[^\s@]+@[^\s@]+\.[^\s@]+', email):
        raise ValueError('Enter a valid certificate contact email, or leave it empty.')
    agreed = os.environ.get('GUARD_HTTPS_AGREE_TOS') == '1'
    if not agreed:
        print('A free HTTPS certificate is issued by Let’s Encrypt. Terms: ' + TOS)
        agreed = prompt('Accept the certificate authority terms? (yes/no)', 'no').lower() == 'yes'
    if not agreed:
        raise ValueError('Certificate terms were not accepted. No HTTPS configuration was changed.')
    missing = [package for package in ('nginx', 'certbot') if not shutil.which(package)]
    if missing:
        print('Installing HTTPS prerequisites: ' + ', '.join(missing), flush=True)
        subprocess.run(['apt-get', 'update'], check=True)
        subprocess.run(['apt-get', 'install', '-y', '--no-install-recommends', *missing],
                       check=True, env={**os.environ, 'DEBIAN_FRONTEND': 'noninteractive'})
    created = []
    app_changed = False
    try:
        # bootstrap has umask 077; ACME challenge directories must be traversable
        # by the restricted nginx worker without exposing any private credential.
        for path in (WEBROOT, WEBROOT / '.well-known', WEBROOT / '.well-known/acme-challenge'):
            if path.is_symlink():
                raise ValueError('Unsafe ACME challenge directory; nothing was overwritten.')
            path.mkdir(parents=True, exist_ok=True)
            path.chmod(0o755)
        atomic(CONF, challenge_config(domain))
        created.append(CONF)
        run(['nginx', '-t'])
        run(['systemctl', 'enable', '--now', 'nginx'])
        run(['systemctl', 'reload', 'nginx'])
        print('Requesting the HTTPS certificate. DNS must point here and port 80 must be reachable…', flush=True)
        args = ['certbot', 'certonly', '--webroot', '--webroot-path', str(WEBROOT), '--domain', domain,
                '--cert-name', domain, '--non-interactive', '--agree-tos', '--no-eff-email']
        args += ['--email', email] if email else ['--register-unsafely-without-email']
        run(args)
        token = secrets.token_hex(32)
        atomic(TOKEN, token + '\n', 0o600)
        created.append(TOKEN)
        atomic(PROXY, proxy_config(domain, token), 0o600)
        created.append(PROXY)
        token = ''
        atomic(DROPIN, '[Service]\nEnvironment=GUARD_PUBLIC_ORIGIN=https://' + domain +
               '\nLoadCredential=metricowl-proxy-token:' + str(TOKEN) +
               '\nEnvironment=GUARD_PROXY_TOKEN_FILE=%d/metricowl-proxy-token\n')
        created.append(DROPIN)
        run(['systemctl', 'daemon-reload'])
        app_changed = True
        run(['systemctl', 'restart', 'server-guard'])
        atomic(CONF, tls_config(domain))
        run(['nginx', '-t'])
        run(['systemctl', 'reload', 'nginx'])
        verify_panel(domain)
        atomic(RENEWAL, '#!/bin/sh\nset -eu\n[ "${RENEWED_LINEAGE:-}" = "/etc/letsencrypt/live/' + domain +
               '" ] || exit 0\n/usr/sbin/nginx -t >/dev/null 2>&1 && /bin/systemctl reload nginx\n', 0o755)
        created.append(RENEWAL)
        atomic(MARKER, json.dumps({'public_origin': 'https://' + domain, 'managed': 'installer'}, indent=2) + '\n')
        created.append(MARKER)
        print('HTTPS is ready. Open https://' + domain + '/admin from any computer or phone.')
    except Exception:
        # Remove only files created by this invocation, never an existing site/certificate.
        for path in reversed(created):
            path.unlink(missing_ok=True)
        if app_changed:
            run(['systemctl', 'daemon-reload'])
            run(['systemctl', 'restart', 'server-guard'])
        run(['nginx', '-t'])
        if nginx_was_active:
            run(['systemctl', 'reload', 'nginx'])
        else:
            run(['systemctl', 'stop', 'nginx'])
        if not nginx_was_enabled:
            run(['systemctl', 'disable', 'nginx'])
        print('This attempt’s HTTPS configuration was rolled back. Existing websites were not edited. '
              'Any certificate issued during this attempt was retained for operator review.', file=sys.stderr)
        raise


if __name__ == '__main__':
    try:
        if sys.argv[1:] == ['--prepare-admin']:
            prepare_administrator()
        elif not sys.argv[1:]:
            configure()
        else:
            raise ValueError('Usage: configure-https.py [--prepare-admin]')
    except (ValueError, RuntimeError) as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1) from None
    except Exception as error:
        print('HTTPS setup failed (' + type(error).__name__ + '). Private details were not logged.', file=sys.stderr)
        raise SystemExit(1) from None
