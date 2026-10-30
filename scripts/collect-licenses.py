#!/usr/bin/env python3
import pathlib,json,subprocess,shutil
root=pathlib.Path(__file__).resolve().parents[1];out=root/'artifacts/release/third-party-licenses';out.mkdir(parents=True,exist_ok=True)
metadata=json.loads(subprocess.check_output(['cargo','metadata','--locked','--format-version','1'],cwd=root))
packages=[(p['name']+'-'+p['version'],pathlib.Path(p['manifest_path']).parent)for p in metadata['packages']if p.get('source')]
licenses=json.loads(subprocess.check_output(['pnpm','--dir','web','licenses','list','--json'],cwd=root))
for group in licenses.values():
 for p in group:
  for path in p.get('paths',[]):packages.append((p['name'].replace('/','_')+'-'+p['versions'][0],pathlib.Path(path)))
for name,path in packages:
 for f in path.iterdir():
  if f.is_file()and f.name.upper().startswith(('LICENSE','LICENCE','COPYING','NOTICE','OFL')):
   destination=out/name;destination.mkdir(exist_ok=True);shutil.copyfile(f,destination/f.name)
shutil.copyfile(root/'docs/DEPENDENCIES.md',out/'DEPENDENCIES.md')
# Remove developer-specific absolute paths from the distributable inventory.
for group in licenses.values():
 for p in group:p.pop('paths',None)
(out/'javascript-licenses.json').write_text(json.dumps(licenses,ensure_ascii=False,indent=2))
print(f'{len(list(out.iterdir()))} dependency license records packaged.')
