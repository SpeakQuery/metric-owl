#!/usr/bin/env python3
"""Audit the intended source tree without printing possible secret values."""
import argparse
import json
import pathlib
import re
import subprocess
import sys

ROOT=pathlib.Path(__file__).resolve().parents[1]
ROOT_FILES={'.gitignore','.node-version','Cargo.toml','Cargo.lock','rust-toolchain.toml',
            'README.md','CONTRIBUTING.md','LICENSE','BINARY-LICENSE.txt'}
PREFIXES=('crates/','web/','scripts/','packaging/','.github/','docs/','website/')
EXCLUDED_PREFIXES=('website/deploy/preview/',)
EXCLUDED_FILES={'website/deploy/VERIFIED.md','docs/NEXT_FEATURES.md','docs/COMPETITOR_RESEARCH_2026-10-31.md'}
FORBIDDEN_PARTS={'node_modules','target','dist','artifacts','.local-data','.test-data',
                 '__pycache__','.git','playwright-report','test-results'}
PATTERNS={
 'private_key':re.compile(rb'-----BEGIN (?:RSA |EC |OPENSSH |DSA )?PRIVATE KEY-----'),
 'github_token':re.compile(rb'\b(?:gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,})\b'),
 'provider_key':re.compile(rb'\b(?:sk-proj-[A-Za-z0-9_-]{24,}|AIza[0-9A-Za-z_-]{30,}|xai-[A-Za-z0-9_-]{30,})\b'),
 'aws_access_key':re.compile(rb'\b(?:AKIA|ASIA)[A-Z0-9]{16}\b'),
 'operator_home_path':re.compile(re.escape(str(pathlib.Path.home()).encode())+rb'/(?:Documents|Desktop)/'),
}

def permitted(name):
    path=pathlib.PurePosixPath(name)
    return ((name in ROOT_FILES or name.startswith(PREFIXES))
            and not any(part in FORBIDDEN_PARTS for part in path.parts)
            and name not in EXCLUDED_FILES and not name.startswith(EXCLUDED_PREFIXES)
            and not path.name.startswith('.env')
            and path.suffix.lower() not in {'.sqlite','.db','.key','.pem','.p12','.pfx','.pyc'}
            and 'sqlite' not in path.suffix.lower()
            and path.name not in {'setup-key','relay-smtp-password'})

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--staged',action='store_true',help='Inspect exact staged blobs instead of working files.')
    parser.add_argument('--manifest',type=pathlib.Path,help='Write an allowlisted NUL-delimited path manifest.')
    parser.add_argument('--private-patterns',type=pathlib.Path,help='Ignored local file of additional literal values to forbid; values are never printed.')
    args=parser.parse_args()
    if args.private_patterns:
        for index,value in enumerate(args.private_patterns.read_bytes().splitlines()):
            if value.strip():
                PATTERNS['private_literal_'+str(index+1)]=re.compile(re.escape(value.strip()))
    command=['git','ls-files','-z','--cached']
    if not args.staged:command+=['--others','--exclude-standard']
    names=sorted(set(subprocess.check_output(command,cwd=ROOT).decode().split('\0'))-{''})
    selected=[];excluded=[];findings=[]
    for name in names:
        if not permitted(name):
            excluded.append(name)
            if args.staged:
                findings.append({'path':name,'kind':'excluded_path_in_index'})
            continue
        path=ROOT/name
        if path.is_symlink():
            findings.append({'path':name,'kind':'symlink'});continue
        if not args.staged and not path.is_file():continue
        data=(subprocess.check_output(['git','show',':'+name],cwd=ROOT)
              if args.staged else path.read_bytes())
        if len(data)>20*1024*1024:
            findings.append({'path':name,'kind':'oversized_source_file'});continue
        for kind,pattern in PATTERNS.items():
            for match in pattern.finditer(data):
                findings.append({'path':name,'line':data[:match.start()].count(b'\n')+1,'kind':kind})
        selected.append(name)
    if args.manifest and not findings:
        args.manifest.parent.mkdir(parents=True,exist_ok=True)
        args.manifest.write_bytes(('\0'.join(selected)+'\0').encode())
    result={'passed':not findings,'mode':'staged' if args.staged else 'working-tree',
            'included_files':len(selected),'excluded_files':excluded,'findings':findings,
            'note':'Heuristic secret/path audit plus a publication allowlist; human review remains required.'}
    print(json.dumps(result,indent=2))
    return 0 if not findings else 1

if __name__=='__main__':sys.exit(main())
