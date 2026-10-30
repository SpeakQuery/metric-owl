#!/usr/bin/env python3
"""Install a private GitHub release on an authorized Ubuntu server.

GitHub authentication stays on the operator's computer. Only short-lived asset
URLs cross SSH stdin. They are never printed, saved, or placed in command arguments.
"""
import argparse
import json
import re
import shlex
import subprocess
import urllib.error
import urllib.parse
import urllib.request

ASSETS = ['SHA256SUMS','server-guard-linux-amd64','install-linux.sh','show-setup.sh',
          'connection-hints.sh','server-guard.service','LICENSE','BINARY-LICENSE.txt',
          'THIRD-PARTY-NOTICES.txt','release-metadata.json']
ALLOWED_HOSTS = {'release-assets.githubusercontent.com','objects.githubusercontent.com','objects-origin.githubusercontent.com'}

class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, message, headers, newurl):
        return None

def signed_url(repository, asset_id, token):
    request = urllib.request.Request(f'https://api.github.com/repos/{repository}/releases/assets/{asset_id}',headers={
        'Authorization':f'Bearer {token}','Accept':'application/octet-stream',
        'X-GitHub-Api-Version':'2022-11-28','User-Agent':'Metric-Owl-Private-Installer'})
    try:
        with urllib.request.build_opener(NoRedirect).open(request,timeout=30) as response:
            raise RuntimeError(f'Expected an asset redirect; received HTTP {response.status}.')
    except urllib.error.HTTPError as error:
        if error.code != 302:
            raise RuntimeError(f'GitHub asset authorization failed (HTTP {error.code}).') from None
        value=error.headers.get('Location','')
        parsed=urllib.parse.urlsplit(value)
        if parsed.scheme!='https' or parsed.hostname not in ALLOWED_HOSTS or parsed.username or parsed.password or parsed.port not in (None,443):
            raise RuntimeError('GitHub returned an unexpected asset destination.')
        return value

REMOTE_SCRIPT = r'''
import hashlib,json,os,pathlib,re,shutil,subprocess,sys,tempfile,urllib.parse,urllib.request
ALLOWED={'release-assets.githubusercontent.com','objects.githubusercontent.com','objects-origin.githubusercontent.com'}
class RestrictedRedirect(urllib.request.HTTPRedirectHandler):
 def redirect_request(self,request,fp,code,message,headers,newurl):
  p=urllib.parse.urlsplit(newurl)
  if p.scheme!='https' or p.hostname not in ALLOWED or p.username or p.password or p.port not in (None,443):raise ValueError('Unexpected redirect')
  return super().redirect_request(request,fp,code,message,headers,newurl)
try:
 if os.geteuid()!=0: raise RuntimeError('Connect with an authorized root SSH account for installation.')
 if os.uname().sysname!='Linux' or os.uname().machine!='x86_64':raise RuntimeError('This preview supports Ubuntu 24.04 x86-64 only.')
 os_release={k:v.strip('"') for k,v in (line.strip().split('=',1) for line in pathlib.Path('/etc/os-release').read_text().splitlines() if '=' in line)}
 if os_release.get('ID')!='ubuntu' or os_release.get('VERSION_ID')!='24.04':raise RuntimeError('This binary has only been validated on Ubuntu 24.04.')
 raw=sys.stdin.buffer.read(128*1024+1)
 if len(raw)>128*1024:raise ValueError('Asset map exceeded its size limit.')
 request=json.loads(raw)
 assets=request['assets']
 required={'SHA256SUMS','server-guard-linux-amd64','install-linux.sh','show-setup.sh','connection-hints.sh','server-guard.service','LICENSE','BINARY-LICENSE.txt','THIRD-PARTY-NOTICES.txt','release-metadata.json'}
 if set(assets)!=required:raise ValueError('Unexpected release asset set.')
 with tempfile.TemporaryDirectory(prefix='metric-owl-private-') as directory:
  stage=pathlib.Path(directory)
  os.chmod(stage,0o700)
  opener=urllib.request.build_opener(RestrictedRedirect)
  for name in ['SHA256SUMS']+sorted(required-{'SHA256SUMS'}):
   url=assets[name];parsed=urllib.parse.urlsplit(url)
   if parsed.scheme!='https' or parsed.hostname not in ALLOWED or parsed.username or parsed.password or parsed.port not in (None,443):raise ValueError('Invalid asset origin.')
   limit=150*1024*1024 if name.startswith('server-guard-linux-') else 20*1024*1024
   with opener.open(url,timeout=45) as response,(stage/name).open('wb') as target:
    total=0
    while True:
     block=response.read(128*1024)
     if not block:break
     total+=len(block)
     if total>limit:raise ValueError('Release asset exceeded its size limit.')
     target.write(block)
  sums={}
  for line in (stage/'SHA256SUMS').read_text().splitlines():
   match=re.fullmatch(r'([a-fA-F0-9]{64})  ([A-Za-z0-9_.-]+)',line)
   if not match or match[2] in sums:raise ValueError('Invalid or duplicate release checksum.')
   sums[match[2]]=match[1].lower()
  for name in required-{'SHA256SUMS'}:
   if hashlib.sha256((stage/name).read_bytes()).hexdigest()!=sums.get(name):raise ValueError('Release checksum validation failed.')
  metadata=json.loads((stage/'release-metadata.json').read_text())
  if metadata.get('version')!=request['version'] or metadata.get('source_commit')!=request['commit'] or metadata.get('os')!='ubuntu' or metadata.get('os_version')!='24.04' or metadata.get('architecture')!='x86_64':raise ValueError('Release provenance or platform metadata does not match.')
  for folder in ['scripts','packaging/systemd','release']:(stage/folder).mkdir(parents=True)
  for name in ['install-linux.sh','show-setup.sh','connection-hints.sh']:shutil.move(stage/name,stage/'scripts'/name)
  shutil.move(stage/'server-guard.service',stage/'packaging/systemd/server-guard.service')
  shutil.move(stage/'server-guard-linux-amd64',stage/'release/server-guard')
  for name in ['LICENSE','BINARY-LICENSE.txt','THIRD-PARTY-NOTICES.txt']:shutil.move(stage/name,stage/'release'/name)
  (stage/'release/SHA256SUMS').write_text(sums['server-guard-linux-amd64']+'  server-guard\n')
  print('Private GitHub assets downloaded directly over HTTPS; every required checksum passed.',flush=True)
  print('Release: '+request['version']+' | source commit: '+request['commit'],flush=True)
  subprocess.run(['bash',str(stage/'scripts/install-linux.sh'),str(stage/'release')],check=True)
  print('Private GitHub installation completed. No GitHub token was stored on this server.',flush=True)
except Exception as error:
 # Network exceptions can contain signed URLs. Never print messages or tracebacks.
 print('Private installation failed ('+type(error).__name__+'). No asset URLs or credentials were logged.',file=sys.stderr)
 sys.exit(1)
'''

