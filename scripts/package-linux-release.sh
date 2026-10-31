#!/usr/bin/env bash
# Stage a versioned Ubuntu preview. This command never uploads or publishes it.
set -euo pipefail
cd "$(dirname "$0")/.."
[ "$(uname -s)" = Linux ] || { printf 'Build the release on Linux.\n' >&2; exit 1; }
[[ "$(uname -m)" == x86_64 ]] || { printf 'Only the tested x86-64 preview is packaged.\n' >&2; exit 1; }
source /etc/os-release
[[ "${ID:-}" == ubuntu && "${VERSION_ID:-}" == 24.04 ]] || { printf 'This release profile requires Ubuntu 24.04.\n' >&2; exit 1; }
version="${METRIC_OWL_RELEASE_VERSION:-v0.1.0-preview.4}"
[[ "$version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$ ]] || { printf 'Invalid release version.\n' >&2; exit 1; }
source_commit="${METRIC_OWL_SOURCE_COMMIT:-$(git rev-parse HEAD 2>/dev/null || true)}"
[[ "$source_commit" =~ ^[a-f0-9]{40}$ ]] || { printf 'Set METRIC_OWL_SOURCE_COMMIT to the exact reviewed Git commit.\n' >&2; exit 1; }
file artifacts/release/server-guard | grep -q 'ELF 64-bit.*x86-64' || { printf 'An x86-64 static Linux ELF binary is required.\n' >&2; exit 1; }
program_headers="$(readelf -lW artifacts/release/server-guard)"
dynamic_entries="$(readelf -dW artifacts/release/server-guard)"
if grep -q INTERP <<< "$program_headers" || grep -q NEEDED <<< "$dynamic_entries"; then
  printf 'This release requires a static musl binary without an ELF interpreter or dynamic dependencies.\n' >&2
  exit 1
fi
[[ -d artifacts/release/third-party-licenses ]] || { printf 'Collect dependency licenses before packaging.\n' >&2; exit 1; }
destination="artifacts/releases/$version/linux-amd64"
[[ ! -e "$destination" ]] || { printf 'Refusing to overwrite an existing versioned package: %s\n' "$destination" >&2; exit 1; }
mkdir -p "$(dirname "$destination")"
stage="$(mktemp -d "$(dirname "$destination")/.stage.XXXXXX")"
trap 'rm -rf "$stage"' EXIT
cp artifacts/release/server-guard "$stage/server-guard-linux-amd64"
cp scripts/bootstrap-linux.sh scripts/install-linux.sh scripts/uninstall-linux.sh scripts/show-setup.sh scripts/connection-hints.sh scripts/configure-https.py "$stage/"
cp packaging/systemd/server-guard.service LICENSE BINARY-LICENSE.txt "$stage/"
python3 - "$stage" "$version" "$source_commit" <<'PY'
import datetime,json,pathlib,re,subprocess,sys
destination,version,commit=sys.argv[1:]
out=pathlib.Path(destination)
root=pathlib.Path('artifacts/release/third-party-licenses')
licenses=[p for p in sorted(root.rglob('*')) if p.is_file()]
if len(licenses)<10: raise SystemExit('The dependency notice collection is incomplete.')
with (out/'THIRD-PARTY-NOTICES.txt').open('w') as f:
 f.write('Metric Owl third-party component notices\nEach component retains its own license.\n')
 for path in licenses:
  f.write('\n'+'='*72+'\n'+str(path.relative_to(root))+'\n'+'='*72+'\n')
  f.write(path.read_text(errors='replace'))
  f.write('\n')
metadata={'product':'Metric Owl','version':version,'source_commit':commit,'binary':'server-guard-linux-amd64','os':'linux','architecture':'x86_64','libc':'musl','linkage':'static','platforms':['ubuntu-22.04','ubuntu-24.04','ubuntu-26.04','debian-12','debian-13'],'built_at_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'distribution':'public preview','not_validated':['Linux ARM64','Windows'],'license':'BINARY-LICENSE.txt'}
(out/'release-metadata.json').write_text(json.dumps(metadata,indent=2)+'\n')
PY
bundle="$stage/metric-owl-$version-linux-amd64"
mkdir -p "$bundle/scripts" "$bundle/packaging/systemd" "$bundle/release"
cp "$stage/install-linux.sh" "$stage/uninstall-linux.sh" "$stage/show-setup.sh" "$stage/connection-hints.sh" "$stage/configure-https.py" "$bundle/scripts/"
cp "$stage/server-guard.service" "$bundle/packaging/systemd/"
cp "$stage/server-guard-linux-amd64" "$bundle/release/server-guard"
cp "$stage/LICENSE" "$stage/BINARY-LICENSE.txt" "$stage/THIRD-PARTY-NOTICES.txt" "$bundle/release/"
(cd "$bundle/release" && sha256sum server-guard > SHA256SUMS)
tar -czf "$stage/metric-owl-$version-linux-amd64.tar.gz" -C "$stage" "$(basename "$bundle")"
rm -rf "$bundle"
(cd "$stage" && sha256sum server-guard-linux-amd64 bootstrap-linux.sh install-linux.sh uninstall-linux.sh show-setup.sh connection-hints.sh configure-https.py server-guard.service LICENSE BINARY-LICENSE.txt THIRD-PARTY-NOTICES.txt release-metadata.json "metric-owl-$version-linux-amd64.tar.gz" > SHA256SUMS)
mv "$stage" "$destination"
trap - EXIT
printf 'Preview package staged at %s. Nothing has been uploaded or published.\n' "$destination"
