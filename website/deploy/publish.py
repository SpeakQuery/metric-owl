#!/usr/bin/env python3
"""Publish only the reviewed static website/public tree to the isolated website root.

Usage: python3 website/deploy/publish.py --host root@host [--control /path/to/socket]
The server must already have the checked Metric Owl Nginx configuration and TLS.
"""
import argparse
import datetime
import hashlib
import json
import pathlib
import shlex
import subprocess
import tarfile
import tempfile

def allowed_public_file(relative, size_bytes):
    """Keep demo media confined to assets/demo and retain the per-file cap."""
    allowed = {'.html', '.css', '.js', '.svg', '.jpg', '.jpeg', '.png', '.webp', '.woff2', '.txt', '.xml'}
    suffix = relative.suffix.lower()
    demo_media = relative.parts[:2] == ('assets', 'demo') and suffix in {'.mp4', '.gif'}
    return size_bytes <= 20 * 1024 * 1024 and (suffix in allowed or demo_media)


parser = argparse.ArgumentParser()
parser.add_argument('--host', required=True)
parser.add_argument('--control')
args = parser.parse_args()
source = pathlib.Path(__file__).resolve().parents[1] / 'public'
required = ['index.html','404.html','styles.css','site.js','privacy.html','docs/quick-start.html',
            'docs/releases.html','assets/owl.svg','assets/dashboard.jpg','assets/rules.jpg',
            'assets/mcp.jpg','assets/dashboard-light.jpg','assets/social-preview.jpg']
for name in required:
    if not (source / name).is_file():
        raise SystemExit(f'Required public file is missing: {name}')
files = []
for path in sorted(source.rglob('*')):
    relative = path.relative_to(source)
    if path.is_symlink() or any(part.startswith('.') for part in relative.parts):
        raise SystemExit(f'Unsafe public path: {relative}')
    if path.is_file():
        if not allowed_public_file(relative, path.stat().st_size):
            raise SystemExit(f'Unexpected public file: {relative}')
        files.append(path)
manifest = {str(p.relative_to(source)):hashlib.sha256(p.read_bytes()).hexdigest() for p in files}
digest = hashlib.sha256(json.dumps(manifest,sort_keys=True).encode()).hexdigest()[:12]
release = 'web-' + datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%SZ') + '-' + digest
ssh = ['ssh','-o','BatchMode=yes','-o','ConnectTimeout=10']
if args.control:
    ssh += ['-S',args.control]

def remote(script, **kwargs):
    return subprocess.run(ssh+[args.host,script],check=True,**kwargs)

with tempfile.TemporaryDirectory(prefix='metricowl-publish-') as temporary:
    archive = pathlib.Path(temporary)/'public.tar.gz'
    with tarfile.open(archive,'w:gz',format=tarfile.PAX_FORMAT) as tar:
        for path in files:
            info = tar.gettarinfo(str(path),arcname=str(path.relative_to(source)))
            info.uid = info.gid = 0
            info.uname = info.gname = 'root'
            info.mode = 0o644
            with path.open('rb') as content:
                tar.addfile(info,content)
    directory = '/var/www/metricowl/releases/' + release
    quoted = shlex.quote(directory)
    with archive.open('rb') as stream:
        remote(f'set -eu; test -f /etc/nginx/sites-enabled/metricowl.cloud; test -f /etc/letsencrypt/live/metricowl.cloud/fullchain.pem; test ! -e {quoted}; install -d -m 0755 {quoted}; tar --no-same-owner -xzf - -C {quoted}; find {quoted} -type d -exec chmod 0755 {{}} +; find {quoted} -type f -exec chmod 0644 {{}} +',stdin=stream)
    previous = remote('readlink -f /var/www/metricowl/current',capture_output=True,text=True).stdout.strip()
    if not previous.startswith('/var/www/metricowl/releases/'):
        raise SystemExit('Current release is outside the expected directory; activation stopped.')
    expected_index = manifest['index.html']
    activate = f'''set -eu
ln -s {quoted} /var/www/metricowl/current.next
mv -Tf /var/www/metricowl/current.next /var/www/metricowl/current
test "$(curl -sS -o /dev/null -w '%{{http_code}}' --max-time 15 --resolve metricowl.cloud:443:127.0.0.1 https://metricowl.cloud/)" = 200
test "$(curl -fsS --max-time 15 --resolve metricowl.cloud:443:127.0.0.1 https://metricowl.cloud/ | sha256sum | cut -d ' ' -f 1)" = {shlex.quote(expected_index)}
'''
    try:
        remote(activate)
    except subprocess.CalledProcessError:
        remote(f'ln -sfn {shlex.quote(previous)} /var/www/metricowl/current.rollback; mv -Tf /var/www/metricowl/current.rollback /var/www/metricowl/current')
        raise
print(json.dumps({'release':directory,'previous_release':previous,'file_count':len(files),'manifest':manifest},indent=2))
