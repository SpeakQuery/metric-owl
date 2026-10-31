#!/usr/bin/env bash
set -euo pipefail
set +x
[[ "$(id -u)" == 0 ]] || { printf 'Account recovery requires root privileges.\n' >&2; exit 1; }
[[ "$(uname -s)" == Linux ]] || { printf 'This helper requires Linux.\n' >&2; exit 1; }
[[ $# == 0 || ( $# == 1 && "$1" == --show-username ) ]] || { printf 'Usage: recover-admin.sh [--show-username]\n' >&2; exit 1; }
for path in /usr/local/bin/server-guard /var/lib/server-guard /var/lib/server-guard/guard.sqlite; do
  [[ -e "$path" && ! -L "$path" ]] || { printf 'Recovery refused an absent or linked application path.\n' >&2; exit 1; }
done
if [[ "${1:-}" == --show-username ]]; then
  exec /usr/local/bin/server-guard --data-dir /var/lib/server-guard recover-admin --show-username
fi
was_active=0
if systemctl is-active --quiet server-guard; then was_active=1; fi
restart_if_needed() {
  local outcome=$?
  trap - EXIT
  if (( was_active )); then
    if ! systemctl start server-guard; then
      printf 'Recovery helper could not restart Metric Owl. Run: sudo systemctl start server-guard\n' >&2
      exit 1
    fi
  fi
  exit "$outcome"
}
trap restart_if_needed EXIT
systemctl stop server-guard
/usr/local/bin/server-guard --data-dir /var/lib/server-guard recover-admin
