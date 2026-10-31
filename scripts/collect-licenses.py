#!/usr/bin/env python3
"""Collect original Cargo and shipped JavaScript dependency notices.

Pinned supplements cover upstream archives that omit their license files and code
bundled inside another package. Unresolved Cargo/runtime notices fail collection.
Development-only JavaScript metadata and known gaps are reported separately.
No network downloads occur in this step.
"""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / 'artifacts/release/third-party-licenses'
SUPPLEMENTS = ROOT / 'docs/licenses'
NOTICE_PREFIXES = ('LICENSE', 'LICENCE', 'COPYING', 'COPYRIGHT', 'NOTICE', 'OFL')


def copy_package_notices(name, package, destination, declared=None):
    """Honor Cargo's license_file even when its name is not LICENSE/COPYING."""
    files = [p for p in package.iterdir()
             if p.is_file() and p.name.upper().startswith(NOTICE_PREFIXES)]
    if declared:
        path = Path(declared)
        path = path if path.is_absolute() else package / path
        if not path.is_file():
            raise ValueError(name + ': declared license_file is missing')
        files.append(path)
    copied = []
    for path in sorted(set(files)):
        data = path.read_bytes()
        if not data.strip():
            continue
        target = destination / name / path.name
        target.parent.mkdir(parents=True, exist_ok=True)
        if target.exists() and target.read_bytes() != data:
            raise ValueError(name + ': conflicting notice filenames')
        target.write_bytes(data)
        copied.append(path.name)
    return copied


def apply_supplements(destination, package_names):
    manifest = json.loads((SUPPLEMENTS / 'manifest.json').read_text())
    applied = []
    for record in manifest['supplements']:
        matches = package_names.intersection(record.get('packages', []))
        if not matches and record.get('kind') != 'runtime':
            continue
        targets = matches if record.get('kind', 'cargo') in ('cargo', 'package') else {record['name']}
        for entry in record['files']:
            source = (SUPPLEMENTS / entry['path']).resolve()
            if not source.is_relative_to(SUPPLEMENTS.resolve()) or not source.is_file():
                raise ValueError('Invalid supplemental notice path')
            data = source.read_bytes()
            if hashlib.sha256(data).hexdigest() != entry['sha256']:
                raise ValueError('Supplemental notice checksum mismatch: ' + entry['path'])
            for name in targets:
                target = destination / name / Path(entry['path']).name
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(data)
        applied.append(record)
    (destination / 'supplemental-sources.json').write_text(json.dumps(applied, indent=2) + '\n')


def main():
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--locked', '--format-version', '1'], cwd=ROOT))
    all_javascript = json.loads(subprocess.check_output(
        ['pnpm', '--dir', 'web', 'licenses', 'list', '--json'], cwd=ROOT))
    javascript = json.loads(subprocess.check_output(
        ['pnpm', '--dir', 'web', 'licenses', 'list', '--prod', '--json'], cwd=ROOT))
    runtime_names = {p['name'] for group in javascript.values() for p in group}
    development_gaps = []
    for group in all_javascript.values():
        for package in group:
            if (package['name'] == 'stackback' and '0.0.2' in package['versions']
                    and package['name'] not in runtime_names):
                development_gaps.append({
                    'package': 'stackback', 'version': '0.0.2', 'declared_license': 'MIT',
                    'shipped_runtime': False,
                    'reason': 'The package and upstream release declare MIT but omit a full license notice file. '
                              'No copyright attribution was invented.',
                    'source_url': 'https://github.com/defunctzombie/node-stackback/tree/2963095372abf7b75ba55f01cef08ab1e62c2ff4'})
    packages = {}
    for package in metadata['packages']:
        if package.get('source'):
            name = package['name'] + '-' + package['version']
            packages[name] = (Path(package['manifest_path']).parent, package.get('license_file'))
    for group in javascript.values():
        for package in group:
            for location in package.get('paths', []):
                path = Path(location)
                info = json.loads((path / 'package.json').read_text())
                name = info['name'].replace('/', '_') + '-' + info['version']
                packages[name] = (path, None)
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    report_path = OUTPUT.parent / 'license-collection-report.json'
    report_path.write_text(json.dumps({'complete': False, 'phase': 'collecting'}, indent=2) + '\n')
    with tempfile.TemporaryDirectory(prefix='.licenses-', dir=OUTPUT.parent) as temporary:
        staging = Path(temporary) / 'notices'
        staging.mkdir()
        for name, (path, declared) in sorted(packages.items()):
            copy_package_notices(name, path, staging, declared)
        apply_supplements(staging, set(packages))
        unresolved = [name for name in sorted(packages)
                      if not (staging / name).is_dir() or not any((staging / name).iterdir())]
        report = {'complete': not unresolved, 'resolved_packages': len(packages),
                  'unresolved': unresolved,
                  'cargo_scope': 'All locked Cargo packages, including development and platform-conditional packages.',
                  'javascript_scope': 'Production dependencies only; build/test tool executables are not shipped.',
                  'unshipped_development_notice_gaps': development_gaps,
                  'scope': 'Original notice files and pinned supplements; not a legal compliance certification.'}
        report_path.write_text(json.dumps(report, indent=2) + '\n')
        if unresolved:
            raise SystemExit('Unresolved dependency notices: ' + ', '.join(unresolved))
        shutil.copyfile(ROOT / 'docs/DEPENDENCIES.md', staging / 'DEPENDENCIES.md')
        for inventory in (javascript, all_javascript):
            for group in inventory.values():
                for package in group:
                    package.pop('paths', None)
        for filename, inventory in [('javascript-licenses.json', all_javascript),
                                    ('javascript-runtime-licenses.json', javascript),
                                    ('development-notice-gaps.json', development_gaps)]:
            (staging / filename).write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + '\n')
        # Replace generated notices only after a complete successful collection.
        # This removes stale files from packages no longer in the lockfiles.
        if OUTPUT.exists():
            shutil.rmtree(OUTPUT)
        shutil.move(str(staging), OUTPUT)
    print(json.dumps(report))


if __name__ == '__main__':
    main()
