#!/usr/bin/env bash
# One command on the server: curl -fsSL https://raw.githubusercontent.com/SpeakQuery/metric-owl/v0.1.0-preview.8/scripts/bootstrap-linux.sh | sudo bash
# GitHub release and script versions stay pinned. Never bypass checksum validation.
set -euo pipefail
set +x
umask 077
[ "$(id -u)" = 0 ] || { printf 'Run this installer with sudo or as root.\n' >&2; exit 1; }
[ "$(uname -s)" = Linux ] || { printf 'This installer requires Linux.\n' >&2; exit 1; }
[[ "$(uname -m)" == x86_64 ]] || { printf 'This preview provides an x86-64 Linux binary.\nOther architectures do not have a published supported binary yet.\n' >&2; exit 1; }
architecture=amd64
[[ -r /etc/os-release ]] || { printf 'Cannot identify this Linux distribution.\n' >&2; exit 1; }
source /etc/os-release
case "${ID:-}:${VERSION_ID:-}" in
  ubuntu:22.04|ubuntu:24.04|ubuntu:26.04|debian:12|debian:13) ;;
  *) printf 'This preview supports Ubuntu 22.04/24.04/26.04 LTS and Debian 12/13, x86-64.\n' >&2; exit 1 ;;
esac
release_url="${GUARD_RELEASE_URL:-https://github.com/SpeakQuery/metric-owl/releases/download/v0.1.0-preview.8}"
[[ "$release_url" =~ ^https://github\.com/SpeakQuery/metric-owl/releases/download/v[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$ ]] || {
  printf 'GUARD_RELEASE_URL must identify an official, versioned Metric Owl GitHub release.\n' >&2; exit 1;
}
if ! command -v python3 >/dev/null || ! command -v curl >/dev/null || ! command -v ss >/dev/null || ! command -v runuser >/dev/null || ! dpkg-query -W -f='${Status}' ca-certificates 2>/dev/null | grep -q 'install ok installed'; then
  printf 'Installing the small set of download prerequisites…\n'
  apt-get update
  NEEDRESTART_MODE=l DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends python3 curl ca-certificates iproute2 util-linux
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
for asset in "$binary" install-linux.sh uninstall-linux.sh show-setup.sh connection-hints.sh configure-https.py server-guard.service LICENSE BINARY-LICENSE.txt THIRD-PARTY-NOTICES.txt release-metadata.json; do
  download "$asset"
  digest="$(awk -v filename="$asset" '$2==filename { print $1; found++ } END { if(found!=1) exit 1 }' "$stage/SHA256SUMS")" || {
    printf 'Missing or duplicate checksum for %s.\n' "$asset" >&2; exit 1;
  }
  [[ "$digest" =~ ^[a-fA-F0-9]{64}$ ]] || { printf 'Invalid release checksum.\n' >&2; exit 1; }
  (cd "$stage" && printf '%s  %s\n' "$digest" "$asset" | sha256sum -c -)
done
python3 - "$stage/release-metadata.json" "${release_url##*/}" "$ID-$VERSION_ID" <<'PY'
import json,re,sys
m=json.load(open(sys.argv[1]))
if (m.get('version')!=sys.argv[2] or not re.fullmatch(r'[a-f0-9]{40}',m.get('source_commit',''))
    or m.get('os')!='linux' or m.get('architecture')!='x86_64'
    or m.get('libc')!='musl' or m.get('linkage')!='static'
    or sys.argv[3] not in m.get('platforms', [])):
 raise SystemExit('Release provenance or platform metadata does not match this installation.')
PY
mv "$stage/$binary" "$stage/release/server-guard"
mv "$stage/install-linux.sh" "$stage/uninstall-linux.sh" "$stage/show-setup.sh" "$stage/connection-hints.sh" "$stage/configure-https.py" "$stage/scripts/"
mv "$stage/server-guard.service" "$stage/packaging/systemd/"
mv "$stage/LICENSE" "$stage/BINARY-LICENSE.txt" "$stage/THIRD-PARTY-NOTICES.txt" "$stage/release/"
(cd "$stage/release" && sha256sum server-guard > SHA256SUMS)
GUARD_INSTALL_QUIET=1 bash "$stage/scripts/install-linux.sh" "$stage/release"
if [[ "${GUARD_PREPARE_ADMIN:-1}" != 0 ]]; then
  printf '\nStep 1 of 2: Create your Metric Owl administrator account.\n'
  if ! python3 "$stage/scripts/configure-https.py" --prepare-admin; then
    printf '\nSETUP INCOMPLETE: Administrator setup did not finish.\nRun this installation command again to continue; existing accounts and history are preserved.\n' >&2
    exit 1
  fi
fi
if [[ "${GUARD_CONFIGURE_HTTPS:-1}" != 0 ]]; then
  printf '\nStep 2 of 2: Choose how to access your panel.\n'
  if ! python3 "$stage/scripts/configure-https.py"; then
    printf '\nSETUP INCOMPLETE: The application is running locally, but your browser address is not ready.\nCorrect the reported connection issue and run this installation command again.\nAny administrator account you created, settings and history are preserved.\n' >&2
    exit 1
  fi
fi
bash "$stage/scripts/show-setup.sh" /var/lib/server-guard ssh 4877 "$(systemctl show server-guard -p MainPID --value)"