def gh_json(path):
    return json.loads(subprocess.check_output(['gh','api',path],text=True))

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo',default='SpeakQuery/metric-owl')
    parser.add_argument('--tag',default='v0.1.0-preview.2')
    parser.add_argument('--host',required=True,help='Authorized root SSH target, such as root@YOUR_SERVER_IP')
    parser.add_argument('--port',type=int,default=22)
    parser.add_argument('--control')
    args=parser.parse_args()
    if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+',args.repo) or not re.fullmatch(r'v[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.]+)?',args.tag):
        raise RuntimeError('Invalid repository or version.')
    if not re.fullmatch(r'[A-Za-z0-9_][A-Za-z0-9_.:@\[\]-]*',args.host) or not 0<args.port<65536:
        raise RuntimeError('Invalid SSH target or port.')
    repository=gh_json(f'repos/{args.repo}')
    if repository.get('private') is not True:raise RuntimeError('This helper is reserved for private repositories.')
    release=gh_json(f'repos/{args.repo}/releases/tags/{args.tag}')
    if release.get('draft'):raise RuntimeError('Finish the draft release inside the private repository before testing.')
    commit=gh_json(f'repos/{args.repo}/commits/{args.tag}')['sha']
    matches={name:[asset for asset in release['assets'] if asset['name']==name] for name in ASSETS}
    if any(len(values)!=1 for values in matches.values()):raise RuntimeError('Required release assets are missing or duplicated.')
    token=subprocess.check_output(['gh','auth','token','--hostname','github.com'],text=True).strip()
    urls={name:signed_url(args.repo,values[0]['id'],token) for name,values in matches.items()}
    token=''
    payload=json.dumps({'version':args.tag,'commit':commit,'assets':urls}).encode()
    command=['ssh','-o','BatchMode=yes','-o','ConnectTimeout=10','-p',str(args.port)]
    if args.control:command+=['-S',args.control]
    command += [args.host,'python3 -c '+shlex.quote(REMOTE_SCRIPT)]
    print(f'Installing private release {args.repo}@{args.tag}. GitHub authentication stays on this computer.',flush=True)
    subprocess.run(command,input=payload,check=True)

if __name__=='__main__':
    try:main()
    except Exception as error:
        print('Private-release setup failed ('+type(error).__name__+'). Check GitHub access, release completeness and SSH connectivity.',file=__import__('sys').stderr)
        raise SystemExit(1) from None
