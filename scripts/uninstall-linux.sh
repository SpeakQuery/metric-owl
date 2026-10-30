#!/usr/bin/env bash
set -euo pipefail
[ "$(id -u)" = 0 ] || exit 1
systemctl disable --now server-guard
rm -f /etc/systemd/system/server-guard.service /usr/local/bin/server-guard
systemctl daemon-reload
printf 'Data preserved: /var/lib/server-guard\n'
