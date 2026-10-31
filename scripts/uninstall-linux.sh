#!/usr/bin/env bash
set -euo pipefail
[ "$(id -u)" = 0 ] || exit 1
scripts_dir="$(cd "$(dirname "$0")" && pwd)"
if [[ -f /etc/metric-owl/panel-access.json ]]; then
  [[ -f "$scripts_dir/configure-https.py" ]] || { printf 'The HTTPS removal helper is missing. Nothing was removed.\n' >&2; exit 1; }
  python3 "$scripts_dir/configure-https.py" --uninstall
fi
systemctl disable --now server-guard || true
rm -f /etc/systemd/system/server-guard.service /usr/local/bin/server-guard
systemctl daemon-reload
printf 'Data preserved: /var/lib/server-guard\n'
printf 'Existing websites and non-Metric-Owl firewall rules were not removed.\n'
