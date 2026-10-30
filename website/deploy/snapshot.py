#!/usr/bin/env python3
"""Read-only inventory of existing web configurations, public certificates and routes."""
import concurrent.futures
import hashlib
import json
import pathlib
import re
import subprocess
import sys

def digest(path):
    try:
        return hashlib.sha256(path.read_bytes()).hexdigest()
    except (OSError, PermissionError):
        return None

files = {}
for root in (pathlib.Path('/etc/nginx'), pathlib.Path('/etc/letsencrypt/renewal')):
    if root.exists():
        for path in sorted(root.rglob('*')):
            if path.is_file() and 'metricowl' not in str(path) and not re.search(r'privkey|private|\.key$', str(path), re.I):
                files[str(path)] = digest(path)
for path in pathlib.Path('/etc/letsencrypt/archive').glob('*/*.pem'):
    if 'metricowl' not in str(path) and not path.name.startswith('privkey'):
        files[str(path)] = digest(path)

names = set()
for path in pathlib.Path('/etc/nginx/sites-enabled').glob('*'):
    if path.is_file() and 'metricowl' not in path.name:
        for row in re.findall(r'^\s*server_name\s+([^;]+);', path.read_text(), re.M):
            for name in row.split():
                if re.fullmatch(r'[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}', name):
                    names.add(name)

def check(name):
    result = {}
    for scheme, port in (('http',80),('https',443)):
        run = subprocess.run(['curl','-sS','--max-time','8','--noproxy','*','--resolve',f'{name}:{port}:127.0.0.1','-o','/dev/null','-w','%{http_code}',f'{scheme}://{name}/'],capture_output=True,text=True)
        result[scheme] = {'status':run.stdout,'exit_code':run.returncode}
    return name,result

with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
    responses = dict(pool.map(check, sorted(names)))
print(json.dumps({'files':files,'routes':responses,'nginx_pid':pathlib.Path('/run/nginx.pid').read_text().strip()},indent=2))
