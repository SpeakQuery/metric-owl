#!/usr/bin/env bash
set -euo pipefail
[ "$(id -u)" = 0 ] || { printf 'Installation requires root privileges.\n' >&2; exit 1; }
[ "$(uname -s)" = Linux ] || { printf 'This installer requires Linux.\n' >&2; exit 1; }
set +x
for tool in curl sha256sum systemctl install useradd mktemp; do
  command -v "$tool" >/dev/null || { printf 'Required command is missing: %s\n' "$tool" >&2; exit 1; }
done
bundle="${1:?Usage: install-linux.sh <release-directory>}"
bundle="$(cd "$bundle" && pwd)"
(cd "$bundle" && sha256sum -c SHA256SUMS)
scripts_dir="$(cd "$(dirname "$0")" && pwd)"
[[ -f "$scripts_dir/show-setup.sh" && -f "$scripts_dir/connection-hints.sh" && -f "$scripts_dir/../packaging/systemd/server-guard.service" ]] || {
  printf 'The installation bundle is incomplete.\n' >&2; exit 1;
}
backup="$(mktemp -d)"
trap 'rm -rf "$backup"' EXIT
# Catch an incompatible executable before stopping an existing installation.
# A downloaded file need not already have its executable permission bit set.
install -m 755 "$bundle/server-guard" "$backup/candidate"
"$backup/candidate" --help >/dev/null
had_binary=0; had_unit=0; was_active=0; was_enabled=0; install_started=0
if [[ -f /usr/local/bin/server-guard ]]; then cp -p /usr/local/bin/server-guard "$backup/server-guard"; had_binary=1; fi
if [[ -f /etc/systemd/system/server-guard.service ]]; then cp -p /etc/systemd/system/server-guard.service "$backup/server-guard.service"; had_unit=1; fi
if systemctl is-active --quiet server-guard; then was_active=1; fi
if systemctl is-enabled --quiet server-guard; then was_enabled=1; fi
rollback() {
  trap - ERR
  set +e
  restore_failed=0
  printf 'Startup verification failed. Restoring the previous application files.\n' >&2
  systemctl stop server-guard 2>/dev/null || true
  if (( had_binary )); then install -m 755 "$backup/server-guard" /usr/local/bin/server-guard || restore_failed=1;
  else rm -f /usr/local/bin/server-guard || restore_failed=1; fi
  if (( had_unit )); then install -m 644 "$backup/server-guard.service" /etc/systemd/system/server-guard.service || restore_failed=1;
  else systemctl disable server-guard 2>/dev/null || true; rm -f /etc/systemd/system/server-guard.service || restore_failed=1; fi
  systemctl daemon-reload || restore_failed=1
  if (( !was_enabled )); then systemctl disable server-guard 2>/dev/null || true; fi
  if (( was_active )); then systemctl start server-guard || restore_failed=1; fi
  if (( restore_failed )); then
    trap - EXIT
    printf 'Automatic restoration was incomplete. Recovery files are retained at: %s\n' "$backup" >&2
  fi
  printf 'Existing account and history files have been retained. Check the service before retrying.\n' >&2
  exit 1
}
trap 'if (( install_started )); then rollback; fi' ERR
id server-guard >/dev/null 2>&1 || useradd --system --home /var/lib/server-guard --shell /usr/sbin/nologin server-guard
install -d -o server-guard -g server-guard -m 700 /var/lib/server-guard
bash "$scripts_dir/connection-hints.sh" write /var/lib/server-guard
install_started=1
systemctl stop server-guard 2>/dev/null || true
install -m 755 "$backup/candidate" /usr/local/bin/server-guard.new
mv /usr/local/bin/server-guard.new /usr/local/bin/server-guard
install -m 644 "$scripts_dir/../packaging/systemd/server-guard.service" /etc/systemd/system/server-guard.service
install -d -m 755 /usr/share/doc/server-guard
for notice in LICENSE BINARY-LICENSE.txt THIRD-PARTY-NOTICES.txt; do
  if [[ -f "$bundle/$notice" ]]; then install -m 644 "$bundle/$notice" "/usr/share/doc/server-guard/$notice"; fi
done
install -m 644 "$scripts_dir/show-setup.sh" "$scripts_dir/connection-hints.sh" /usr/share/doc/server-guard/
if [[ -f "$scripts_dir/configure-https.py" ]]; then install -m 644 "$scripts_dir/configure-https.py" /usr/share/doc/server-guard/; fi
systemctl daemon-reload
systemctl enable --now server-guard || rollback
if [[ "${GUARD_INSTALL_QUIET:-0}" == 1 ]]; then
  GUARD_HIDE_SETUP_KEY=1 bash "$scripts_dir/show-setup.sh" /var/lib/server-guard ssh 4877 "$(systemctl show server-guard -p MainPID --value)" >/dev/null || rollback
else
  bash "$scripts_dir/show-setup.sh" /var/lib/server-guard ssh 4877 "$(systemctl show server-guard -p MainPID --value)" || rollback
fi
trap - ERR
