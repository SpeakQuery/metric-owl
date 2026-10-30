#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[ "$(uname -s)" = Linux ] || { echo 'Build this package in an actual Linux environment.' >&2; exit 1; }
[[ "$(uname -m)" == x86_64 ]] || { echo 'Only the tested Ubuntu x86-64 preview is packaged.' >&2; exit 1; }
source /etc/os-release
[[ "${ID:-}" == ubuntu && "${VERSION_ID:-}" == 24.04 ]] || { echo 'Build this preview package on Ubuntu 24.04.' >&2; exit 1; }
arch=amd64
version=0.1.0~preview.2
file artifacts/release/server-guard | grep -q ELF || { echo 'A Linux ELF binary is required.' >&2; exit 1; }
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir -p "$stage/DEBIAN" "$stage/usr/local/bin" "$stage/lib/systemd/system" "$stage/usr/share/doc/server-guard"
cp artifacts/release/server-guard "$stage/usr/local/bin/"
cp packaging/systemd/server-guard.service "$stage/lib/systemd/system/"
cp LICENSE "$stage/usr/share/doc/server-guard/copyright"
cp BINARY-LICENSE.txt "$stage/usr/share/doc/server-guard/"
cp scripts/show-setup.sh "$stage/usr/share/doc/server-guard/show-setup.sh"
cp scripts/connection-hints.sh "$stage/usr/share/doc/server-guard/connection-hints.sh"
cp -R artifacts/release/third-party-licenses "$stage/usr/share/doc/server-guard/"
cat > "$stage/DEBIAN/control" <<CONTROL
Package: server-guard
Version: $version
Section: admin
Priority: optional
Architecture: $arch
Maintainer: Metric Owl <services@metricowl.cloud>
Depends: libc6 (>= 2.39), curl, bash, ca-certificates, systemd
Description: Metric Owl private preview for Ubuntu 24.04
 Local read-only server monitoring with a secure web panel.
CONTROL
cat > "$stage/DEBIAN/postinst" <<'POST'
#!/bin/sh
set -e
id server-guard >/dev/null 2>&1 || useradd --system --home /var/lib/server-guard --shell /usr/sbin/nologin server-guard
install -d -o server-guard -g server-guard -m 700 /var/lib/server-guard
bash /usr/share/doc/server-guard/connection-hints.sh write /var/lib/server-guard
systemctl daemon-reload
systemctl enable server-guard
systemctl restart server-guard
GUARD_HIDE_SETUP_KEY=1 bash /usr/share/doc/server-guard/show-setup.sh /var/lib/server-guard ssh 4877 "$(systemctl show server-guard -p MainPID --value)"
POST
cat > "$stage/DEBIAN/prerm" <<'PRE'
#!/bin/sh
set -e
systemctl stop server-guard || true
if [ "$1" = remove ]; then systemctl disable server-guard || true; fi
PRE
chmod 755 "$stage/DEBIAN/postinst" "$stage/DEBIAN/prerm" "$stage/usr/local/bin/server-guard"
dpkg-deb --build --root-owner-group "$stage" "artifacts/release/server-guard_${version}_${arch}.deb"
