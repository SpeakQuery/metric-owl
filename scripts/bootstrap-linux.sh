#!/usr/bin/env bash
# Public release entry point. Kept version-pinned; private preview testing uses
# install-private-release.py without exporting a GitHub credential to the server.
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
release_url="${GUARD_RELEASE_URL:-https://github.com/SpeakQuery/metric-owl/releases/download/v0.1.0-preview.2}"
[[ "$release_url" == https://* && "$release_url" != *'?'* && "$release_url" != *'#'* ]] || {
  printf 'GUARD_RELEASE_URL must identify a versioned HTTPS release.\n' >&2; exit 1;
}
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
download SHA256SUMS
binary="server-guard-linux-$architecture"
for asset in "$binary" install-linux.sh show-setup.sh connection-hints.sh server-guard.service LICENSE BINARY-LICENSE.txt THIRD-PARTY-NOTICES.txt; do
  download "$asset"
  digest="$(awk -v filename="$asset" '$2==filename { print $1; found++ } END { if(found!=1) exit 1 }' "$stage/SHA256SUMS")" || {
    printf 'Missing or duplicate checksum for %s.\n' "$asset" >&2; exit 1;
  }
  [[ "$digest" =~ ^[a-fA-F0-9]{64}$ ]] || { printf 'Invalid release checksum.\n' >&2; exit 1; }
  (cd "$stage" && printf '%s  %s\n' "$digest" "$asset" | sha256sum -c -)
done
mv "$stage/$binary" "$stage/release/server-guard"
mv "$stage/install-linux.sh" "$stage/show-setup.sh" "$stage/connection-hints.sh" "$stage/scripts/"
mv "$stage/server-guard.service" "$stage/packaging/systemd/"
mv "$stage/LICENSE" "$stage/BINARY-LICENSE.txt" "$stage/THIRD-PARTY-NOTICES.txt" "$stage/release/"
(cd "$stage/release" && sha256sum server-guard > SHA256SUMS)
bash "$stage/scripts/install-linux.sh" "$stage/release"
