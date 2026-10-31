#!/usr/bin/env python3
"""Configure a dedicated HTTPS panel on the server’s public IPv4 address.

Invoked by bootstrap-linux.sh. Existing operator-managed HTTPS is preserved.
No shell interpolation or credential output. Only owned HTTPS/firewall entries change.
"""
import ipaddress
import getpass
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import shlex
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
RENEWAL = Path('/etc/metric-owl/renew-panel')
SITES_CONF = Path('/etc/nginx/sites-enabled/zz-metricowl-panel.conf')
CERTBOT = Path('/opt/metric-owl/certbot/bin/certbot')
CERTBOT_VERSION = '5.9.0'
ACME = Path('/etc/metric-owl/acme')
ACME_WORK = Path('/var/lib/metric-owl-acme')
ACME_LOGS = Path('/var/log/metric-owl-acme')
RENEW_SERVICE = Path('/etc/systemd/system/metricowl-certificate-renew.service')
RENEW_TIMER = Path('/etc/systemd/system/metricowl-certificate-renew.timer')
FIREWALL_TAG = 'Metric Owl installer'
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


def public_ip(value):
    try:
        address = ipaddress.IPv4Address(value)
    except (ipaddress.AddressValueError, TypeError):
        raise ValueError('Enter a public IPv4 address, without https://, a port or a path.') from None
    if not address.is_global or address.is_multicast or address.is_reserved:
        raise ValueError('This installer needs a public IPv4 address reachable from the internet.')
    return str(address)


def panel_port(value):
    if not isinstance(value, str) or not re.fullmatch(r'[1-9][0-9]{0,4}', value):
        raise ValueError('The panel port must be a number from 1024 to 65535.')
    number = int(value)
    if not 1024 <= number <= 65535 or number in (4877, 4878):
        raise ValueError('Choose a dedicated panel port from 1024 to 65535, excluding 4877/4878.')
    return number


def choose_panel_port():
    explicit = os.environ.get('GUARD_PANEL_PORT')
    candidates = [panel_port(explicit)] if explicit else [8443, 9443, 10443]
    for number in candidates:
        if not run(['ss', '-H', '-ltn', 'sport', '=', ':' + str(number)]).stdout.strip():
            return number
    choices = ', '.join(map(str, candidates))
    raise ValueError('Panel ports ' + choices + ' are already used. No service was stopped. '
                     'Choose a free GUARD_PANEL_PORT and retry.')


def discover_public_ip():
    addresses = []
    result = run(['ip', '-j', '-4', 'addr', 'show', 'scope', 'global'])
    for interface in json.loads(result.stdout):
        for info in interface.get('addr_info', []):
            try:
                value = public_ip(info.get('local'))
            except ValueError:
                continue
            if value not in addresses:
                addresses.append(value)
    # An SSH target helps on multihomed servers without contacting an IP-lookup service.
    connection = os.environ.get('SSH_CONNECTION', '').split()
    if len(connection) == 4:
        try:
            target = public_ip(connection[2])
            if target in addresses or not addresses:
                return target
        except ValueError:
            pass
    return addresses[0] if len(addresses) == 1 else ''


def public_origin(value):
    if not isinstance(value, str) or not value.startswith('https://'):
        raise ValueError('Invalid HTTPS access marker.')
    authority = value[8:]
    if ':' in authority:
        address, port = authority.rsplit(':', 1)
        expected = 'https://' + public_ip(address) + ':' + str(panel_port(port))
        if value != expected:
            raise ValueError('Invalid HTTPS access marker.')
        return value
    # Preserve reviewed hostname installations from earlier releases. New installs use IPs.
    if (len(authority) > 253 or '.' not in authority or
            any(not re.fullmatch(r'[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?', part)
                for part in authority.split('.'))):
        raise ValueError('Invalid HTTPS access marker.')
    return value


