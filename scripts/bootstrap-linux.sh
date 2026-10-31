#!/usr/bin/env bash
# One command on the server: curl -fsSL https://metricowl.cloud/install.sh | sudo bash
# GitHub release and script versions stay pinned. Never bypass checksum validation.
set -euo pipefail
set +x
umask 077
[ "$(id -u)" = 0 ] || { printf 'Run this installer with sudo or as root.\n' >&2; exit 1; }
[ "$(uname -s)" = Linux ] || { printf 'This installer requires Linux.\n' >&2; exit 1; }
[[ "$(uname -m)" == x86_64 ]] || { printf 'This preview has only been validated on Ubuntu 24.04 x86-64.\nOther architectures do not have a published supported binary yet.\n' >&2; exit 1; }
architecture=amd64
[[ -r /etc/os-release ]] || { printf 'Cannot identify this Linux distribution.\n' >&2; exit 1; }
source /etc/os-release
[[ "${ID:-}" == ubuntu && "${VERSION_ID:-}" == 24.04 ]] || {
  printf 'This preview installer supports Ubuntu 24.04 x86-64 only.\nDebian and other Linux releases are not yet validated for this binary.\n' >&2; exit 1;
}
release_url="${GUARD_RELEASE_URL:-https://github.com/SpeakQuery/metric-owl/releases/download/v0.1.0-preview.3}"
[[ "$release_url" =~ ^https://github\.com/SpeakQuery/metric-owl/releases/download/v[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$ ]] || {
  printf 'GUARD_RELEASE_URL must identify an official, versioned Metric Owl GitHub release.\n' >&2; exit 1;
}
if ! command -v python3 >/dev/null || ! command -v curl >/dev/null || ! command -v ss >/dev/null || ! command -v runuser >/dev/null || ! dpkg-query -W -f='${Status}' ca-certificates 2>/dev/null | grep -q 'install ok installed'; then
  printf 'Installing the small set of download prerequisites…\n'
  apt-get update
  DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends python3 curl ca-certificates iproute2 util-linux
fi
for tool in curl sha256sum systemctl install useradd awk; do
  command -v "$tool" >/dev/null || { printf 'Required command is missing: %s\n' "$tool" >&2; exit 1; }
done
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/scripts" "$stage/packaging/systemd" "$stage/release"
download() {
  curl --fail --show-error --silent --location --proto '=https' --proto-redir '=https' \
    --connect-timeout 15 --max-time 300 --retry 2 "$release_url/$1" --output "$stage/$1"
}
download SHA256SUMS || {
  printf '\nThe selected GitHub release cannot be downloaded. No application files were changed.\nA private or unpublished preview needs authorized access; this installer does not expose private assets.\n' >&2
  exit 1
}
binary="server-guard-linux-$architecture"
for asset in "$binary" install-linux.sh show-setup.sh connection-hints.sh configure-https.py server-guard.service LICENSE BINARY-LICENSE.txt THIRD-PARTY-NOTICES.txt release-metadata.json; do
  download "$asset"
  digest="$(awk -v filename="$asset" '$2==filename { print $1; found++ } END { if(found!=1) exit 1 }' "$stage/SHA256SUMS")" || {
    printf 'Missing or duplicate checksum for %s.\n' "$asset" >&2; exit 1;
  }
  [[ "$digest" =~ ^[a-fA-F0-9]{64}$ ]] || { printf 'Invalid release checksum.\n' >&2; exit 1; }
  (cd "$stage" && printf '%s  %s\n' "$digest" "$asset" | sha256sum -c -)
done
python3 - "$stage/release-metadata.json" "${release_url##*/}" <<'PY'
import json,re,sys
m=json.load(open(sys.argv[1]))
if (m.get('version')!=sys.argv[2] or not re.fullmatch(r'[a-f0-9]{40}',m.get('source_commit',''))
    or m.get('os')!='ubuntu' or m.get('os_version')!='24.04' or m.get('architecture')!='x86_64'):
 raise SystemExit('Release provenance or platform metadata does not match this installation.')
PY
mv "$stage/$binary" "$stage/release/server-guard"
mv "$stage/install-linux.sh" "$stage/show-setup.sh" "$stage/connection-hints.sh" "$stage/configure-https.py" "$stage/scripts/"
mv "$stage/server-guard.service" "$stage/packaging/systemd/"
mv "$stage/LICENSE" "$stage/BINARY-LICENSE.txt" "$stage/THIRD-PARTY-NOTICES.txt" "$stage/release/"
(cd "$stage/release" && sha256sum server-guard > SHA256SUMS)
GUARD_INSTALL_QUIET=1 bash "$stage/scripts/install-linux.sh" "$stage/release"
if [[ "${GUARD_CONFIGURE_HTTPS:-1}" != 0 ]]; then
  if ! python3 "$stage/scripts/configure-https.py"; then
    printf '\nThe monitoring application is installed privately, but HTTPS setup is incomplete.\nCorrect the reported domain/configuration issue and run this same installation command again.\nYour account, settings and history are retained.\n' >&2
    exit 1
  fi
fi
if [[ "${GUARD_PREPARE_ADMIN:-1}" != 0 ]]; then
  python3 "$stage/scripts/configure-https.py" --prepare-admin
fi
bash "$stage/scripts/show-setup.sh" /var/lib/server-guard ssh 4877 "$(systemctl show server-guard -p MainPID --value)"
