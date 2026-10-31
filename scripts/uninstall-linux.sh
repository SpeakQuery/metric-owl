#!/usr/bin/env bash
set -euo pipefail
set +x
[[ "$(id -u)" == 0 ]] || { printf 'Removal requires root privileges.\n' >&2; exit 1; }
scripts_dir="$(cd "$(dirname "$0")" && pwd)"
[[ ! -L "$scripts_dir/manage-installation.py" && -f "$scripts_dir/manage-installation.py" ]] || {
  printf 'The owned removal helper is missing or linked. Nothing was removed.\n' >&2; exit 1;
}
for argument in "$@"; do
  [[ "$argument" == --purge || "$argument" == --dry-run ]] || {
    printf 'Usage: uninstall-linux.sh [--purge] [--dry-run]\n' >&2; exit 1;
  }
done
exec python3 "$scripts_dir/manage-installation.py" uninstall "$@"