def marker_data(path=None):
    path = path or MARKER
    if path.is_symlink() or not path.is_file() or path.stat().st_size > 8192:
        raise ValueError('Unsafe HTTPS access marker.')
    data = json.loads(path.read_text())
    if data.get('managed') not in ('operator', 'installer'):
        raise ValueError('Unknown HTTPS manager; existing configuration was left unchanged.')
    public_origin(data.get('public_origin'))
    return data


def existing_origin(path=None):
    path = path or MARKER
    if path.is_symlink():
        raise ValueError('Unsafe HTTPS access marker.')
    return marker_data(path)['public_origin'] if path.exists() else None


def prompt(text, default=''):
    try:
        with open('/dev/tty', 'r+') as tty:
            tty.write(text + (f' [{default}]' if default else '') + ': ')
            tty.flush()
            line = tty.readline()
    except OSError:
        raise ValueError('Setup needs your SSH terminal. For deliberate unattended HTTPS setup use '
                         'GUARD_PUBLIC_IP, GUARD_HTTPS_AGREE_TOS=1 and optional GUARD_HTTPS_EMAIL. '
                         'Administrator password entry still needs a terminal on a new installation.') from None
    if not line:
        raise ValueError('No answer received. Setup was cancelled.')
    return line.strip() or default


def run(args, *, allowed=(0,)):
    result = subprocess.run(args, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    if result.returncode not in allowed:
        # nginx errors can include a private proxy header; never echo command output.
        raise RuntimeError(f'{Path(args[0]).name} did not complete successfully. '
                           'Review that service locally; private configuration was not printed.')
    return result


def host_collision(config, address):
    for declaration in re.findall(r'\bserver_name\s+([^;]+);', re.sub(r'#.*', '', config)):
        for name in declaration.split():
            name = name.strip('"\'').lower()
            if name == address:
                return True
            if name.startswith('~'):
                raise ValueError('Existing Nginx regex hostnames need operator review. No site was changed.')
    return False


def select_conf(config):
    # conf.d is commonly included before sites-enabled. Appending the new IP host
    # after existing sites preserves their implicit default host as well as explicit defaults.
    if re.search(r'include\s+/etc/nginx/sites-enabled/\*\s*;', config):
        later = [p for p in SITES_CONF.parent.iterdir() if p.name >= SITES_CONF.name]
        if later:
            raise ValueError('The Nginx site ordering needs operator review; existing default hosts were preserved.')
        return SITES_CONF
    if (re.search(r'include\s+/etc/nginx/conf\.d/\*\.conf\s*;', config) and
            re.search(r'listen\s+(?:\[::\]:|0\.0\.0\.0:)?80\s+[^;]*\bdefault_server\b', config)):
        return CONF
    raise ValueError('Nginx has a custom include/default-host layout. No existing website was changed.')


def atomic(path, text, mode=0o644):
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.is_symlink():
        raise ValueError('Refusing a symbolic link in Metric Owl configuration.')
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
        temporary.unlink(missing_ok=True)


def challenge_config(address):
    address = public_ip(address)
    return f'''# Managed by Metric Owl: only this exact IP's ACME challenge, no default_server.
server {{
    listen 80;
    server_name {address};
    if ($host != {address}) {{ return 444; }}
    server_tokens off;
    location ^~ /.well-known/acme-challenge/ {{
        root {WEBROOT};
        default_type text/plain;
        try_files $uri =404;
    }}
    location / {{ return 404; }}
}}
'''


def proxy_config(authority, token):
    public_origin('https://' + authority)
    if not re.fullmatch(r'[a-f0-9]{64}', token):
        raise ValueError('Invalid generated proxy credential.')
    return f'''# Root-readable credential; never copy this file into a repository.
proxy_pass http://127.0.0.1:4877;
proxy_http_version 1.1;
proxy_set_header Host {authority};
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


def tls_config(address, port=8443):
    address = public_ip(address)
    port = panel_port(str(port))
    authority = f'{address}:{port}'
    return f'''limit_req_zone $binary_remote_addr zone=metricowl_panel_login:1m rate=6r/m;
limit_conn_zone $binary_remote_addr zone=metricowl_panel_connections:1m;
{challenge_config(address)}
server {{
    listen {port} ssl http2;
    server_name {address};
    if ($http_host != {authority}) {{ return 444; }}
    ssl_certificate {ACME}/live/metricowl-ip/fullchain.pem;
    ssl_certificate_key {ACME}/live/metricowl-ip/privkey.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_session_tickets off;
    server_tokens off;
    client_max_body_size 64k;
    client_body_timeout 15s;
    limit_conn metricowl_panel_connections 32;
    limit_req_status 429;
    access_log /var/log/nginx/metricowl-panel-access.log;
    error_log /var/log/nginx/metricowl-panel-error.log warn;
    add_header X-Content-Type-Options nosniff always;
    add_header X-Frame-Options DENY always;
    add_header Referrer-Policy no-referrer always;
    location = / {{ return 404; }}
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


def acme_args():
    return ['--config-dir', str(ACME), '--work-dir', str(ACME_WORK), '--logs-dir', str(ACME_LOGS)]


def renewal_service():
    return f'''[Unit]
Description=Renew Metric Owl IP certificate
After=network-online.target nginx.service
Wants=network-online.target
[Service]
Type=oneshot
UMask=0077
ExecStart={CERTBOT} renew {' '.join(acme_args())} --quiet --no-random-sleep-on-renew --deploy-hook {RENEWAL}
'''


def renewal_timer():
    return '''[Unit]
Description=Check Metric Owl IP certificate twice daily
[Timer]
OnCalendar=*-*-* 00,12:00:00
RandomizedDelaySec=1800
Persistent=true
[Install]
WantedBy=timers.target
'''


def ensure_prerequisites():
    # A private environment leaves any system Certbot and all other certificates untouched.
    subprocess.run(['apt-get', 'update'], check=True)
    subprocess.run(['apt-get', 'install', '-y', '--no-install-recommends', 'python3-venv'],
                   check=True, env={**os.environ, 'DEBIAN_FRONTEND': 'noninteractive', 'NEEDRESTART_MODE': 'l'})
    if CERTBOT.is_symlink():
        raise ValueError('The isolated certificate client must not be a symbolic link.')
    if not CERTBOT.exists() or run([str(CERTBOT), '--version']).stdout.strip() != 'certbot ' + CERTBOT_VERSION:
        environment = CERTBOT.parent.parent
        if environment.is_symlink():
            raise ValueError('Unsafe certificate client directory.')
        subprocess.run(['python3', '-m', 'venv', str(environment)], check=True)
        subprocess.run([str(environment / 'bin/python'), '-m', 'pip', 'install', '--disable-pip-version-check',
                        '--index-url', 'https://pypi.org/simple', 'certbot==' + CERTBOT_VERSION], check=True)
    if not shutil.which('nginx'):
        subprocess.run(['apt-get', 'install', '-y', '--no-install-recommends', 'nginx'],
                       check=True, env={**os.environ, 'DEBIAN_FRONTEND': 'noninteractive', 'NEEDRESTART_MODE': 'l'})


def ufw_owned_ports():
    if not shutil.which('ufw'):
        return []
    output = run(['ufw', 'show', 'added']).stdout
    ports = []
    for line in output.splitlines():
        try:
            tokens = shlex.split(line)
        except ValueError:
            continue
        if (len(tokens) == 5 and tokens[:2] == ['ufw', 'allow'] and tokens[3:] == ['comment', FIREWALL_TAG]
                and re.fullmatch(r'[1-9][0-9]{0,4}/tcp', tokens[2])):
            ports.append(int(tokens[2].split('/')[0]))
    return ports


def open_firewall(port, owned=None):
    # The caller retains this same list if a later UFW command fails.
    created = owned if owned is not None else []
    if not shutil.which('ufw'):
        return created
    status = run(['ufw', 'status']).stdout
    if not re.search(r'^Status: active\s*$', status, re.M):
        return created
    try:
        for number in (80, port):
            if re.search(r'^' + str(number) + r'(?:/tcp)?\s+(?:DENY|REJECT)(?: IN)?\s+', status, re.M):
                raise ValueError(f'UFW explicitly blocks TCP {number}. That rule was preserved; '
                                 'review it before running the installer again.')
            # Do not alter an existing global allow rule or its comment.
            if re.search(r'^' + str(number) + r'(?:/tcp)?\s+ALLOW(?: IN)?\s+Anywhere\s*(?:#.*)?$', status, re.M):
                continue
            if number in ufw_owned_ports():
                raise ValueError('An unrecorded Metric Owl firewall rule needs operator review.')
            run(['ufw', 'allow', str(number) + '/tcp', 'comment', FIREWALL_TAG])
            created.append(number)
        return created
    except Exception:
        if owned is None:
            close_firewall(created)
        raise


def close_firewall(ports):
    failed = []
    for number in reversed(ports):
        try:
            if isinstance(number, int) and 1 <= number <= 65535 and number in ufw_owned_ports():
                run(['ufw', '--force', 'delete', 'allow', str(number) + '/tcp'])
        except Exception:
            failed.append(number)
    if failed:
        raise RuntimeError('Could not remove owned firewall entries for TCP ports: ' + ', '.join(map(str, failed)))


def cleanup_step(errors, label, operation):
    try:
        operation()
        return True
    except Exception as error:
        # Exception strings may include private process output. Record labels/types only.
        errors.append({'step': label, 'error_type': type(error).__name__})
        return False


def recovery_path():
    return MARKER.with_name('https-recovery.json')


def save_recovery(operation, ownership, errors, **extra):
    path = recovery_path()
    record = {'operation': operation, 'ownership': ownership, 'cleanup_failures': errors, **extra}
    try:
        atomic(path, json.dumps(record, indent=2) + '\n', 0o600)
        return 'Recovery details were retained at ' + str(path) + '.'
    except Exception:
        return ('Recovery details could not be saved. Review Metric Owl configuration files and owned UFW '
                'rules before retrying. Known owned TCP ports: ' +
                ', '.join(map(str, ownership.get('firewall_ports', []))) + '.')


def verify_panel(authority):
    public_origin('https://' + authority)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    for _ in range(10):
        try:
            with opener.open('https://' + authority + '/api/v1/auth/status', timeout=3) as response:
                data = json.loads(response.read(4096))
                if response.status == 200 and isinstance(data.get('configured'), bool):
                    return
        except (OSError, ValueError, urllib.error.URLError):
            pass
        time.sleep(0.5)
    raise RuntimeError('The trusted HTTPS panel could not be reached at https://' + authority +
                       '/admin. Allow inbound TCP 80 and the panel port in your hosting provider firewall '
                       'as well as on the server. No certificate-warning bypass or insecure login was enabled.')


def configure():
    if os.geteuid() != 0:
        raise ValueError('HTTPS installation requires root privileges.')
    if recovery_path().exists() or recovery_path().is_symlink():
        raise ValueError('An earlier HTTPS operation needs recovery. Review ' + str(recovery_path()) +
                         ' before retrying; no new configuration was created.')
    origin = existing_origin()
    if origin:
        verify_panel(origin[8:])
        print('Preserved existing HTTPS configuration: ' + origin + '/admin')
        return
    for path in (CONF, SITES_CONF, PROXY, TOKEN, DROPIN, RENEWAL, RENEW_SERVICE, RENEW_TIMER):
        if path.exists() or path.is_symlink():
            raise ValueError('Existing Metric Owl HTTPS files need review: ' + str(path) + '. Nothing was overwritten.')
    supplied_ip = os.environ.get('GUARD_PUBLIC_IP')
    address = public_ip(supplied_ip or prompt('Your server’s public IPv4 address', discover_public_ip()))
    port = choose_panel_port()
    authority = f'{address}:{port}'
    listeners = run(['ss', '-H', '-ltnp', 'sport', '=', ':80']).stdout.strip()
    if listeners and any('"nginx"' not in line for line in listeners.splitlines()):
        raise ValueError('Another web server owns port 80. It was left unchanged. '
                         'This preview needs Nginx or an operator-reviewed ACME challenge route; it will not stop your websites.')
    nginx_was_active = run(['systemctl', 'is-active', '--quiet', 'nginx'], allowed=(0, 3, 4)).returncode == 0
    nginx_was_enabled = run(['systemctl', 'is-enabled', '--quiet', 'nginx'], allowed=(0, 1, 3, 4)).returncode == 0
    conf = SITES_CONF
    if shutil.which('nginx'):
        existing = run(['nginx', '-T']).stdout
        if host_collision(existing, address):
            raise ValueError('An existing website explicitly serves this IP. Its configuration was preserved; '
                             'an operator must add the ACME challenge route.')
        conf = select_conf(existing)
    email = os.environ.get('GUARD_HTTPS_EMAIL', '')
    if email and not re.fullmatch(r'[^\s@]+@[^\s@]+\.[^\s@]+', email):
        raise ValueError('GUARD_HTTPS_EMAIL must be a valid certificate contact email.')
    if os.environ.get('GUARD_HTTPS_AGREE_TOS') != '1':
        print('A free HTTPS certificate for your IP is issued by Let’s Encrypt. Terms: ' + TOS)
        if prompt('Accept the certificate authority terms? (yes/no)', 'no').lower() != 'yes':
            raise ValueError('Certificate terms were not accepted. No HTTPS configuration was changed.')
    print(f'Preparing HTTPS at https://{authority}/admin. Port 80 is used only for certificate validation.', flush=True)
    created = []
    firewall = []
    app_changed = False
    timer_started = False
    try:
        ensure_prerequisites()
        for path in (WEBROOT, WEBROOT / '.well-known', WEBROOT / '.well-known/acme-challenge'):
            if path.is_symlink():
                raise ValueError('Unsafe ACME challenge directory.')
            path.mkdir(parents=True, exist_ok=True)
            path.chmod(0o755)
        atomic(conf, challenge_config(address))
        created.append(conf)
        run(['nginx', '-t'])
        run(['systemctl', 'enable', '--now', 'nginx'])
        run(['systemctl', 'reload', 'nginx'])
        firewall = open_firewall(port, firewall)
        print('Requesting the IP certificate. Your provider must allow inbound TCP 80…', flush=True)
        args = [str(CERTBOT), 'certonly', *acme_args(), '--webroot', '--webroot-path', str(WEBROOT),
                '--ip-address', address, '--preferred-profile', 'shortlived', '--cert-name', 'metricowl-ip',
                '--non-interactive', '--agree-tos', '--keep-until-expiring']
        args += ['--email', email] if email else ['--register-unsafely-without-email']
        try:
            run(args)
        except RuntimeError:
            raise RuntimeError('IP certificate validation failed. Check that this is the correct public IP '
                               'and inbound TCP 80 is allowed in your hosting provider firewall. '
                               'Certificate diagnostics are in ' + str(ACME_LOGS) + '. No insecure login was enabled.') from None
        token = secrets.token_hex(32)
        atomic(TOKEN, token + '\n', 0o600)
        created.append(TOKEN)
        atomic(PROXY, proxy_config(authority, token), 0o600)
        created.append(PROXY)
        token = ''
        atomic(DROPIN, '[Service]\nEnvironment=GUARD_PUBLIC_ORIGIN=https://' + authority +
               '\nLoadCredential=metricowl-proxy-token:' + str(TOKEN) +
               '\nEnvironment=GUARD_PROXY_TOKEN_FILE=%d/metricowl-proxy-token\n')
        created.append(DROPIN)
        run(['systemctl', 'daemon-reload'])
        app_changed = True
        run(['systemctl', 'restart', 'server-guard'])
        atomic(conf, tls_config(address, port))
        run(['nginx', '-t'])
        run(['systemctl', 'reload', 'nginx'])
        verify_panel(authority)
        atomic(RENEWAL, '#!/bin/sh\nset -eu\n[ "${RENEWED_LINEAGE:-}" = "' + str(ACME) +
               '/live/metricowl-ip" ] || exit 0\n/usr/sbin/nginx -t >/dev/null 2>&1\n/bin/systemctl reload nginx\n', 0o755)
        created.append(RENEWAL)
        run(['sh', '-n', str(RENEWAL)])
        atomic(RENEW_SERVICE, renewal_service())
        created.append(RENEW_SERVICE)
        atomic(RENEW_TIMER, renewal_timer())
        created.append(RENEW_TIMER)
        run(['systemctl', 'daemon-reload'])
        timer_started = True
        run(['systemctl', 'enable', '--now', RENEW_TIMER.name])
        run(['systemctl', 'is-active', '--quiet', RENEW_TIMER.name])
        run(['systemctl', 'start', RENEW_SERVICE.name])
        atomic(MARKER, json.dumps({'public_origin': 'https://' + authority, 'managed': 'installer',
                                  'schema': 2, 'nginx_conf': str(conf), 'firewall_ports': firewall,
                                  'address': address, 'port': port}, indent=2) + '\n')
        created.append(MARKER)
        print('HTTPS is ready. Open https://' + authority + '/admin from any computer or phone.')
        print('Certificate renewal is automatic. Keep TCP 80 reachable for future renewal.')
    except Exception:
        errors = []
        ownership = {'public_origin': 'https://' + authority, 'nginx_conf': str(conf),
                     'created_paths': [str(path) for path in created], 'firewall_ports': firewall,
                     'nginx_was_active': nginx_was_active, 'nginx_was_enabled': nginx_was_enabled}
        if timer_started:
            cleanup_step(errors, 'disable renewal timer', lambda: run(
                ['systemctl', 'disable', '--now', RENEW_TIMER.name], allowed=(0, 1, 5)))
        for path in reversed(created):
            cleanup_step(errors, 'remove ' + str(path), lambda path=path: path.unlink(missing_ok=True))
        cleanup_step(errors, 'reload systemd', lambda: run(['systemctl', 'daemon-reload']))
        if app_changed:
            cleanup_step(errors, 'restart private application', lambda: run(['systemctl', 'restart', 'server-guard']))
        cleanup_step(errors, 'remove owned firewall rules', lambda: close_firewall(firewall))
        if shutil.which('nginx'):
            valid = cleanup_step(errors, 'validate restored nginx', lambda: run(['nginx', '-t']))
            # Reload only a valid configuration. Stopping a newly installed nginx remains independent.
            if valid or not nginx_was_active:
                cleanup_step(errors, 'restore nginx activity', lambda: run(
                    ['systemctl', 'reload' if nginx_was_active else 'stop', 'nginx']))
            if not nginx_was_enabled:
                cleanup_step(errors, 'restore nginx enablement', lambda: run(['systemctl', 'disable', 'nginx']))
        if errors:
            location = save_recovery('installation rollback', ownership, errors)
            raise RuntimeError('HTTPS setup failed and automatic cleanup was incomplete. ' + location +
                               ' All independent cleanup steps were attempted.') from None
        print('This attempt’s HTTPS and owned firewall entries were rolled back. Existing websites were not edited. '
              'The private certificate environment is retained so retrying can reuse its certificate.', file=sys.stderr)
        raise


def uninstall_https():
    if os.geteuid() != 0:
        raise ValueError('Removing HTTPS requires root privileges.')
    if not MARKER.exists():
        if recovery_path().exists() or recovery_path().is_symlink():
            raise ValueError('An earlier HTTPS cleanup needs review: ' + str(recovery_path()))
        print('No installer-managed HTTPS configuration found.')
        return
    data = marker_data()
    if data.get('managed') != 'installer' or data.get('schema') != 2:
        raise ValueError('This HTTPS configuration is operator-managed or from an earlier installer; it was preserved.')
    conf = Path(data.get('nginx_conf', ''))
    if conf not in (CONF, SITES_CONF):
        raise ValueError('Unexpected HTTPS configuration path; removal stopped.')
    firewall = data.get('firewall_ports', [])
    if not isinstance(firewall, list) or any(type(p) is not int or p not in (80, data.get('port')) for p in firewall):
        raise ValueError('Invalid owned firewall records; removal stopped.')
    for path in (conf, PROXY, TOKEN, DROPIN, RENEWAL, RENEW_SERVICE, RENEW_TIMER, MARKER):
        if path.is_symlink():
            raise ValueError('Unexpected symlink; removal stopped.')
    original_conf = conf.read_text() if conf.exists() else None
    errors = []
    try:
        conf.unlink(missing_ok=True)
        run(['nginx', '-t'])
        run(['systemctl', 'reload', 'nginx'])
    except Exception:
        if original_conf is not None:
            restored = cleanup_step(errors, 'restore original nginx file', lambda: atomic(conf, original_conf))
            if restored:
                restored = cleanup_step(errors, 'validate original nginx', lambda: run(['nginx', '-t']))
            if restored:
                cleanup_step(errors, 'reload original nginx', lambda: run(['systemctl', 'reload', 'nginx']))
        else:
            errors.append({'step': 'validate detached nginx', 'error_type': 'RuntimeError'})
        if errors:
            location = save_recovery('uninstall rollback', data, errors, nginx_conf_backup=original_conf)
            raise RuntimeError('HTTPS removal and restoration could not be completed. ' + location) from None
        raise RuntimeError('HTTPS removal could not be validated. The original panel configuration was restored; '
                           'ownership records and firewall rules were retained.') from None
    cleanup_step(errors, 'disable renewal timer', lambda: run(
        ['systemctl', 'disable', '--now', RENEW_TIMER.name], allowed=(0, 1, 5)))
    cleanup_step(errors, 'stop renewal service', lambda: run(
        ['systemctl', 'stop', RENEW_SERVICE.name], allowed=(0, 1, 5)))
    for path in (PROXY, TOKEN, DROPIN, RENEWAL, RENEW_SERVICE, RENEW_TIMER):
        cleanup_step(errors, 'remove ' + str(path), lambda path=path: path.unlink(missing_ok=True))
    cleanup_step(errors, 'remove owned firewall rules', lambda: close_firewall(firewall))
    cleanup_step(errors, 'reload systemd', lambda: run(['systemctl', 'daemon-reload']))
    if errors:
        location = save_recovery('uninstall', data, errors)
        raise RuntimeError('HTTPS removal was incomplete. The ownership marker was retained for retry. ' + location) from None
    # The marker is the durable ownership record and is deliberately removed last.
    MARKER.unlink(missing_ok=True)
    recovery_path().unlink(missing_ok=True)
    print('Metric Owl HTTPS and its owned firewall entries were removed. Existing websites are unchanged.')
    print('Certificate/account files were preserved in ' + str(ACME) + ' for safe reinstallation.')


if __name__ == '__main__':
    try:
        if sys.argv[1:] == ['--prepare-admin']:
            prepare_administrator()
        elif sys.argv[1:] == ['--uninstall']:
            uninstall_https()
        elif not sys.argv[1:]:
            configure()
        else:
            raise ValueError('Usage: configure-https.py [--prepare-admin|--uninstall]')
    except (ValueError, RuntimeError) as error:
        print(str(error), file=sys.stderr)
        raise SystemExit(1) from None
    except Exception as error:
        print('HTTPS setup failed (' + type(error).__name__ + '). Private details were not logged.', file=sys.stderr)
        raise SystemExit(1) from None
